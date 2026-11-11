//! One payment for one order: what each of Stripe's events leads to, in
//! whatever order they arrive.

use portal_core::payment::ReviewReason;
use portal_core::payment::stripe::{
    CardUsed, CheckoutMode, CheckoutSession, EventKind, PaymentStatus,
};

use super::paying::*;
use super::*;
use crate::{Progress, Recorded};

const WAITING: Progress = Progress::Granted { pulled: false };

#[test]
fn a_paid_order_whose_card_passes_is_entered_for_mooring_to_grant() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    // The checkout alone: paid, but the card is not yet known.
    record_all(&mut store, &[event(1, 60, completed(1, order, &month()))]);
    let paid = order_of(&store, &account, order);
    assert_eq!(paid.progress, Progress::Checking);
    assert_eq!(paid.paid_at, Some(at(NOW + 60)));
    assert_eq!(entries(&store), []);
    // The charge: a card that is no banned account's.
    record_all(&mut store, &[event(2, 50, charged("pi_1", 1))]);
    assert_eq!(progress(&store, &account, order), WAITING);
    assert_eq!(entries(&store), [entry("grant", order, None)]);
    let recorded_at: i64 = store
        .conn
        .query_row("SELECT recorded_at FROM entitlements", [], |row| row.get(0))
        .unwrap();
    assert_eq!(recorded_at.cast_unsigned(), NOW + 51);
    // The card itself is nowhere in the database: only the verdict.
    let kept: (String, Option<String>) = store
        .conn
        .query_row(
            "SELECT payment_intent, card FROM stripe_payments",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(kept, ("pi_1".to_owned(), Some("clear".to_owned())));
}

#[test]
fn the_charge_may_come_before_the_checkout() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(&mut store, &[event(1, 50, charged("pi_1", 1))]);
    assert_eq!(progress(&store, &account, order), Progress::Pending);
    assert_eq!(entries(&store), []);
    record_all(&mut store, &[event(2, 60, completed(1, order, &month()))]);
    assert_eq!(progress(&store, &account, order), WAITING);
    assert_eq!(entries(&store), [entry("grant", order, None)]);
}

#[test]
fn an_event_delivered_twice_is_recorded_once() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    let checkout = event(1, 60, completed(1, order, &month()));
    let charge = event(2, 50, charged("pi_1", 1));
    record_all(&mut store, &[checkout.clone(), charge.clone()]);
    for again in [&checkout, &charge, &checkout] {
        assert_eq!(record(&mut store, again), Recorded::Duplicate);
    }
    assert_eq!(entries(&store), [entry("grant", order, None)]);
    assert_eq!(rows(&store, "stripe_events"), 2);
    assert_eq!(rows(&store, "stray_payments"), 0);
    // An id seen before is not read again, whatever comes with it.
    let forged = event(2, 50, refunded("pi_1", 800, true));
    assert_eq!(record(&mut store, &forged), Recorded::Duplicate);
    assert_eq!(order_of(&store, &account, order).refunded, 0);
    // The same checkout told of under another id pays nothing twice.
    record_all(&mut store, &[event(3, 61, completed(1, order, &month()))]);
    assert_eq!(entries(&store), [entry("grant", order, None)]);
    assert_eq!(rows(&store, "stray_payments"), 0);
}

#[test]
fn an_event_the_portal_does_not_act_on_changes_nothing_but_is_remembered() {
    let mut store = shop();
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    place(&mut store, 1, &month(), 1);
    let unused = event(1, 0, EventKind::Unused);
    assert_eq!(record(&mut store, &unused), Recorded::New);
    assert_eq!(record(&mut store, &unused), Recorded::Duplicate);
    assert_eq!(rows(&store, "stripe_events"), 1);
    for table in ["stripe_payments", "stray_payments", "entitlements"] {
        assert_eq!(rows(&store, table), 0, "{table}");
    }
}

#[test]
fn a_payment_that_settles_later_pays_the_order_when_it_has() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    let unpaid = CheckoutSession {
        status: PaymentStatus::Unpaid,
        ..checkout(1, order, &month())
    };
    record_all(
        &mut store,
        &[event(1, 60, EventKind::CheckoutCompleted(unpaid))],
    );
    assert_eq!(progress(&store, &account, order), Progress::Pending);
    assert_eq!(rows(&store, "stray_payments"), 0);
    // The same checkout, paid.
    record_all(
        &mut store,
        &[
            event(2, 3_600, completed(1, order, &month())),
            event(3, 3_601, charged("pi_1", 1)),
        ],
    );
    let paid = order_of(&store, &account, order);
    assert_eq!(paid.progress, WAITING);
    assert_eq!(paid.paid_at, Some(at(NOW + 3_600)));
}

