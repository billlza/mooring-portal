//! Placing orders and reading them back.

use portal_core::payment::stripe::{CheckoutSession, EventKind, Renewal};
use portal_core::payment::{OrderTerms, ReviewReason};

use super::paying::*;
use super::*;
use crate::{AutoRenewal, Order, OrderError, Progress, Recorded, StoreError};

#[test]
fn an_order_is_placed_for_the_sessions_account_and_its_mooring_user() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let placed = store
        .place_order(&digest(1), &month(), oid(1), at(NOW + 60))
        .unwrap();
    assert_eq!(placed, oid(1));
    assert_eq!(
        store.orders_of(account.id, 10).unwrap(),
        [Order {
            id: oid(1),
            user: user("pilotusera"),
            terms: month(),
            created_at: at(NOW + 60),
            paid_at: None,
            progress: Progress::Pending,
            refunded: 0,
            refunded_in_full: false,
            disputed: false,
            renewal_of: None,
        }]
    );
    assert_eq!(
        store.order_of(account.id, &oid(1)).unwrap(),
        store.orders_of(account.id, 10).unwrap().pop()
    );
}

#[test]
fn an_order_needs_a_live_session_and_a_bound_account() {
    let mut store = shop();
    let account = register(&mut store, "a@qq.com", 1);
    // No session with the digest.
    assert!(matches!(
        store.place_order(&digest(1), &month(), oid(1), at(NOW)),
        Err(OrderError::SignedOut)
    ));
    store
        .start_session(&session(&account, 1, NOW, 100))
        .unwrap();
    // A session, but no binding.
    assert!(matches!(
        store.place_order(&digest(1), &month(), oid(1), at(NOW)),
        Err(OrderError::Unbound)
    ));
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    // The session has expired.
    assert!(matches!(
        store.place_order(&digest(1), &month(), oid(1), at(NOW + 100)),
        Err(OrderError::SignedOut)
    ));
    assert_eq!(rows(&store, "orders"), 0);
    store
        .place_order(&digest(1), &month(), oid(1), at(NOW + 99))
        .unwrap();
    assert_eq!(rows(&store, "orders"), 1);
    assert_eq!(
        OrderError::Unbound.to_string(),
        "the account is not bound to a Mooring user"
    );
    assert_eq!(OrderError::SignedOut.to_string(), "the session has ended");
}

#[test]
fn asking_again_gives_the_same_unpaid_order_while_its_terms_stand() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    assert_eq!(place(&mut store, 1, &month(), 1), oid(1));
    // The same plan on the same terms: the order placed before.
    assert_eq!(place(&mut store, 1, &month(), 2), oid(1));
    // Another plan: another order.
    assert_eq!(place(&mut store, 1, &pack(), 3), oid(3));
    assert_eq!(place(&mut store, 1, &pack(), 4), oid(3));
    let ids = |store: &Store| -> Vec<String> {
        let mut ids: Vec<String> = store
            .orders_of(account.id, 10)
            .unwrap()
            .iter()
            .map(|order| order.id.to_string())
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(&store), [oid(1).to_string(), oid(3).to_string()]);
    // Each of the terms, changed, makes a new order in the old one's
    // place: nobody is sent to pay for what the catalogue no longer has.
    let mut standing = month();
    let changes: [fn(&mut OrderTerms); 6] = [
        |terms| terms.amount = 900,
        |terms| terms.days = 31,
        |terms| terms.traffic_bytes += 1,
        |terms| terms.currency = portal_core::payment::Currency::parse("usd").unwrap(),
        |terms| terms.payment_link_id = "plink_1Other".to_owned(),
        |terms| terms.kind = portal_core::payment::PlanKind::Traffic,
    ];
    for (fresh, change) in (5..).zip(changes) {
        change(&mut standing);
        assert_eq!(
            place(&mut store, 1, &standing, fresh),
            oid(fresh),
            "{standing:?}"
        );
        assert_eq!(
            ids(&store),
            [oid(3).to_string(), oid(fresh).to_string()],
            "{standing:?}"
        );
        assert_eq!(order_of(&store, &account, oid(fresh)).terms, standing);
    }
}

#[test]
fn an_unpaid_order_for_another_mooring_user_is_replaced() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    place(&mut store, 1, &month(), 1);
    store.unbind(&account.code).unwrap();
    store
        .bind(&account.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    assert_eq!(place(&mut store, 1, &month(), 2), oid(2));
    let orders = store.orders_of(account.id, 10).unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].user, user("pilotuserb"));
}

