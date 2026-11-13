//! Payments that are no order's, and what the portal forgets.

use portal_core::payment::CardVerdict;
use portal_core::payment::stripe::{BillingReason, CheckoutSession, EventKind, Renewal};

use super::paying::*;
use super::*;
use crate::{DismissError, Progress, StoreError, StrayPayment, StrayReason, UnmatchedPayment};

/// A checkout on the month's page that names `order`.
fn stray_checkout(n: u8, order: Option<portal_core::payment::OrderId>) -> EventKind {
    EventKind::CheckoutCompleted(CheckoutSession {
        order,
        ..checkout(n, oid(1), &month())
    })
}

#[test]
fn a_checkout_that_names_no_order_of_the_portal_waits_for_the_operator() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            // No reference at all, and one that is no order's.
            event(1, 60, stray_checkout(1, None)),
            event(2, 61, stray_checkout(2, Some(oid(9)))),
            event(3, 50, charged("pi_2", 1)),
        ],
    );
    assert_eq!(progress(&store, &account, order), Progress::Pending);
    assert_eq!(entries(&store), []);
    let stray = |id, session: &str, payment_intent: &str, card| StrayPayment {
        id,
        noticed_at: at(NOW + 60 + id),
        reason: StrayReason::NoOrder,
        session: Some(sid(session)),
        invoice: None,
        payment_intent: Some(sid(payment_intent)),
        subscription: None,
        amount: Some(800),
        currency: Some(gbp()),
        card,
        refunded: 0,
        refunded_in_full: false,
        open_subscription: None,
    };
    assert_eq!(
        store.stray_payments().unwrap(),
        [
            stray(1, "cs_1", "pi_1", None),
            stray(2, "cs_2", "pi_2", Some(CardVerdict::Clear)),
        ]
    );
    // Told of again, each is still one.
    record_all(&mut store, &[event(4, 62, stray_checkout(1, None))]);
    assert_eq!(store.stray_payments().unwrap().len(), 2);
}

#[test]
fn a_second_checkout_for_an_order_already_paid_is_no_orders() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    let again = |n| {
        EventKind::CheckoutCompleted(CheckoutSession {
            order: Some(order),
            ..checkout(n, order, &month())
        })
    };
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 50, charged("pi_1", 1)),
            event(3, 70, again(2)),
            event(4, 71, charged("pi_2", 1)),
        ],
    );
    // The first payment is the order's and is granted once.
    assert_eq!(entries(&store), [entry("grant", order, None)]);
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 1);
    assert_eq!(strays[0].reason, StrayReason::PaidAgain);
    assert_eq!(strays[0].session, Some(sid("cs_2")));
    assert_eq!(strays[0].card, Some(CardVerdict::Clear));
    // So is one for an order that waits for the operator.
    let waiting = place(&mut store, 1, &pack(), 2);
    let short = CheckoutSession {
        amount_total: Some(1),
        ..checkout(3, waiting, &pack())
    };
    record_all(
        &mut store,
        &[
            event(5, 80, EventKind::CheckoutCompleted(short)),
            event(6, 81, completed(4, waiting, &pack())),
        ],
    );
    assert!(matches!(
        progress(&store, &account, waiting),
        Progress::Review(_)
    ));
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 2);
    assert_eq!(
        (
            strays[1].reason,
            strays[1].session.clone(),
            strays[1].amount
        ),
        (StrayReason::PaidAgain, Some(sid("cs_4")), Some(300))
    );
}

#[test]
fn a_stray_payment_refunded_in_full_needs_no_one_any_more() {
    let mut store = shop();
    record_all(
        &mut store,
        &[
            event(1, 60, stray_checkout(1, None)),
            event(2, 61, stray_checkout(2, None)),
            // Part of the first goes back: it still waits, and says so.
            event(3, 100, refunded("pi_1", 300, false)),
        ],
    );
    let strays = store.stray_payments().unwrap();
    assert_eq!(
        strays
            .iter()
            .map(|stray| stray.refunded)
            .collect::<Vec<_>>(),
        [300, 0]
    );
    // All of it: gone from the list, the other still there.
    record_all(&mut store, &[event(4, 200, refunded("pi_1", 800, true))]);
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 1);
    assert_eq!(strays[0].session, Some(sid("cs_2")));
    // A refund that came first settles it as soon as it is known.
    record_all(
        &mut store,
        &[
            event(5, 300, refunded("pi_3", 800, true)),
            event(6, 301, stray_checkout(3, None)),
        ],
    );
    assert_eq!(store.stray_payments().unwrap().len(), 1);
}

