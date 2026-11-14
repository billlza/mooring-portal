//! What every page shows an account besides its content: for an
//! operator, how much waits for it.

use portal_core::feedback::MessageBody;
use portal_core::payment::stripe::{CheckoutSession, EventKind};

use super::paying::{
    charged, checkout, completed, customer, disputed, event, month, oid, place, record_all,
    refunded, shop, short_paid,
};
use super::*;
use crate::Signals;

#[test]
fn an_operator_is_told_how_many_payments_wait_and_no_one_else_is() {
    let mut store = shop();
    let operator = register(&mut store, "op@qq.com", 50);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let waiting = |store: &Store| store.signals(operator.id).unwrap().payments_waiting;
    // Placed and not paid, an order waits for no one.
    let paid = place(&mut store, 1, &month(), 1);
    assert_eq!(waiting(&store), 0);
    // Paid, it waits for word of its card; paid wrongly, for the operator.
    record_all(&mut store, &[event(1, 60, completed(1, paid, &month()))]);
    assert_eq!(waiting(&store), 1);
    short_paid(&mut store, 1, 2);
    assert_eq!(waiting(&store), 2);
    // A payment that is no order's.
    let stray = CheckoutSession {
        order: None,
        ..checkout(3, oid(9), &month())
    };
    record_all(
        &mut store,
        &[event(3, 62, EventKind::CheckoutCompleted(stray))],
    );
    assert_eq!(waiting(&store), 3);
    // Granted, an order waits no more, nor one refunded in full.
    record_all(
        &mut store,
        &[
            event(4, 63, charged("pi_1", 1)),
            event(5, 64, refunded("pi_2", 1, true)),
        ],
    );
    assert_eq!(waiting(&store), 1);
    // Disputed before it was granted, an order is listed still, but waits
    // for nothing the portal follows.
    let disputed_order = short_paid(&mut store, 1, 6);
    assert_eq!(waiting(&store), 2);
    record_all(&mut store, &[event(6, 66, disputed("pi_6", 1))]);
    assert_eq!(waiting(&store), 1);
    assert!(
        store
            .orders_waiting()
            .unwrap()
            .iter()
            .any(|listed| listed.order.id == disputed_order && listed.order.disputed)
    );
    // Everything that waits: the payment and a conversation.
    let body = MessageBody::parse("付了款还没开通").unwrap();
    store
        .send_feedback(&digest(1), &body, at(NOW + 65))
        .unwrap();
    assert_eq!(
        store.signals(operator.id).unwrap(),
        Signals {
            operator: true,
            unread_reply: false,
            awaiting_reply: 1,
            payments_waiting: 1,
        }
    );
    assert_eq!(store.signals(operator.id).unwrap().waiting(), 2);
    // Only an operator is told what waits.
    assert_eq!(store.signals(account.id).unwrap(), Signals::default());
}