#[test]
fn a_paid_order_is_never_given_again_nor_replaced() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let first = place(&mut store, 1, &month(), 1);
    record_all(&mut store, &[event(1, 0, completed(1, first, &month()))]);
    // The same plan again: a new order beside the paid one.
    assert_eq!(place(&mut store, 1, &month(), 2), oid(2));
    assert_eq!(place(&mut store, 1, &month(), 3), oid(2));
    assert_eq!(progress(&store, &account, first), Progress::Checking);
    assert_eq!(progress(&store, &account, oid(2)), Progress::Pending);
    assert_eq!(rows(&store, "orders"), 2);
}

#[test]
fn each_account_has_its_own_unpaid_orders_and_reads_only_its_own() {
    let mut store = shop();
    let first = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let second = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    assert_eq!(place(&mut store, 1, &month(), 1), oid(1));
    assert_eq!(place(&mut store, 2, &month(), 2), oid(2));
    assert_eq!(store.orders_of(first.id, 10).unwrap().len(), 1);
    assert_eq!(order_of(&store, &second, oid(2)).user, user("pilotuserb"));
    // Another account's order is not there to be read.
    assert_eq!(store.order_of(first.id, &oid(2)).unwrap(), None);
    assert_eq!(store.order_of(second.id, &oid(1)).unwrap(), None);
    assert_eq!(store.order_of(first.id, &oid(9)).unwrap(), None);
}

#[test]
fn orders_are_listed_newest_first_and_no_more_than_asked_for() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    for (fresh, terms, after) in [(1, month(), 10), (2, pack(), 30), (3, auto(), 20)] {
        store
            .place_order(&digest(1), &terms, oid(fresh), at(NOW + after))
            .unwrap();
    }
    let listed = |limit| -> Vec<String> {
        store
            .orders_of(account.id, limit)
            .unwrap()
            .iter()
            .map(|order| order.id.to_string())
            .collect()
    };
    assert_eq!(
        listed(10),
        [oid(2).to_string(), oid(3).to_string(), oid(1).to_string()]
    );
    assert_eq!(listed(2), [oid(2).to_string(), oid(3).to_string()]);
    assert_eq!(listed(0), Vec::<String>::new());
}

#[test]
fn a_renewing_plan_is_not_ordered_twice_while_stripe_may_charge_for_it() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let other = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let first = place(&mut store, 1, &auto(), 1);
    // Unpaid, it is the same order again.
    assert_eq!(place(&mut store, 1, &auto(), 2), first);
    assert_eq!(store.renewals_of(account.id).unwrap(), []);
    record_all(&mut store, &[event(1, 0, completed(1, first, &auto()))]);
    let renewing = |state| {
        vec![AutoRenewal {
            plan: auto().plan,
            state,
        }]
    };
    // Stripe has said nothing of the subscription: it is running.
    assert_eq!(
        store.renewals_of(account.id).unwrap(),
        renewing(Renewal::Active)
    );
    assert!(matches!(
        store.place_order(&digest(1), &auto(), oid(3), at(NOW)),
        Err(OrderError::Renewing)
    ));
    // A plan that does not renew may be bought beside it, and another
    // account may order the renewing plan.
    assert_eq!(place(&mut store, 1, &month(), 4), oid(4));
    assert_eq!(place(&mut store, 2, &auto(), 5), oid(5));
    assert_eq!(store.renewals_of(other.id).unwrap(), []);
    // Cancelled at the period's end, Stripe may still charge again if the
    // payer changes their mind.
    record_all(
        &mut store,
        &[event(2, 10, subscription("sub_1", Renewal::Ending))],
    );
    assert_eq!(
        store.renewals_of(account.id).unwrap(),
        renewing(Renewal::Ending)
    );
    assert!(matches!(
        store.place_order(&digest(1), &auto(), oid(3), at(NOW)),
        Err(OrderError::Renewing)
    ));
    // Over, the plan can be ordered anew.
    record_all(
        &mut store,
        &[event(3, 20, subscription("sub_1", Renewal::Ended))],
    );
    assert_eq!(store.renewals_of(account.id).unwrap(), []);
    assert_eq!(place(&mut store, 1, &auto(), 6), oid(6));
    assert_eq!(
        OrderError::Renewing.to_string(),
        "the account already has a plan that renews by itself"
    );
}