#[test]
fn the_operator_can_take_a_stray_payment_off_the_list() {
    let mut store = shop();
    record_all(
        &mut store,
        &[
            event(1, 60, stray_checkout(1, None)),
            event(2, 61, stray_checkout(2, None)),
        ],
    );
    store.dismiss_stray(1, None).unwrap();
    for id in [1, 7] {
        assert!(matches!(store.dismiss_stray(id, None), Err(DismissError::Unknown(n)) if n == id));
    }
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 1);
    assert_eq!(strays[0].id, 2);
    // Its number is not given out again.
    record_all(&mut store, &[event(3, 62, stray_checkout(3, None))]);
    assert_eq!(store.stray_payments().unwrap()[1].id, 3);
    // Dismissed, the checkout told of again would be listed again: the
    // portal remembers events for a while, not what was dismissed.
    assert!(matches!(
        store.dismiss_stray(u64::MAX, None),
        Err(DismissError::Store(StoreError::OutOfRange { .. }))
    ));
}

#[test]
fn an_invoice_that_renews_nothing_the_portal_knows_waits_for_the_operator() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &auto(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &auto())),
            // A renewal of a subscription no order began.
            event(2, 100, invoiced("in_7", "sub_7", BillingReason::Cycle, 750)),
            // An invoice of the order's subscription that is neither its
            // first nor a renewal, and one of a subscription unknown.
            event(3, 101, invoiced("in_8", "sub_1", BillingReason::Other, 120)),
            event(4, 102, invoiced("in_9", "sub_7", BillingReason::Other, 50)),
            // A first invoice says nothing its checkout does not.
            event(
                5,
                103,
                invoiced("in_6", "sub_6", BillingReason::Create, 750),
            ),
            event(6, 104, invoice_paid_by("in_7", "pi_7")),
        ],
    );
    let strays = store.stray_payments().unwrap();
    let seen: Vec<_> = strays
        .iter()
        .map(|stray| {
            (
                stray.reason,
                stray.invoice.as_ref().map(ToString::to_string),
                stray.subscription.as_ref().map(ToString::to_string),
                stray.payment_intent.as_ref().map(ToString::to_string),
                stray.amount,
            )
        })
        .collect();
    let text = |text: &str| Some(text.to_owned());
    assert_eq!(
        seen,
        [
            (
                StrayReason::NoSubscription,
                text("in_7"),
                text("sub_7"),
                text("pi_7"),
                Some(750)
            ),
            (
                StrayReason::OtherInvoice,
                text("in_8"),
                text("sub_1"),
                None,
                Some(120)
            ),
            (
                StrayReason::OtherInvoice,
                text("in_9"),
                text("sub_7"),
                None,
                Some(50)
            ),
        ]
    );
    assert!(strays.iter().all(|stray| stray.session.is_none()));
    // None of them is an order, and each is listed once.
    assert_eq!(store.orders_of(account.id, 10).unwrap().len(), 1);
    record_all(
        &mut store,
        &[event(
            7,
            105,
            invoiced("in_7", "sub_7", BillingReason::Cycle, 750),
        )],
    );
    assert_eq!(store.stray_payments().unwrap().len(), 3);
    // The renewal's refund settles its payment, but the subscription is
    // still unknown and may charge again.
    record_all(&mut store, &[event(8, 200, refunded("pi_7", 750, true))]);
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 3);
    assert_eq!(
        (strays[0].refunded_in_full, strays[0].open_subscription),
        (true, Some(Renewal::Active))
    );
    // An extra invoice of an order's own subscription is a payment
    // problem alone. Cancelling sub_7 likewise leaves the decision of
    // whether to refund in_9, but clears the already refunded in_7.
    assert_eq!(strays[1].open_subscription, None);
    assert_eq!(strays[2].open_subscription, Some(Renewal::Active));
    record_all(
        &mut store,
        &[event(9, 201, subscription("sub_7", Renewal::Ended))],
    );
    assert_eq!(store.stray_payments().unwrap().len(), 2);
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays[1].invoice, Some(sid("in_9")));
    assert_eq!(strays[1].open_subscription, None);
    store.dismiss_stray(strays[0].id, None).unwrap();
    record_all(
        &mut store,
        &[
            event(10, 202, invoice_paid_by("in_9", "pi_9")),
            event(11, 203, refunded("pi_9", 50, true)),
        ],
    );
    assert!(store.stray_payments().unwrap().is_empty());
    for reason in StrayReason::ALL {
        assert_eq!(StrayReason::parse(reason.as_str()), Some(reason));
    }
    assert_eq!(
        StrayReason::ALL.map(StrayReason::as_str),
        ["no_order", "paid_again", "no_subscription", "other_invoice"]
    );
    assert_eq!(StrayReason::parse("No_order"), None);
}

