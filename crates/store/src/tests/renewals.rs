//! A plan that renews by itself: its first payment, which Stripe bills by
//! invoice, and each renewal after it.

use portal_core::payment::stripe::{BillingReason, Event, Renewal, StripeId};
use portal_core::payment::{OrderId, ReviewReason};

use super::paying::*;
use super::*;
use crate::{Order, Progress, Recorded, StrayReason};

const WAITING: Progress = Progress::Granted { pulled: false };

/// A store with one customer, registered with `address` (digested once
/// by the caller: registering is slow).
fn store_with(address: &DigestedEmail) -> (Store, Account) {
    let mut store = shop();
    let account = store
        .register(
            NewAccount {
                code: AccountCode::from_bits(1),
                email: address.clone(),
                password_hash: hash("first"),
                created_at: at(NOW),
            },
            100,
        )
        .unwrap();
    store
        .start_session(&session(&account, 1, NOW, 1_000_000))
        .unwrap();
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    (store, account)
}

/// What Stripe says when the subscription `sub_1` begins.
fn beginning(order: OrderId) -> [Event; 4] {
    [
        event(1, 60, completed(1, order, &auto())),
        event(2, 61, invoiced("in_1", "sub_1", BillingReason::Create, 750)),
        event(3, 62, invoice_paid_by("in_1", "pi_1")),
        event(4, 50, charged("pi_1", 1)),
    ]
}

/// What Stripe says when `sub_1` renews, a month on.
const MONTH: u64 = 30 * 86_400;
fn renewing() -> [Event; 3] {
    [
        event(
            11,
            MONTH,
            invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
        ),
        event(12, MONTH + 1, invoice_paid_by("in_2", "pi_2")),
        event(13, MONTH - 5, charged("pi_2", 1)),
    ]
}

#[test]
fn a_subscriptions_first_payment_is_found_through_its_invoice() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &auto(), 1);
    let [checkout, first_invoice, paid_by, charge] = beginning(order);
    // The checkout names a subscription and an invoice, but no payment.
    record_all(&mut store, &[checkout, first_invoice]);
    assert_eq!(progress(&store, &account, order), Progress::Checking);
    // The card of some payment: not yet known to be this order's.
    record_all(&mut store, &[charge]);
    assert_eq!(progress(&store, &account, order), Progress::Checking);
    assert_eq!(entries(&store), []);
    // The invoice's payment ties them together.
    record_all(&mut store, &[paid_by]);
    assert_eq!(progress(&store, &account, order), WAITING);
    assert_eq!(entries(&store), [entry("grant", order, None)]);
    // The first invoice made no order of its own, and nothing is astray.
    assert_eq!(rows(&store, "orders"), 1);
    assert_eq!(rows(&store, "stray_payments"), 0);
}

#[test]
fn the_first_payment_is_granted_in_whatever_order_stripe_tells_of_it() {
    let address = digested("a@qq.com");
    let mut seen = 0;
    for ordering in orderings(&[0_usize, 1, 2, 3]) {
        let (mut store, account) = store_with(&address);
        let order = place(&mut store, 1, &auto(), 1);
        let events = beginning(order);
        for index in &ordering {
            assert_eq!(record(&mut store, &events[*index]), Recorded::New);
        }
        assert_eq!(progress(&store, &account, order), WAITING, "{ordering:?}");
        assert_eq!(
            entries(&store),
            [entry("grant", order, None)],
            "{ordering:?}"
        );
        assert_eq!(rows(&store, "stray_payments"), 0, "{ordering:?}");
        seen += 1;
    }
    assert_eq!(seen, 24);
}