/// A checkout for the month's order with one thing about it changed.
fn amiss(order: portal_core::payment::OrderId, reason: ReviewReason) -> CheckoutSession {
    let good = checkout(1, order, &month());
    match reason {
        ReviewReason::Unpaid => CheckoutSession {
            status: PaymentStatus::Other,
            ..good
        },
        ReviewReason::Link => CheckoutSession {
            payment_link: Some(sid("plink_1pack")),
            ..good
        },
        ReviewReason::Mode => CheckoutSession {
            mode: CheckoutMode::Subscription,
            ..good
        },
        ReviewReason::Amount => CheckoutSession {
            amount_total: Some(799),
            ..good
        },
        other => panic!("{other:?} is not about the checkout"),
    }
}

#[test]
fn a_payment_that_is_not_what_the_order_asked_for_waits_for_the_operator() {
    for reason in [
        ReviewReason::Unpaid,
        ReviewReason::Link,
        ReviewReason::Mode,
        ReviewReason::Amount,
    ] {
        let mut store = shop();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        let order = place(&mut store, 1, &month(), 1);
        record_all(
            &mut store,
            &[
                event(1, 60, EventKind::CheckoutCompleted(amiss(order, reason))),
                // A card that passes does not make it the order's payment.
                event(2, 50, charged("pi_1", 1)),
            ],
        );
        let waiting = order_of(&store, &account, order);
        assert_eq!(waiting.progress, Progress::Review(reason), "{reason:?}");
        assert_eq!(waiting.paid_at, Some(at(NOW + 60)));
        assert_eq!(entries(&store), [], "{reason:?}");
        // It is the order's checkout all the same: no payment is astray,
        // and told of again it changes nothing.
        record_all(
            &mut store,
            &[event(
                3,
                61,
                EventKind::CheckoutCompleted(amiss(order, reason)),
            )],
        );
        assert_eq!(rows(&store, "stray_payments"), 0, "{reason:?}");
        // The plan may be ordered again beside it.
        assert_eq!(place(&mut store, 1, &month(), 2), oid(2));
    }
}

#[test]
fn a_payment_from_a_wallet_or_by_another_method_waits_for_the_operator() {
    for card in [CardUsed::Wallet, CardUsed::Other] {
        for charge_first in [true, false] {
            let mut store = shop();
            let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
            let order = place(&mut store, 1, &month(), 1);
            let mut events = vec![
                event(1, 50, charged_otherwise("pi_1", card.clone())),
                event(2, 60, completed(1, order, &month())),
            ];
            if !charge_first {
                events.reverse();
            }
            record_all(&mut store, &events);
            assert_eq!(
                progress(&store, &account, order),
                Progress::Review(ReviewReason::CardUnchecked),
                "{card:?} {charge_first}"
            );
            assert_eq!(entries(&store), []);
        }
    }
}

#[test]
fn a_payment_with_a_banned_accounts_card_is_never_entered() {
    let mut store = shop();
    let banned = customer(&mut store, "banned@qq.com", 1, "pilotusera");
    let account = customer(&mut store, "a@qq.com", 2, "pilotuserb");
    let other = customer(&mut store, "b@qq.com", 3, "pilotuserc");
    store
        .ban(
            &banned.code,
            &[fingerprint(7).digest(), fingerprint(8).digest()],
            at(NOW),
        )
        .unwrap();
    let first = place(&mut store, 2, &month(), 1);
    let second = place(&mut store, 3, &pack(), 2);
    let third = place(&mut store, 2, &pack(), 3);
    record_all(
        &mut store,
        &[
            // Either of the banned account's cards, before or after the
            // checkout.
            event(1, 60, completed(1, first, &month())),
            event(2, 50, charged("pi_1", 7)),
            event(3, 50, charged("pi_2", 8)),
            event(4, 60, completed(2, second, &pack())),
            // Another card pays as ever.
            event(5, 60, completed(3, third, &pack())),
            event(6, 50, charged("pi_3", 9)),
        ],
    );
    assert_eq!(
        progress(&store, &account, first),
        Progress::Review(ReviewReason::CardBanned)
    );
    assert_eq!(
        progress(&store, &other, second),
        Progress::Review(ReviewReason::CardBanned)
    );
    assert_eq!(progress(&store, &account, third), WAITING);
    assert_eq!(entries(&store), [entry("grant", third, None)]);
    // Unbanned, the cards pay again; what waited goes on waiting.
    assert!(store.unban(&ban_digest("banned@qq.com")).unwrap());
    assert_eq!(rows(&store, "banned_cards"), 0);
    let fourth = place(&mut store, 3, &month(), 4);
    record_all(
        &mut store,
        &[
            event(7, 60, completed(4, fourth, &month())),
            event(8, 50, charged("pi_4", 7)),
        ],
    );
    assert_eq!(progress(&store, &other, fourth), WAITING);
    assert_eq!(
        progress(&store, &account, first),
        Progress::Review(ReviewReason::CardBanned)
    );
}