#[test]
fn a_subscription_begun_without_an_order_is_astray_and_so_are_its_renewals() {
    let mut store = shop();
    let begun = CheckoutSession {
        order: None,
        ..checkout(1, oid(1), &auto())
    };
    record_all(
        &mut store,
        &[
            event(1, 60, EventKind::CheckoutCompleted(begun)),
            event(2, 61, invoice_paid_by("in_1", "pi_1")),
            event(
                3,
                3_000,
                invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
            ),
        ],
    );
    let strays = store.stray_payments().unwrap();
    let first = &strays[0];
    assert_eq!(first.reason, StrayReason::NoOrder);
    assert_eq!(
        (
            first.session.clone(),
            first.invoice.clone(),
            first.subscription.clone(),
            // Found through its invoice, as an order's would be.
            first.payment_intent.clone(),
        ),
        (
            Some(sid("cs_1")),
            Some(sid("in_1")),
            Some(sid("sub_1")),
            Some(sid("pi_1"))
        )
    );
    assert_eq!(strays[1].reason, StrayReason::NoSubscription);
    assert_eq!(strays.len(), 2);
    assert_eq!(rows(&store, "orders"), 0);
}

#[test]
fn a_stray_subscription_blocks_disabling_payments_until_it_has_ended() {
    let mut store = shop();
    let begun = CheckoutSession {
        order: None,
        ..checkout(1, oid(1), &auto())
    };
    // Stripe can confirm the refund before telling us about checkout.
    record_all(
        &mut store,
        &[
            event(1, 62, refunded("pi_1", 750, true)),
            event(2, 61, invoice_paid_by("in_1", "pi_1")),
            event(3, 60, EventKind::CheckoutCompleted(begun)),
        ],
    );
    assert!(store.payments_outstanding().unwrap());
    assert_eq!(entries(&store), []);
    record_all(
        &mut store,
        &[event(4, 100, subscription("sub_1", Renewal::Ended))],
    );
    assert!(!store.payments_outstanding().unwrap());
    assert!(store.stray_payments().unwrap().is_empty());
}

#[test]
fn the_operator_takes_off_a_payment_whose_subscription_stripe_may_charge_only_by_naming_it() {
    let mut store = shop();
    let begun = CheckoutSession {
        order: None,
        ..checkout(1, oid(1), &auto())
    };
    record_all(
        &mut store,
        &[
            event(1, 60, EventKind::CheckoutCompleted(begun)),
            event(2, 61, invoice_paid_by("in_1", "pi_1")),
            event(3, 62, refunded("pi_1", 750, true)),
            event(4, 63, stray_checkout(2, None)),
        ],
    );
    let open = |store: &Store| -> Vec<Option<Renewal>> {
        store
            .stray_payments()
            .unwrap()
            .iter()
            .map(|stray| stray.open_subscription)
            .collect()
    };
    // Each payment says what Stripe last said of its subscription, while
    // Stripe may charge that again.
    assert_eq!(open(&store), [Some(Renewal::Active), None]);
    record_all(
        &mut store,
        &[event(5, 70, subscription("sub_1", Renewal::Ending))],
    );
    assert_eq!(open(&store), [Some(Renewal::Ending), None]);
    // It stays unless the operator says its own subscription has ended;
    // a payment with no subscription has none to name.
    assert!(matches!(
        store.dismiss_stray(1, None),
        Err(DismissError::NeedsCancellation(1, ref open)) if *open == sid("sub_1")
    ));
    assert!(matches!(
        store.dismiss_stray(1, Some(&sid("sub_9"))),
        Err(DismissError::NotItsSubscription(1, ref named)) if *named == sid("sub_9")
    ));
    assert!(matches!(
        store.dismiss_stray(2, Some(&sid("sub_1"))),
        Err(DismissError::NotItsSubscription(2, _))
    ));
    assert_eq!(open(&store).len(), 2);
    assert!(store.payments_outstanding().unwrap());
    store.dismiss_stray(1, Some(&sid("sub_1"))).unwrap();
    assert_eq!(open(&store), [None]);
    assert!(!store.payments_outstanding().unwrap());
    // Wrongly said: Stripe charges the subscription again, and the
    // renewal is listed in its place.
    record_all(
        &mut store,
        &[event(
            6,
            3_000,
            invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
        )],
    );
    assert_eq!(open(&store), [None, Some(Renewal::Ending)]);
    assert!(store.payments_outstanding().unwrap());
}

