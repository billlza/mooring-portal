//! One database, one kind of money: the mode it has entered, the events
//! it refuses, a rehearsal discarded, and money noticed in the air.

use portal_core::payment::stripe::{BillingReason, CheckoutSession, Event, EventKind, Renewal};

use super::paying::*;
use super::*;
use crate::{DiscardError, Discarded, ModeError, Progress, PullState, Recorded};

/// `event` as Stripe sends it for a payment made with real money.
fn live(event: Event) -> Event {
    Event {
        livemode: true,
        ..event
    }
}

/// A checkout that names no order: a payment for the operator to look at.
fn astray(n: u8) -> EventKind {
    EventKind::CheckoutCompleted(CheckoutSession {
        order: None,
        ..checkout(n, oid(31), &month())
    })
}

#[test]
fn a_database_enters_a_mode_and_changes_it_only_while_it_holds_no_payment() {
    let mut store = store();
    // Entered, and entered again: nothing to it.
    store.enter_payment_mode(false).unwrap();
    store.enter_payment_mode(false).unwrap();
    assert_eq!(rows(&store, "payment_mode"), 1);
    // Holding nothing, it changes over and back; what Stripe said in the
    // mode it leaves is forgotten, for none of it can be of the other.
    record_all(
        &mut store,
        &[
            event(1, 0, refunded("pi_9", 5, false)),
            event(2, 1, invoice_paid_by("in_9", "pi_9")),
            event(3, 2, subscription("sub_9", Renewal::Active)),
        ],
    );
    let said = |store: &Store| {
        [
            "stripe_events",
            "stripe_payments",
            "stripe_invoice_payments",
            "stripe_subscriptions",
        ]
        .map(|table| rows(store, table))
    };
    assert_eq!(said(&store), [3, 1, 1, 1]);
    store.enter_payment_mode(true).unwrap();
    assert_eq!(said(&store), [0, 0, 0, 0]);
    assert_eq!(
        record(&mut store, &live(event(1, 0, EventKind::Unused))),
        Recorded::New
    );
    store.enter_payment_mode(true).unwrap();
    assert_eq!(said(&store), [1, 0, 0, 0], "the same mode forgets nothing");
    store.enter_payment_mode(false).unwrap();
    assert_eq!(rows(&store, "payment_mode"), 1);

    // An order, even unpaid, keeps it in its mode.
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    place(&mut store, 1, &month(), 1);
    let refused = store.enter_payment_mode(true);
    assert!(
        matches!(refused, Err(ModeError::Held { livemode: false })),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the database holds orders and payments of Stripe's test mode, and the catalogue is \
         for real payments: remove them with `discard-test-payments` first"
    );
    // It is still in the mode it was in.
    assert_eq!(
        record(&mut store, &event(5, 0, EventKind::Unused)),
        Recorded::New
    );
    store.enter_payment_mode(false).unwrap();
}

#[test]
fn a_payment_that_is_no_orders_keeps_a_database_in_its_mode_too() {
    let mut store = store();
    store.enter_payment_mode(true).unwrap();
    record_all(&mut store, &[live(event(1, 0, astray(1)))]);
    assert_eq!(rows(&store, "orders"), 0);
    let refused = store.enter_payment_mode(false);
    assert!(
        matches!(refused, Err(ModeError::Held { livemode: true })),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the database holds orders and payments made with real money, and the catalogue is \
         for Stripe's test mode: rehearse with another database"
    );
    // Dismissed, nothing holds it.
    store.dismiss_stray(1, None).unwrap();
    store.enter_payment_mode(false).unwrap();
}