#[test]
fn a_renewal_makes_an_order_like_the_first_and_is_granted_the_same_way() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &auto(), 1);
    record_all(&mut store, &beginning(first));
    let [invoice, paid_by, charge] = renewing();
    record_all(&mut store, std::slice::from_ref(&invoice));
    let renewal = OrderId::for_renewal(&sid("in_2"));
    assert_eq!(
        order_of(&store, &account, renewal),
        Order {
            id: renewal,
            user: user("pilotusera"),
            terms: auto(),
            created_at: at(NOW + MONTH),
            paid_at: Some(at(NOW + MONTH)),
            progress: Progress::Checking,
            refunded: 0,
            refunded_in_full: false,
            disputed: false,
            renewal_of: Some(first),
        }
    );
    // Its invoice told of again, under another id, makes no second order.
    record_all(
        &mut store,
        &[event(
            14,
            MONTH + 2,
            invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
        )],
    );
    assert_eq!(rows(&store, "orders"), 2);
    record_all(&mut store, &[paid_by, charge]);
    assert_eq!(progress(&store, &account, renewal), WAITING);
    assert_eq!(
        entries(&store),
        [entry("grant", first, None), entry("grant", renewal, None)]
    );
    // The newest first, as its owner lists them.
    let listed: Vec<OrderId> = store
        .orders_of(account.id, 10)
        .unwrap()
        .iter()
        .map(|order| order.id)
        .collect();
    assert_eq!(listed, [renewal, first]);
    // A second renewal is another order of the same first.
    record_all(
        &mut store,
        &[
            event(
                21,
                2 * MONTH,
                invoiced("in_3", "sub_1", BillingReason::Cycle, 750),
            ),
            event(22, 2 * MONTH, invoice_paid_by("in_3", "pi_3")),
            event(23, 2 * MONTH, charged("pi_3", 1)),
        ],
    );
    let second = OrderId::for_renewal(&sid("in_3"));
    assert_eq!(order_of(&store, &account, second).renewal_of, Some(first));
    assert_eq!(entries(&store).len(), 3);
    // There is still one subscription to show for it, to its owner and to
    // the operator.
    assert_eq!(store.renewals_of(account.id).unwrap().len(), 1);
    assert_eq!(store.open_renewals().unwrap().len(), 1);
}

#[test]
fn every_renewal_is_of_the_order_that_began_the_subscription() {
    // Stripe's clock and the portal's are two clocks: were renewals said
    // to be made before the first order was placed, each is still the
    // first order's, not the renewal's before it.
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &auto(), 1);
    record_all(&mut store, &beginning(first));
    for (n, invoice, before) in [(11, "in_2", 200), (12, "in_3", 100)] {
        let made = Event {
            created: at(NOW - before),
            ..event(n, 0, invoiced(invoice, "sub_1", BillingReason::Cycle, 750))
        };
        assert_eq!(record(&mut store, &made), Recorded::New);
    }
    for invoice in ["in_2", "in_3"] {
        assert_eq!(
            order_of(&store, &account, OrderId::for_renewal(&sid(invoice))).renewal_of,
            Some(first),
            "{invoice}"
        );
    }
}

/// A renewal of `sub_1` at another price, two months on.
fn renewing_short() -> Event {
    event(
        21,
        2 * MONTH,
        invoiced("in_3", "sub_1", BillingReason::Cycle, 700),
    )
}

#[test]
fn a_renewal_told_of_before_its_subscriptions_checkout_is_the_orders_once_that_comes() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &auto(), 1);
    // Stripe's delivery of the checkout failed, and is made again after
    // two renewals. Another subscription renews too, which no order ever
    // begins; and the first is billed once for something other than a
    // renewal.
    record_all(&mut store, &renewing());
    record_all(
        &mut store,
        &[
            renewing_short(),
            event(
                31,
                MONTH,
                invoiced("in_9", "sub_9", BillingReason::Cycle, 750),
            ),
            event(
                32,
                MONTH + 2,
                invoiced("in_8", "sub_1", BillingReason::Other, 120),
            ),
        ],
    );
    let reasons = |store: &Store| -> Vec<(StrayReason, Option<StripeId>)> {
        store
            .stray_payments()
            .unwrap()
            .into_iter()
            .map(|stray| (stray.reason, stray.subscription))
            .collect()
    };
    assert_eq!(rows(&store, "orders"), 1);
    assert_eq!(
        reasons(&store),
        [
            (StrayReason::NoSubscription, Some(sid("sub_1"))),
            (StrayReason::NoSubscription, Some(sid("sub_1"))),
            (StrayReason::NoSubscription, Some(sid("sub_9"))),
            (StrayReason::OtherInvoice, Some(sid("sub_1"))),
        ]
    );
    record_all(&mut store, &beginning(first));
    // Each is the order it would have been: made when Stripe said, not
    // when the checkout came, and judged as any renewal is.
    let renewal = OrderId::for_renewal(&sid("in_2"));
    assert_eq!(
        order_of(&store, &account, renewal),
        Order {
            id: renewal,
            user: user("pilotusera"),
            terms: auto(),
            created_at: at(NOW + MONTH),
            paid_at: Some(at(NOW + MONTH)),
            progress: WAITING,
            refunded: 0,
            refunded_in_full: false,
            disputed: false,
            renewal_of: Some(first),
        }
    );
    let short = order_of(&store, &account, OrderId::for_renewal(&sid("in_3")));
    assert_eq!(short.progress, Progress::Review(ReviewReason::Amount));
    assert_eq!(short.created_at, at(NOW + 2 * MONTH));
    assert_eq!(short.renewal_of, Some(first));
    assert_eq!(progress(&store, &account, first), WAITING);
    // The renewal's payment was known already, so it was entered with
    // the checkout; the first order's once its own was.
    assert_eq!(
        entries(&store),
        [entry("grant", renewal, None), entry("grant", first, None)]
    );
    // The other subscription's renewal is still no order's, and so is
    // what is not a renewal.
    assert_eq!(
        reasons(&store),
        [
            (StrayReason::NoSubscription, Some(sid("sub_9"))),
            (StrayReason::OtherInvoice, Some(sid("sub_1"))),
        ]
    );
    // Told of again, a renewal makes no second order and is not astray.
    record_all(
        &mut store,
        &[event(
            41,
            MONTH + 9,
            invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
        )],
    );
    assert_eq!(rows(&store, "orders"), 3);
    assert_eq!(rows(&store, "stray_payments"), 2);
}