#[test]
fn a_refund_or_dispute_of_a_payment_nothing_names_is_shown_as_such() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 61, charged("pi_1", 1)),
            event(3, 62, stray_checkout(2, None)),
            // Of the order's payment, of the stray one, and of two that
            // nothing names.
            event(4, 100, refunded("pi_1", 100, false)),
            event(5, 101, refunded("pi_2", 100, false)),
            event(6, 102, refunded("pi_8", 250, false)),
            event(7, 103, disputed("pi_9", 800)),
            event(8, 104, refunded("pi_9", 10, false)),
            // A charge that nothing names is no one's business.
            event(9, 105, charged("pi_5", 1)),
            // Disputed and never refunded; told of again with less, the
            // larger amount stands.
            event(10, 106, disputed("pi_7", 800)),
            event(11, 107, disputed("pi_7", 300)),
        ],
    );
    assert_eq!(
        store.unmatched_payments().unwrap(),
        [
            UnmatchedPayment {
                payment_intent: sid("pi_8"),
                refunded: 250,
                disputed: None,
            },
            UnmatchedPayment {
                payment_intent: sid("pi_9"),
                refunded: 10,
                disputed: Some(800),
            },
            UnmatchedPayment {
                payment_intent: sid("pi_7"),
                refunded: 0,
                disputed: Some(800),
            },
        ]
    );
    assert_eq!(order_of(&store, &account, order).refunded, 100);
}

#[test]
fn the_portal_forgets_events_and_what_never_became_an_orders() {
    const DAY: u64 = 86_400;
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &auto(), 1);
    record_all(
        &mut store,
        &[
            // The order's own: its checkout, its invoice's payment, its
            // card, its subscription's state.
            event(1, 0, completed(1, order, &auto())),
            event(2, 1, invoice_paid_by("in_1", "pi_1")),
            event(3, 2, charged("pi_1", 1)),
            event(4, 3, subscription("sub_1", Renewal::Ending)),
            // A stray payment's.
            event(5, 4, stray_checkout(2, None)),
            event(6, 5, charged("pi_2", 1)),
            // No one's.
            event(7, 6, charged("pi_9", 1)),
            event(8, 7, invoice_paid_by("in_9", "pi_9")),
            event(9, 8, subscription("sub_9", Renewal::Active)),
        ],
    );
    let counts = |store: &Store| {
        [
            "stripe_events",
            "stripe_payments",
            "stripe_invoice_payments",
            "stripe_subscriptions",
        ]
        .map(|table| rows(store, table))
    };
    assert_eq!(counts(&store), [9, 3, 2, 2]);
    // Forty-five days on, to the second, nothing has gone.
    let later = |store: &mut Store, n, after| {
        let event = event(n, after, EventKind::Unused);
        store
            .record_stripe_event(&event, &on_sale(), at(NOW + after))
            .unwrap();
    };
    // The first event was received a second after it was made.
    later(&mut store, 10, 45 * DAY + 1);
    assert_eq!(counts(&store), [10, 3, 2, 2]);
    // A second more: the first event's id is forgotten; the facts were
    // noticed later than that.
    later(&mut store, 11, 45 * DAY + 2);
    assert_eq!(counts(&store), [10, 3, 2, 2]);
    // No one's payment was noticed seven seconds in: forty-five days
    // after that, to the second, it is still remembered; a second more,
    // and it is not. The events received before then are forgotten.
    later(&mut store, 12, 45 * DAY + 7);
    assert_eq!(counts(&store), [6, 3, 2, 2]);
    later(&mut store, 13, 45 * DAY + 8);
    assert_eq!(counts(&store), [6, 2, 2, 2]);
    // Once all of it is older: the events go, and what no order and no
    // stray payment names; the rest stays for as long as they do.
    later(&mut store, 14, 45 * DAY + 20);
    assert_eq!(counts(&store), [5, 2, 1, 1]);
    let kept = |table: &str, key: &str| -> Vec<String> {
        store
            .conn
            .prepare(&format!("SELECT {key} FROM {table} ORDER BY {key}"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(kept("stripe_payments", "payment_intent"), ["pi_1", "pi_2"]);
    assert_eq!(kept("stripe_invoice_payments", "invoice"), ["in_1"]);
    assert_eq!(kept("stripe_subscriptions", "subscription"), ["sub_1"]);
    assert_eq!(
        progress(&store, &account, order),
        Progress::Granted { pulled: false }
    );
    // An event forgotten and delivered again would be read again; its
    // checkout is still known for the order's, so nothing is paid twice.
    record_all(&mut store, &[event(1, 0, completed(1, order, &auto()))]);
    assert_eq!(entries(&store).len(), 1);
    assert_eq!(rows(&store, "stray_payments"), 1);
}
