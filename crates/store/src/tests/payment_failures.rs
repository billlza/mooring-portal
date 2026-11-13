//! A payment operation that fails, fails whole and loudly.

use portal_core::payment::stripe::{BillingReason, CheckoutSession, EventKind};

use super::paying::*;
use super::*;
use crate::{
    ApproveError, DiscardError, Discarded, DismissError, ModeError, OrderError, Progress,
    PullError, Recorded, StoreError,
};

#[test]
fn an_amount_too_large_to_store_fails_the_event_whole() {
    let mut store = shop();
    let too_much = u64::MAX;
    for kind in [
        refunded("pi_1", too_much, false),
        disputed("pi_1", too_much),
    ] {
        let failed = store.record_stripe_event(&event(1, 0, kind), &on_sale(), at(NOW));
        assert!(
            matches!(failed, Err(StoreError::OutOfRange { .. })),
            "{failed:?}"
        );
    }
    // Nothing of it is kept, so Stripe's next delivery is read afresh.
    assert_eq!(rows(&store, "stripe_events"), 0);
    assert_eq!(rows(&store, "stripe_payments"), 0);
    let stray = CheckoutSession {
        amount_total: Some(too_much),
        ..checkout(1, oid(1), &month())
    };
    assert!(matches!(
        store.record_stripe_event(
            &event(1, 0, EventKind::CheckoutCompleted(stray)),
            &on_sale(),
            at(NOW)
        ),
        Err(StoreError::OutOfRange {
            what: "paid amount"
        })
    ));
    assert_eq!(rows(&store, "stripe_events"), 0);
}

#[test]
fn an_order_that_no_longer_parses_fails_its_checkout_whole() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    store
        .conn
        .execute("UPDATE orders SET currency = 'jpy'", [])
        .unwrap();
    let checkout = event(1, 60, completed(1, order, &month()));
    assert!(matches!(
        store.record_stripe_event(&checkout, &on_sale(), at(NOW)),
        Err(StoreError::Corrupt {
            what: "order's currency",
            ..
        })
    ));
    assert_eq!(rows(&store, "stripe_events"), 0);
    store
        .conn
        .execute("UPDATE orders SET currency = 'gbp'", [])
        .unwrap();
    assert_eq!(record(&mut store, &checkout), Recorded::New);
    assert_eq!(progress(&store, &account, order), Progress::Checking);
}

/// A store whose payment tables are gone after a customer ordered.
fn broken() -> (Store, Account) {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    store
        .conn
        .execute_batch(
            "DROP TABLE entitlements; DROP TABLE stray_payments; DROP TABLE stripe_payments; \
             DROP TABLE stripe_invoice_payments; DROP TABLE stripe_subscriptions; \
             DROP TABLE stripe_events; DROP TABLE pull_access; DROP TABLE banned_cards; \
             DROP TABLE orders;",
        )
        .unwrap();
    (store, account)
}

fn sqlite<T: std::fmt::Debug>(result: Result<T, StoreError>) {
    assert!(matches!(result, Err(StoreError::Sqlite(_))), "{result:?}");
}

#[test]
fn payment_reads_fail_loudly() {
    let (store, account) = broken();
    sqlite(store.orders_of(account.id, 10));
    sqlite(store.order_of(account.id, &oid(1)));
    sqlite(store.renewals_of(account.id));
    sqlite(store.orders_waiting());
    sqlite(store.recent_orders(10));
    sqlite(store.stray_payments());
    sqlite(store.open_renewals());
    sqlite(store.unmatched_payments());
    sqlite(store.pull_state());
    sqlite(store.is_pull_token(&digest(77)));
}