#[test]
fn were_a_payment_charged_twice_the_worse_verdict_would_stand() {
    for (verdicts, stands) in [
        (vec![CardUsed::Wallet], "unchecked"),
        (vec![CardUsed::Other, CardUsed::Wallet], "unchecked"),
        (vec![CardUsed::Card(fingerprint(1))], "clear"),
        (
            vec![
                CardUsed::Card(fingerprint(1)),
                CardUsed::Card(fingerprint(2)),
            ],
            "clear",
        ),
        (
            vec![CardUsed::Card(fingerprint(1)), CardUsed::Wallet],
            "unchecked",
        ),
        (
            vec![CardUsed::Wallet, CardUsed::Card(fingerprint(1))],
            "unchecked",
        ),
        (
            vec![
                CardUsed::Card(fingerprint(1)),
                CardUsed::Card(fingerprint(7)),
            ],
            "banned",
        ),
        (
            vec![
                CardUsed::Card(fingerprint(7)),
                CardUsed::Card(fingerprint(1)),
            ],
            "banned",
        ),
        (
            vec![CardUsed::Card(fingerprint(7)), CardUsed::Wallet],
            "banned",
        ),
        (
            vec![CardUsed::Wallet, CardUsed::Card(fingerprint(7))],
            "banned",
        ),
    ] {
        let mut store = shop();
        let banned = register(&mut store, "banned@qq.com", 1);
        store
            .ban(&banned.code, &[fingerprint(7).digest()], at(NOW))
            .unwrap();
        // A refund told of first leaves the verdict open.
        record_all(&mut store, &[event(0, 0, refunded("pi_1", 1, false))]);
        for (n, card) in verdicts.iter().enumerate() {
            let n = u32::try_from(n).unwrap() + 1;
            record_all(
                &mut store,
                &[event(n, 0, charged_otherwise("pi_1", card.clone()))],
            );
        }
        let verdict: String = store
            .conn
            .query_row("SELECT card FROM stripe_payments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(verdict, stands, "{verdicts:?}");
        assert_eq!(rows(&store, "stripe_payments"), 1);
    }
}

#[test]
fn a_payment_for_an_account_no_longer_bound_to_the_orders_user_waits() {
    for rebound in [None, Some("pilotuserb")] {
        let mut store = shop();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        let order = place(&mut store, 1, &month(), 1);
        record_all(&mut store, &[event(1, 60, completed(1, order, &month()))]);
        store.unbind(&account.code).unwrap();
        if let Some(other) = rebound {
            store.bind(&account.code, &user(other), at(NOW)).unwrap();
        }
        record_all(&mut store, &[event(2, 50, charged("pi_1", 1))]);
        assert_eq!(
            progress(&store, &account, order),
            Progress::Review(ReviewReason::Binding),
            "{rebound:?}"
        );
        assert_eq!(entries(&store), []);
    }
    // Bound to the same user again in time, the order goes through.
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    store.unbind(&account.code).unwrap();
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 50, charged("pi_1", 1)),
        ],
    );
    assert_eq!(progress(&store, &account, order), WAITING);
    // Once granted, it is Mooring's: unbinding the account does not send
    // it back for review, and what Stripe says of it later is entered
    // after its grant.
    store.unbind(&account.code).unwrap();
    record_all(&mut store, &[event(3, 700, refunded("pi_1", 100, false))]);
    assert_eq!(progress(&store, &account, order), WAITING);
    assert_eq!(store.orders_waiting().unwrap(), []);
    assert_eq!(
        entries(&store),
        [
            entry("grant", order, None),
            entry("refund", order, Some(100))
        ]
    );
}