#[test]
fn an_event_of_the_other_mode_is_not_recorded_at_all() {
    // A rehearsal's database is sent a real payment's events, and a
    // database of real payments a rehearsal's.
    for database_is_live in [false, true] {
        let mut store = store();
        store.enter_payment_mode(database_is_live).unwrap();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        let order = place(&mut store, 1, &month(), 1);
        let of_mode = |event: Event, livemode: bool| Event { livemode, ..event };
        let checkout = event(1, 60, completed(1, order, &month()));
        let charge = event(2, 61, charged("pi_1", 1));
        for other in [&checkout, &charge] {
            assert_eq!(
                record(&mut store, &of_mode(other.clone(), !database_is_live)),
                Recorded::OtherMode
            );
        }
        assert_eq!(rows(&store, "stripe_events"), 0);
        assert_eq!(rows(&store, "stripe_payments"), 0);
        assert_eq!(progress(&store, &account, order), Progress::Pending);
        // Nothing of them was kept, not even their ids: the same events
        // in the database's own mode are new.
        for own in [checkout, charge] {
            assert_eq!(
                record(&mut store, &of_mode(own, database_is_live)),
                Recorded::New
            );
        }
        assert_eq!(
            progress(&store, &account, order),
            Progress::Granted { pulled: false }
        );
    }
    // A database that has entered no mode takes no event of either.
    let mut unsold = store();
    for event in [
        event(1, 0, EventKind::Unused),
        live(event(2, 0, EventKind::Unused)),
    ] {
        assert_eq!(record(&mut unsold, &event), Recorded::OtherMode);
    }
    assert_eq!(rows(&unsold, "stripe_events"), 0);
}

/// A rehearsal: two customers' orders (one granted with a refund, one
/// renewing with a renewal, one unpaid), a payment that is no order's,
/// and a refund nothing names; Mooring has pulled the first entry.
fn rehearsal() -> (Store, Account) {
    let mut store = shop();
    let first = customer(&mut store, "a@qq.com", 1, "pilotusera");
    customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let once = place(&mut store, 1, &month(), 1);
    let renewing = place(&mut store, 2, &auto(), 2);
    place(&mut store, 1, &pack(), 3);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, once, &month())),
            event(2, 61, charged("pi_1", 1)),
            event(3, 62, refunded("pi_1", 100, false)),
            event(4, 70, completed(2, renewing, &auto())),
            event(5, 71, invoice_paid_by("in_2", "pi_2")),
            event(6, 72, charged("pi_2", 2)),
            event(7, 900, invoiced("in_3", "sub_2", BillingReason::Cycle, 750)),
            event(8, 901, astray(8)),
            event(9, 902, refunded("pi_9", 5, false)),
        ],
    );
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    store
        .pull_entitlements(&digest(77), 1, 10, at(NOW + 1_000))
        .unwrap();
    (store, first)
}

/// How many rows each table of payments holds.
fn held(store: &Store) -> [i64; 7] {
    [
        "orders",
        "entitlements",
        "stray_payments",
        "stripe_events",
        "stripe_payments",
        "stripe_invoice_payments",
        "stripe_subscriptions",
    ]
    .map(|table| rows(store, table))
}