#[test]
fn payment_writes_fail_loudly() {
    let (mut store, _) = broken();
    assert!(matches!(
        store.place_order(&digest(1), &month(), oid(1), at(NOW)),
        Err(OrderError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.place_order(&digest(1), &auto(), oid(1), at(NOW)),
        Err(OrderError::Store(StoreError::Sqlite(_)))
    ));
    for kind in [
        EventKind::Unused,
        completed(1, oid(1), &month()),
        charged("pi_1", 1),
    ] {
        sqlite(store.record_stripe_event(&event(1, 0, kind), &on_sale(), at(NOW)));
    }
    assert!(matches!(
        store.approve_order(&oid(1), at(NOW)),
        Err(ApproveError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.dismiss_stray(1, None),
        Err(DismissError::Store(StoreError::Sqlite(_)))
    ));
    sqlite(store.payments_outstanding());
    assert!(matches!(
        store.enter_payment_mode(true),
        Err(ModeError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.discard_test_payments(),
        Err(DiscardError::Store(StoreError::Sqlite(_)))
    ));
    sqlite(store.set_pull_token(&digest(77), at(NOW)));
    sqlite(store.revoke_pull_token());
    assert!(matches!(
        store.pull_entitlements(&digest(77), 0, 10, at(NOW)),
        Err(PullError::Store(StoreError::Sqlite(_)))
    ));
    // The address is banned, but there is nowhere to keep its cards.
    let banned = ban_digest("a@qq.com");
    store
        .conn
        .execute(
            "INSERT INTO banned_emails (email_digest, banned_at) VALUES (?1, 0)",
            [banned.as_bytes()],
        )
        .unwrap();
    sqlite(store.ban_cards(&banned, &[fingerprint(1).digest()]));
}

#[test]
fn an_event_that_fails_part_way_is_not_recorded_at_all() {
    // With the journal made to refuse the grant, the event that would
    // have led to it leaves nothing: neither its id nor the card's
    // verdict, so that Stripe's next delivery of it is read afresh.
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(&mut store, &[event(1, 60, completed(1, order, &month()))]);
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON entitlements \
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let charge = event(2, 61, charged("pi_1", 1));
    sqlite(store.record_stripe_event(&charge, &on_sale(), at(NOW + 62)));
    assert_eq!(rows(&store, "stripe_events"), 1);
    assert_eq!(rows(&store, "stripe_payments"), 0);
    assert_eq!(progress(&store, &account, order), Progress::Checking);
    // So does an approval that would enter it.
    store
        .conn
        .execute(
            "UPDATE orders SET state = 'review', review_reason = 'card_unchecked'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.approve_order(&order, at(NOW)),
        Err(ApproveError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        progress(&store, &account, order),
        Progress::Review(_)
    ));
    store.conn.execute_batch("DROP TRIGGER refuse;").unwrap();
    store
        .conn
        .execute("UPDATE orders SET state = 'paid', review_reason = NULL", [])
        .unwrap();
    assert_eq!(record(&mut store, &charge), Recorded::New);
    assert_eq!(entries(&store), [entry("grant", order, None)]);
}

#[test]
fn a_rehearsal_that_cannot_be_discarded_whole_is_kept_whole() {
    let mut store = shop();
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 61, invoiced("in_9", "sub_7", BillingReason::Other, 50)),
        ],
    );
    let held = |store: &Store| {
        ["orders", "stray_payments", "stripe_events"].map(|table| rows(store, table))
    };
    assert_eq!(held(&store), [1, 1, 2]);
    // The orders go first; what follows them fails.
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER keep BEFORE DELETE ON stray_payments \
             BEGIN SELECT RAISE(ABORT, 'kept'); END;",
        )
        .unwrap();
    let failed = store.discard_test_payments();
    assert!(
        matches!(failed, Err(DiscardError::Store(StoreError::Sqlite(_)))),
        "{failed:?}"
    );
    assert_eq!(held(&store), [1, 1, 2]);
    // Nor does the database change over to real money with half of what
    // Stripe said forgotten.
    store
        .conn
        .execute_batch(
            "DROP TRIGGER keep;
             DELETE FROM orders;
             DELETE FROM stray_payments;
             CREATE TRIGGER keep BEFORE DELETE ON stripe_payments \
             BEGIN SELECT RAISE(ABORT, 'kept'); END;
             INSERT INTO stripe_payments (payment_intent, noticed_at) VALUES ('pi_1', 0);",
        )
        .unwrap();
    let failed = store.enter_payment_mode(true);
    assert!(
        matches!(failed, Err(ModeError::Store(StoreError::Sqlite(_)))),
        "{failed:?}"
    );
    assert_eq!(rows(&store, "stripe_events"), 2);
    assert_eq!(crate::payment_mode::mode(&store.conn).unwrap(), Some(false));
    // With nothing in the way, both go through.
    store.conn.execute_batch("DROP TRIGGER keep;").unwrap();
    assert_eq!(
        store.discard_test_payments().unwrap(),
        Discarded {
            orders: 0,
            stray_payments: 0
        }
    );
    assert_eq!(rows(&store, "stripe_events"), 0);
    store.enter_payment_mode(true).unwrap();
}