#[test]
fn a_granted_orders_refunds_and_dispute_are_entered_after_it() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 50, charged("pi_1", 1)),
            event(3, 1_000, refunded("pi_1", 300, false)),
        ],
    );
    let grant = entry("grant", order, None);
    assert_eq!(
        entries(&store),
        [grant.clone(), entry("refund", order, Some(300))]
    );
    let refunded_some = order_of(&store, &account, order);
    assert_eq!(
        (
            refunded_some.refunded,
            refunded_some.refunded_in_full,
            refunded_some.disputed
        ),
        (300, false, false)
    );
    assert_eq!(refunded_some.progress, WAITING);
    // The same total told of again, or a smaller one that was made
    // earlier, adds nothing.
    record_all(
        &mut store,
        &[
            event(4, 1_001, refunded("pi_1", 300, false)),
            event(5, 900, refunded("pi_1", 100, false)),
        ],
    );
    assert_eq!(entries(&store).len(), 2);
    // More goes back: the new total. All of it: the same, and in full.
    record_all(
        &mut store,
        &[
            event(6, 2_000, refunded("pi_1", 500, false)),
            event(7, 3_000, refunded("pi_1", 800, true)),
            event(8, 3_001, refunded("pi_1", 800, true)),
        ],
    );
    assert_eq!(
        entries(&store),
        [
            grant.clone(),
            entry("refund", order, Some(300)),
            entry("refund", order, Some(500)),
            entry("refund", order, Some(800)),
        ]
    );
    let refunded_all = order_of(&store, &account, order);
    assert_eq!(
        (refunded_all.refunded, refunded_all.refunded_in_full),
        (800, true)
    );
    // Granted it stays: what Mooring does about the refund is Mooring's.
    assert_eq!(refunded_all.progress, WAITING);
    // A dispute is entered once.
    record_all(
        &mut store,
        &[
            event(9, 4_000, disputed("pi_1", 800)),
            event(10, 4_001, disputed("pi_1", 800)),
        ],
    );
    assert_eq!(entries(&store).len(), 5);
    assert_eq!(entries(&store)[4], entry("dispute", order, Some(800)));
    assert!(order_of(&store, &account, order).disputed);
    // The numbers only grow.
    let numbers: Vec<i64> = journal(&store).iter().map(|(seq, ..)| *seq).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5]);
}

#[test]
fn a_payment_returned_before_it_is_granted_is_never_granted() {
    for returned in [refunded("pi_1", 800, true), disputed("pi_1", 800)] {
        let mut store = shop();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        let order = place(&mut store, 1, &month(), 1);
        record_all(
            &mut store,
            &[
                event(1, 60, completed(1, order, &month())),
                event(2, 70, returned.clone()),
                event(3, 50, charged("pi_1", 1)),
            ],
        );
        assert_eq!(
            progress(&store, &account, order),
            Progress::Returned,
            "{returned:?}"
        );
        assert_eq!(entries(&store), []);
    }
    // A part refunded does not hold the grant back; it is entered with it.
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 70, refunded("pi_1", 799, false)),
            event(3, 50, charged("pi_1", 1)),
        ],
    );
    assert_eq!(progress(&store, &account, order), WAITING);
    assert_eq!(
        entries(&store),
        [
            entry("grant", order, None),
            entry("refund", order, Some(799))
        ]
    );
}

#[test]
fn the_outcome_is_the_same_in_whatever_order_the_events_arrive() {
    // One store for every ordering would need the same account in each:
    // registering is slow, so its digest is worked out once.
    let address = digested("a@qq.com");
    let events = |order| {
        [
            event(1, 60, completed(1, order, &month())),
            event(2, 50, charged("pi_1", 1)),
            event(3, 500, refunded("pi_1", 300, false)),
            event(4, 900, refunded("pi_1", 450, false)),
        ]
    };
    let mut seen = 0;
    for ordering in orderings(&[0_usize, 1, 2, 3]) {
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
        let order = place(&mut store, 1, &month(), 1);
        let events = events(order);
        for index in &ordering {
            assert_eq!(record(&mut store, &events[*index]), Recorded::New);
        }
        let settled = order_of(&store, &account, order);
        assert_eq!(settled.progress, WAITING, "{ordering:?}");
        assert_eq!((settled.refunded, settled.refunded_in_full), (450, false));
        let written = entries(&store);
        assert_eq!(written[0], entry("grant", order, None), "{ordering:?}");
        assert_eq!(
            written.last(),
            Some(&entry("refund", order, Some(450))),
            "{ordering:?}"
        );
        seen += 1;
    }
    assert_eq!(seen, 24);
}
