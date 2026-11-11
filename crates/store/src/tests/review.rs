//! What the operator sees of payments: the lists of what waits.

use portal_core::payment::stripe::{CheckoutSession, EventKind, Renewal};
use portal_core::payment::{CardVerdict, OrderId};

use super::paying::*;
use super::*;
use crate::{OpenRenewal, OperatorOrder, StoreError};

#[test]
fn the_operator_sees_each_order_with_its_account_and_stripes_ids() {
    let mut store = shop();
    let first = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let second = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let pending = store
        .place_order(&digest(1), &pack(), oid(1), at(NOW + 10))
        .unwrap();
    let auto_order = store
        .place_order(&digest(2), &auto(), oid(2), at(NOW + 20))
        .unwrap();
    let month_order = store
        .place_order(&digest(1), &month(), oid(3), at(NOW + 30))
        .unwrap();
    record_all(
        &mut store,
        &[
            event(1, 60, completed(2, auto_order, &auto())),
            event(2, 61, invoice_paid_by("in_2", "pi_2")),
            event(3, 70, completed(3, month_order, &month())),
            event(4, 71, charged("pi_3", 1)),
        ],
    );
    let recent = store.recent_orders(10).unwrap();
    assert_eq!(
        recent
            .iter()
            .map(|listed| listed.order.id)
            .collect::<Vec<_>>(),
        [month_order, auto_order, pending]
    );
    assert_eq!(
        recent[0],
        OperatorOrder {
            order: order_of(&store, &first, month_order),
            account: first.code,
            card: Some(CardVerdict::Clear),
            session: Some(sid("cs_3")),
            payment_intent: Some(sid("pi_3")),
            invoice: None,
            subscription: None,
        }
    );
    assert_eq!(
        recent[1],
        OperatorOrder {
            order: order_of(&store, &second, auto_order),
            account: second.code,
            card: None,
            session: Some(sid("cs_2")),
            payment_intent: Some(sid("pi_2")),
            invoice: Some(sid("in_2")),
            subscription: Some(sid("sub_2")),
        }
    );
    assert_eq!(
        (&recent[2].session, &recent[2].payment_intent),
        (&None, &None)
    );
    assert_eq!(store.recent_orders(1).unwrap(), recent[..1]);
    assert_eq!(store.recent_orders(0).unwrap(), []);
    // What waits: the order whose card is not yet checked. An unpaid
    // order waits for its payer, a granted one for no one.
    let waiting = store.orders_waiting().unwrap();
    assert_eq!(waiting, recent[1..2]);
    assert_eq!(
        store.open_renewals().unwrap(),
        [OpenRenewal {
            subscription: sid("sub_2"),
            account: second.code,
            plan: auto().plan,
            state: Renewal::Active,
        }]
    );
}

#[test]
fn what_waits_is_listed_oldest_first_until_it_is_settled() {
    let mut store = shop();
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let waiting = |store: &Store| -> Vec<OrderId> {
        store
            .orders_waiting()
            .unwrap()
            .iter()
            .map(|listed| listed.order.id)
            .collect()
    };
    let checking = store
        .place_order(&digest(2), &pack(), oid(9), at(NOW + 5))
        .unwrap();
    record_all(&mut store, &[event(1, 60, completed(9, checking, &pack()))]);
    let review = short_paid(&mut store, 1, 1);
    assert_eq!(waiting(&store), [review, checking]);
    // Refunded in part, the one in review still waits; in full, it does
    // not. The other's card passes.
    record_all(&mut store, &[event(2, 100, refunded("pi_1", 1, false))]);
    assert_eq!(waiting(&store), [review, checking]);
    record_all(
        &mut store,
        &[
            event(3, 101, refunded("pi_1", 1, true)),
            event(4, 102, charged("pi_9", 1)),
        ],
    );
    assert_eq!(waiting(&store), []);
}

#[test]
fn the_operators_lists_report_rows_that_no_longer_parse() {
    type Read = fn(&Store) -> Result<(), StoreError>;
    let cases: [(&str, &str, Read); 10] = [
        (
            "UPDATE stripe_payments SET card = 'maybe' WHERE payment_intent = 'pi_1'",
            "card verdict",
            |store| store.recent_orders(10).map(|_| ()),
        ),
        (
            "UPDATE stripe_payments SET card = 'maybe' WHERE payment_intent = 'pi_2'",
            "card verdict",
            |store| store.stray_payments().map(|_| ()),
        ),
        (
            "UPDATE orders SET session = 'no id'",
            "Stripe id",
            |store| store.recent_orders(10).map(|_| ()),
        ),
        (
            "UPDATE orders SET plan = 'No Plan'",
            "order's plan",
            |store| store.open_renewals().map(|_| ()),
        ),
        (
            "UPDATE stripe_subscriptions SET state = 'paused'",
            "subscription state",
            |store| store.open_renewals().map(|_| ()),
        ),
        (
            "UPDATE stray_payments SET reason = 'whim'",
            "stray payment's reason",
            |store| store.stray_payments().map(|_| ()),
        ),
        (
            "UPDATE stray_payments SET amount = -1",
            "stray payment's amount",
            |store| store.stray_payments().map(|_| ()),
        ),
        (
            "UPDATE stray_payments SET currency = 'jpy'",
            "stray payment's currency",
            |store| store.stray_payments().map(|_| ()),
        ),
        (
            "UPDATE stray_payments SET noticed_at = -1",
            "stray payment's time",
            |store| store.stray_payments().map(|_| ()),
        ),
        (
            "UPDATE stripe_payments SET disputed_amount = -1",
            "disputed amount",
            |store| store.unmatched_payments().map(|_| ()),
        ),
    ];
    for (change, what, read) in cases {
        let mut store = shop();
        customer(&mut store, "a@qq.com", 1, "pilotusera");
        let order = place(&mut store, 1, &auto(), 1);
        let stray = CheckoutSession {
            order: None,
            ..checkout(2, order, &month())
        };
        record_all(
            &mut store,
            &[
                event(1, 60, completed(1, order, &auto())),
                event(2, 61, subscription("sub_1", Renewal::Ending)),
                event(3, 62, EventKind::CheckoutCompleted(stray)),
                event(4, 63, refunded("pi_8", 5, false)),
                event(5, 64, invoice_paid_by("in_1", "pi_1")),
                event(6, 65, charged("pi_1", 1)),
                event(7, 66, charged("pi_2", 1)),
            ],
        );
        // The schema's own checks are lifted for the test's sake.
        store
            .conn
            .execute_batch(&format!("PRAGMA ignore_check_constraints = ON; {change};"))
            .unwrap();
        let outcome = read(&store);
        assert!(
            matches!(&outcome, Err(StoreError::Corrupt { what: found, .. }) if *found == what),
            "{change}: {outcome:?}"
        );
    }
}