#[test]
fn a_renewal_and_its_subscriptions_checkout_come_to_the_same_in_whatever_order() {
    let address = digested("a@qq.com");
    let mut seen = 0;
    for ordering in orderings(&[0_usize, 1, 2, 3]) {
        let (mut store, account) = store_with(&address);
        let first = place(&mut store, 1, &auto(), 1);
        let [checkout, rest @ ..] = beginning(first);
        record_all(&mut store, &rest);
        let [invoice, paid_by, charge] = renewing();
        let events = [checkout, invoice, paid_by, charge];
        for index in &ordering {
            assert_eq!(record(&mut store, &events[*index]), Recorded::New);
        }
        let renewal = OrderId::for_renewal(&sid("in_2"));
        assert_eq!(progress(&store, &account, first), WAITING, "{ordering:?}");
        assert_eq!(progress(&store, &account, renewal), WAITING, "{ordering:?}");
        assert_eq!(
            order_of(&store, &account, renewal).created_at,
            at(NOW + MONTH),
            "{ordering:?}"
        );
        let mut granted = entries(&store);
        granted.sort();
        let mut both = [entry("grant", first, None), entry("grant", renewal, None)];
        both.sort();
        assert_eq!(granted, both, "{ordering:?}");
        assert_eq!(rows(&store, "stray_payments"), 0, "{ordering:?}");
        seen += 1;
    }
    assert_eq!(seen, 24);
}

#[test]
fn a_renewal_is_granted_in_whatever_order_stripe_tells_of_it() {
    let address = digested("a@qq.com");
    let mut seen = 0;
    for ordering in orderings(&[0_usize, 1, 2]) {
        let (mut store, account) = store_with(&address);
        let first = place(&mut store, 1, &auto(), 1);
        record_all(&mut store, &beginning(first));
        let events = renewing();
        for index in &ordering {
            assert_eq!(record(&mut store, &events[*index]), Recorded::New);
        }
        let renewal = OrderId::for_renewal(&sid("in_2"));
        assert_eq!(progress(&store, &account, renewal), WAITING, "{ordering:?}");
        assert_eq!(
            entries(&store),
            [entry("grant", first, None), entry("grant", renewal, None)],
            "{ordering:?}"
        );
        seen += 1;
    }
    assert_eq!(seen, 6);
}

#[test]
fn a_renewal_at_another_price_or_with_a_bad_card_waits_for_the_operator() {
    let mut store = shop();
    let banned = register(&mut store, "banned@qq.com", 9);
    store
        .ban(&banned.code, &[fingerprint(7).digest()], at(NOW))
        .unwrap();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &auto(), 1);
    record_all(&mut store, &beginning(first));
    record_all(
        &mut store,
        &[
            // Not the first order's price.
            event(
                11,
                MONTH,
                invoiced("in_2", "sub_1", BillingReason::Cycle, 700),
            ),
            event(12, MONTH, invoice_paid_by("in_2", "pi_2")),
            event(13, MONTH, charged("pi_2", 1)),
            // Its price, with a banned account's card.
            event(
                21,
                2 * MONTH,
                invoiced("in_3", "sub_1", BillingReason::Cycle, 750),
            ),
            event(22, 2 * MONTH, invoice_paid_by("in_3", "pi_3")),
            event(23, 2 * MONTH, charged("pi_3", 7)),
        ],
    );
    assert_eq!(
        progress(&store, &account, OrderId::for_renewal(&sid("in_2"))),
        Progress::Review(ReviewReason::Amount)
    );
    assert_eq!(
        progress(&store, &account, OrderId::for_renewal(&sid("in_3"))),
        Progress::Review(ReviewReason::CardBanned)
    );
    assert_eq!(entries(&store), [entry("grant", first, None)]);
    // And one for an account that has since been unbound.
    store.unbind(&account.code).unwrap();
    record_all(
        &mut store,
        &[
            event(
                31,
                3 * MONTH,
                invoiced("in_4", "sub_1", BillingReason::Cycle, 750),
            ),
            event(32, 3 * MONTH, invoice_paid_by("in_4", "pi_4")),
            event(33, 3 * MONTH, charged("pi_4", 1)),
        ],
    );
    assert_eq!(
        progress(&store, &account, OrderId::for_renewal(&sid("in_4"))),
        Progress::Review(ReviewReason::Binding)
    );
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn a_renewal_of_a_first_order_that_waits_is_judged_by_itself() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &auto(), 1);
    // The first payment was made from a wallet.
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, first, &auto())),
            event(2, 61, invoice_paid_by("in_1", "pi_1")),
            event(
                3,
                50,
                charged_otherwise("pi_1", portal_core::payment::stripe::CardUsed::Wallet),
            ),
        ],
    );
    assert_eq!(
        progress(&store, &account, first),
        Progress::Review(ReviewReason::CardUnchecked)
    );
    // The renewal, paid with a card typed in, goes through.
    record_all(&mut store, &renewing());
    let renewal = OrderId::for_renewal(&sid("in_2"));
    assert_eq!(progress(&store, &account, renewal), WAITING);
    assert_eq!(entries(&store), [entry("grant", renewal, None)]);
}