#[test]
fn an_order_that_no_longer_parses_is_reported_as_corrupt() {
    let cases: [(&str, &str); 9] = [
        ("mooring_user = 'nope'", "Mooring user id"),
        ("plan = 'No Plan'", "order's plan"),
        ("plan_kind = 'yearly'", "order's plan kind"),
        ("days = -1", "order's days"),
        ("traffic_bytes = -1", "order's traffic"),
        ("amount = -1", "order's amount"),
        ("currency = 'jpy'", "order's currency"),
        ("created_at = -1", "order's time"),
        ("renewal_of = 'short'", "order id"),
    ];
    for (change, what) in cases {
        let mut store = shop();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        place(&mut store, 1, &month(), 1);
        store
            .conn
            .execute_batch(&format!(
                "PRAGMA foreign_keys = OFF; UPDATE orders SET {change};"
            ))
            .unwrap();
        for read in [
            store.orders_of(account.id, 10).map(|_| ()),
            store.order_of(account.id, &oid(1)).map(|_| ()),
        ] {
            assert!(
                matches!(&read, Err(StoreError::Corrupt { what: found, .. }) if *found == what),
                "{change}: {read:?}"
            );
        }
    }
    // A state the schema refuses cannot be stored at all.
    let mut store = shop();
    customer(&mut store, "a@qq.com", 1, "pilotusera");
    place(&mut store, 1, &month(), 1);
    for refused in [
        "UPDATE orders SET state = 'done'",
        "UPDATE orders SET state = 'review'",
        "UPDATE orders SET review_reason = 'amount'",
        "UPDATE orders SET state = 'paid'",
        "UPDATE orders SET paid_at = 1",
        "UPDATE orders SET card_passed = 2",
        "UPDATE orders SET id = 'short'",
    ] {
        assert!(store.conn.execute(refused, []).is_err(), "{refused}");
    }
    // Nor can an account have two unpaid orders for one plan; a paid one
    // beside the unpaid one it may.
    let copy = |id: &str, state: &str, paid_at: &str| {
        format!(
            "INSERT INTO orders (id, account_id, mooring_user, plan, plan_kind, days, \
             traffic_bytes, amount, currency, payment_link_id, created_at, state, paid_at) \
             SELECT '{id}', account_id, mooring_user, plan, plan_kind, days, traffic_bytes, \
             amount, currency, payment_link_id, created_at, '{state}', {paid_at} FROM orders \
             WHERE id = '11111111111111111111'"
        )
    };
    let second = store
        .conn
        .execute(&copy("22222222222222222222", "pending", "NULL"), []);
    assert!(
        matches!(&second, Err(error) if error.to_string().contains("UNIQUE")),
        "{second:?}"
    );
    assert_eq!(
        store
            .conn
            .execute(&copy("22222222222222222222", "paid", "1"), [])
            .unwrap(),
        1
    );
    store
        .conn
        .execute("DELETE FROM orders WHERE id = '22222222222222222222'", [])
        .unwrap();
    // A reason that is none is found when the order is read.
    store
        .conn
        .execute(
            "UPDATE orders SET state = 'review', review_reason = 'whim', paid_at = 1",
            [],
        )
        .unwrap();
    let account = store.session_account(&digest(1), at(NOW)).unwrap().unwrap();
    assert!(matches!(
        store.order_of(account.id, &oid(1)),
        Err(StoreError::Corrupt {
            what: "order's review reason",
            ..
        })
    ));
    // So is a state that is none, or one with a reason it cannot have.
    for change in [
        "state = 'done'",
        "state = 'paid', review_reason = 'amount'",
        "state = 'pending', review_reason = 'amount'",
    ] {
        store
            .conn
            .execute_batch(&format!(
                "PRAGMA ignore_check_constraints = ON; UPDATE orders SET {change};"
            ))
            .unwrap();
        let read = store.order_of(account.id, &oid(1));
        assert!(
            matches!(
                read,
                Err(StoreError::Corrupt {
                    what: "order's state",
                    ..
                })
            ),
            "{change}: {read:?}"
        );
    }
}

#[test]
fn a_payment_for_an_order_whose_terms_are_no_longer_sold_waits_for_the_operator() {
    // The month cost more when these orders were placed.
    let dearer = OrderTerms {
        amount: 900,
        ..month()
    };
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let other = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let kept = place(&mut store, 1, &dearer, 1);
    let short = place(&mut store, 2, &dearer, 2);
    record_all(
        &mut store,
        &[
            // Paid as the order asked, from an address kept.
            event(1, 60, completed(1, kept, &dearer)),
            event(2, 61, charged("pi_1", 1)),
            // What is wrong with a checkout itself is said first.
            event(
                3,
                62,
                EventKind::CheckoutCompleted(CheckoutSession {
                    amount_total: Some(800),
                    ..checkout(2, short, &dearer)
                }),
            ),
        ],
    );
    assert_eq!(
        progress(&store, &account, kept),
        Progress::Review(ReviewReason::Terms)
    );
    assert_eq!(
        progress(&store, &other, short),
        Progress::Review(ReviewReason::Amount)
    );
    assert_eq!(entries(&store), []);

    // While the catalogue still has the order's terms, it is paid like
    // any other: what is on sale is what the caller says.
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &dearer, 1);
    let paid = event(1, 60, completed(1, order, &dearer));
    assert_eq!(
        store
            .record_stripe_event(&paid, std::slice::from_ref(&dearer), at(NOW + 61))
            .unwrap(),
        Recorded::New
    );
    assert_eq!(progress(&store, &account, order), Progress::Checking);
}