#[test]
fn a_rehearsal_is_discarded_whole() {
    let (mut store, first) = rehearsal();
    ban_card(&mut store, 7);
    assert_eq!(held(&store), [4, 3, 1, 9, 3, 1, 0]);
    let pulled = PullState {
        token_created_at: Some(at(NOW)),
        pulled_through: 1,
        pulled_at: Some(at(NOW + 1_000)),
        last_seq: 3,
    };
    assert_eq!(store.pull_state().unwrap(), pulled);
    assert_eq!(
        store.discard_test_payments().unwrap(),
        Discarded {
            orders: 4,
            stray_payments: 1,
        }
    );
    assert_eq!(held(&store), [0; 7]);
    // What is not the rehearsal's stays: the accounts, the ban list, the
    // token Mooring pulls with and what it has pulled. The journal's
    // numbers are not given out again.
    assert_eq!(rows(&store, "accounts"), 2);
    assert_eq!(rows(&store, "banned_cards"), 1);
    assert_eq!(store.pull_state().unwrap(), pulled);
    // Discarded again, there is nothing to discard.
    assert_eq!(
        store.discard_test_payments().unwrap(),
        Discarded {
            orders: 0,
            stray_payments: 0,
        }
    );
    // Now it can take real payments, whose entries follow the
    // rehearsal's numbers; and the events of the rehearsal, sent again,
    // are refused.
    store.enter_payment_mode(true).unwrap();
    let order = place(&mut store, 1, &month(), 4);
    assert_eq!(
        record(&mut store, &event(1, 60, completed(1, order, &month()))),
        Recorded::OtherMode
    );
    record_all(
        &mut store,
        &[
            live(event(11, 2_000, completed(4, order, &month()))),
            live(event(12, 2_001, charged("pi_4", 1))),
        ],
    );
    assert_eq!(
        progress(&store, &first, order),
        Progress::Granted { pulled: false }
    );
    let page = store
        .pull_entitlements(&digest(77), 3, 10, at(NOW + 3_000))
        .unwrap();
    assert!(page.livemode);
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.seq)
            .collect::<Vec<_>>(),
        [4]
    );
}

#[test]
fn real_payments_are_never_discarded() {
    let mut store = store();
    store.enter_payment_mode(true).unwrap();
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            live(event(1, 60, completed(1, order, &month()))),
            live(event(2, 61, charged("pi_1", 1))),
            live(event(3, 62, astray(3))),
        ],
    );
    let before = held(&store);
    assert_eq!(before, [1, 1, 1, 3, 1, 0, 0]);
    let refused = store.discard_test_payments();
    assert!(matches!(refused, Err(DiscardError::Live)), "{refused:?}");
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the database is for payments made with real money, which are never discarded; nothing \
         changed"
    );
    assert_eq!(held(&store), before);
    // A database that has entered no mode has taken no payment at all:
    // its unpaid orders may go.
    let mut unsold = super::store();
    customer(&mut unsold, "a@qq.com", 1, "pilotusera");
    place(&mut unsold, 1, &month(), 1);
    assert_eq!(
        unsold.discard_test_payments().unwrap(),
        Discarded {
            orders: 1,
            stray_payments: 0,
        }
    );
}

#[test]
fn money_in_the_air_is_noticed() {
    let mut store = shop();
    assert!(!store.payments_outstanding().unwrap());
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    // An unpaid order is no money at all.
    let order = place(&mut store, 1, &month(), 1);
    assert!(!store.payments_outstanding().unwrap());
    // Paid: in the air until Mooring has it.
    record_all(&mut store, &[event(1, 60, completed(1, order, &month()))]);
    assert!(store.payments_outstanding().unwrap());
    record_all(&mut store, &[event(2, 61, charged("pi_1", 1))]);
    assert!(store.payments_outstanding().unwrap());
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    store
        .pull_entitlements(&digest(77), 1, 10, at(NOW))
        .unwrap();
    assert!(!store.payments_outstanding().unwrap());
    // A payment that is no order's is the operator's to settle at Stripe,
    // with or without the server.
    record_all(&mut store, &[event(3, 70, astray(3))]);
    assert!(!store.payments_outstanding().unwrap());
    // A subscription Stripe may charge again, though its order is with
    // Mooring; until Stripe says it has ended.
    let renewing = place(&mut store, 1, &auto(), 2);
    record_all(
        &mut store,
        &[
            event(4, 80, completed(2, renewing, &auto())),
            event(5, 81, invoice_paid_by("in_2", "pi_2")),
            event(6, 82, charged("pi_2", 1)),
        ],
    );
    store
        .pull_entitlements(&digest(77), 2, 10, at(NOW))
        .unwrap();
    assert!(store.payments_outstanding().unwrap());
    record_all(
        &mut store,
        &[event(7, 90, subscription("sub_2", Renewal::Ended))],
    );
    assert!(!store.payments_outstanding().unwrap());
}