#[test]
fn a_subscription_keeps_the_state_stripe_made_last_and_once_ended_stays_ended() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &auto(), 1);
    record_all(&mut store, &[event(1, 0, completed(1, order, &auto()))]);
    let state = |store: &Store| -> Option<Renewal> {
        store
            .renewals_of(account.id)
            .unwrap()
            .first()
            .map(|renewal| renewal.state)
    };
    assert_eq!(state(&store), Some(Renewal::Active));
    let mut n = 1;
    let mut tell = |store: &mut Store, made: u64, renewal: Renewal| {
        n += 1;
        record_all(store, &[event(n, made, subscription("sub_1", renewal))]);
    };
    tell(&mut store, 100, Renewal::Ending);
    assert_eq!(state(&store), Some(Renewal::Ending));
    // The payer changes their mind.
    tell(&mut store, 200, Renewal::Active);
    assert_eq!(state(&store), Some(Renewal::Active));
    // What was made earlier and arrives late does not undo it; nor does
    // it make the next late one look new.
    tell(&mut store, 150, Renewal::Ending);
    assert_eq!(state(&store), Some(Renewal::Active));
    tell(&mut store, 160, Renewal::Ending);
    assert_eq!(state(&store), Some(Renewal::Active));
    // Two made in the same second: the one that arrives last.
    tell(&mut store, 200, Renewal::Ending);
    assert_eq!(state(&store), Some(Renewal::Ending));
    // Over is over, even when told of with an earlier time, and whatever
    // arrives after.
    tell(&mut store, 10, Renewal::Ended);
    assert_eq!(state(&store), None);
    tell(&mut store, 900, Renewal::Active);
    tell(&mut store, 901, Renewal::Ending);
    assert_eq!(state(&store), None);
    assert_eq!(rows(&store, "stripe_subscriptions"), 1);
    // A state told of before its checkout is known holds once it is.
    let other = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    tell(&mut store, 50, Renewal::Ending);
    record_all(
        &mut store,
        &[event(90, 60, subscription("sub_2", Renewal::Ending))],
    );
    let second = place(&mut store, 2, &auto(), 2);
    record_all(&mut store, &[event(91, 70, completed(2, second, &auto()))]);
    assert_eq!(
        store.renewals_of(other.id).unwrap()[0].state,
        Renewal::Ending
    );
}

#[test]
fn were_two_invoices_paid_by_one_payment_the_second_order_would_wait_without_it() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &auto(), 1);
    record_all(&mut store, &beginning(first));
    record_all(
        &mut store,
        &[
            event(
                11,
                MONTH,
                invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
            ),
            // Stripe never says this; were it to, the payment is the
            // first order's and no second order's.
            event(12, MONTH, invoice_paid_by("in_2", "pi_1")),
        ],
    );
    let renewal = OrderId::for_renewal(&sid("in_2"));
    assert_eq!(progress(&store, &account, renewal), Progress::Checking);
    assert_eq!(progress(&store, &account, first), WAITING);
    // Later events are still recorded.
    record_all(
        &mut store,
        &[event(13, MONTH, refunded("pi_1", 100, false))],
    );
    assert_eq!(order_of(&store, &account, first).refunded, 100);
    assert_eq!(order_of(&store, &account, renewal).refunded, 0);
    // An invoice's payment told of twice: the first stands.
    record_all(
        &mut store,
        &[event(14, MONTH, invoice_paid_by("in_1", "pi_8"))],
    );
    let paid_by: String = store
        .conn
        .query_row(
            "SELECT payment_intent FROM stripe_invoice_payments WHERE invoice = 'in_1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(paid_by, "pi_1");
}
