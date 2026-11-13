//! When an account with payments may go, and what a ban keeps of its
//! cards.

use portal_core::payment::stripe::{BillingReason, Renewal};

use super::paying::*;
use super::*;
use crate::{Ban, DeleteError, Progress};

#[test]
fn an_account_goes_only_once_its_money_is_settled() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let other = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let refused = |store: &mut Store| -> String {
        match store.delete_account(&account.code) {
            Err(error @ (DeleteError::Renewing { .. } | DeleteError::Unsettled { .. })) => {
                error.to_string()
            }
            other => panic!("{other:?}"),
        }
    };
    // An unpaid order holds nothing up, so it is tried on a copy.
    place(&mut store, 1, &pack(), 9);
    let paid = place(&mut store, 1, &month(), 1);
    // Paid, its card not yet checked.
    record_all(&mut store, &[event(1, 60, completed(1, paid, &month()))]);
    assert!(refused(&mut store).contains(&format!("orders that are not settled ({paid})")));
    // Granted, but Mooring does not have it yet.
    record_all(&mut store, &[event(2, 61, charged("pi_1", 1))]);
    assert!(refused(&mut store).contains(&paid.to_string()));
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    store
        .pull_entitlements(&digest(77), 0, 10, at(NOW))
        .unwrap();
    assert!(refused(&mut store).contains("once Mooring has pulled it"));
    store
        .pull_entitlements(&digest(77), 1, 10, at(NOW))
        .unwrap();
    // A refund entered since is again one Mooring does not have.
    record_all(&mut store, &[event(3, 100, refunded("pi_1", 10, false))]);
    assert!(refused(&mut store).contains(&paid.to_string()));
    store
        .pull_entitlements(&digest(77), 2, 10, at(NOW))
        .unwrap();
    // An order in review that has not been refunded.
    let review = short_paid(&mut store, 1, 2);
    let said = refused(&mut store);
    assert!(
        said.contains(&format!("({review})")) && said.contains("approve-order"),
        "{said}"
    );
    record_all(&mut store, &[event(4, 200, refunded("pi_2", 1, true))]);
    // A subscription Stripe may charge again, running or ending.
    let renewing = place(&mut store, 1, &auto(), 3);
    record_all(
        &mut store,
        &[
            event(5, 300, completed(3, renewing, &auto())),
            event(6, 301, invoice_paid_by("in_3", "pi_3")),
            event(7, 302, charged("pi_3", 1)),
        ],
    );
    store
        .pull_entitlements(&digest(77), 3, 10, at(NOW))
        .unwrap();
    for state in [Renewal::Active, Renewal::Ending] {
        if state == Renewal::Ending {
            record_all(&mut store, &[event(8, 400, subscription("sub_3", state))]);
        }
        let said = refused(&mut store);
        assert!(
            said.contains("a subscription that Stripe may charge again (sub_3)"),
            "{said}"
        );
    }
    record_all(
        &mut store,
        &[event(9, 500, subscription("sub_3", Renewal::Ended))],
    );
    // The other account's unsettled order is not this one's concern.
    let theirs = place(&mut store, 2, &month(), 4);
    record_all(
        &mut store,
        &[event(10, 600, completed(4, theirs, &month()))],
    );
    store.delete_account(&account.code).unwrap();
    // Its orders, their entries and what Stripe said of them are gone;
    // the other account's are not.
    assert_eq!(store.recent_orders(10).unwrap().len(), 1);
    assert_eq!(entries(&store), []);
    let left = |table: &str, key: &str| -> Vec<String> {
        store
            .conn
            .prepare(&format!("SELECT {key} FROM {table} ORDER BY {key}"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(
        left("stripe_payments", "payment_intent"),
        Vec::<String>::new()
    );
    assert_eq!(
        left("stripe_invoice_payments", "invoice"),
        Vec::<String>::new()
    );
    assert_eq!(
        left("stripe_subscriptions", "subscription"),
        Vec::<String>::new()
    );
    assert_eq!(left("orders", "id"), [theirs.to_string()]);
    assert_eq!(progress(&store, &other, theirs), Progress::Checking);
    assert!(matches!(
        store.delete_account(&other.code),
        Err(DeleteError::Unsettled { .. })
    ));
}

#[test]
fn an_account_with_nothing_owing_goes_at_once() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    // An unpaid order, and one paid, refunded in full before it was
    // granted, and one disputed before it was.
    place(&mut store, 1, &pack(), 9);
    let returned = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, returned, &month())),
            event(2, 61, refunded("pi_1", 800, true)),
        ],
    );
    let contested = place(&mut store, 1, &month(), 2);
    record_all(
        &mut store,
        &[
            event(3, 70, completed(2, contested, &month())),
            event(4, 71, disputed("pi_2", 800)),
        ],
    );
    assert_eq!(progress(&store, &account, returned), Progress::Returned);
    assert_eq!(progress(&store, &account, contested), Progress::Returned);
    // And one that waited for the operator when its payer disputed it:
    // it cannot be approved, and a disputed payment cannot be refunded,
    // so it waits for nothing.
    let waited = short_paid(&mut store, 1, 3);
    record_all(&mut store, &[event(5, 80, disputed("pi_3", 1))]);
    assert_eq!(
        progress(&store, &account, waited),
        Progress::Review(portal_core::payment::ReviewReason::Amount)
    );
    // Another account's subscription, which Stripe may charge again, is
    // not this one's.
    customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let theirs = place(&mut store, 2, &auto(), 4);
    record_all(&mut store, &[event(6, 90, completed(4, theirs, &auto()))]);
    store.delete_account(&account.code).unwrap();
    assert_eq!(rows(&store, "orders"), 1);
    assert_eq!(rows(&store, "stripe_payments"), 0);
}

#[test]
fn deleting_an_account_does_not_make_its_ended_subscription_look_open() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &auto(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &auto())),
            event(2, 61, invoice_paid_by("in_1", "pi_1")),
            event(3, 62, refunded("pi_1", 750, true)),
            event(4, 70, invoiced("in_2", "sub_1", BillingReason::Other, 50)),
            event(5, 80, subscription("sub_1", Renewal::Ended)),
        ],
    );
    store.delete_account(&account.code).unwrap();
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 1);
    assert_eq!(strays[0].open_subscription, None);
    assert!(!store.payments_outstanding().unwrap());
    store.dismiss_stray(strays[0].id, None).unwrap();
}

#[test]
fn a_ban_does_not_wait_and_says_which_subscriptions_to_cancel() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let other = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    customer(&mut store, "c@qq.com", 3, "pilotuserc");
    let renewing = place(&mut store, 1, &auto(), 1);
    let ended = place(&mut store, 2, &auto(), 2);
    let theirs = place(&mut store, 3, &auto(), 3);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, renewing, &auto())),
            event(2, 61, invoice_paid_by("in_1", "pi_1")),
            event(3, 62, charged("pi_1", 5)),
            event(4, 70, completed(2, ended, &auto())),
            event(5, 71, subscription("sub_2", Renewal::Ended)),
            // A third account's, running: not one of the banned
            // account's to cancel.
            event(9, 72, completed(3, theirs, &auto())),
        ],
    );
    // Unsettled as it is, the account goes.
    assert!(matches!(
        store.delete_account(&account.code),
        Err(DeleteError::Renewing { .. })
    ));
    let cards = [
        fingerprint(5).digest(),
        fingerprint(6).digest(),
        fingerprint(5).digest(),
    ];
    assert_eq!(
        store.ban(&account.code, &cards, at(NOW + 100)).unwrap(),
        Ban {
            user: Some(user("pilotusera")),
            link_ended: false,
            subscriptions: vec![sid("sub_1")],
        }
    );
    assert_eq!(
        rows(&store, "banned_cards"),
        2,
        "a card named twice is kept once"
    );
    assert_eq!(entries(&store), []);
    assert_eq!(rows(&store, "stripe_payments"), 0);
    assert_eq!(rows(&store, "stripe_invoice_payments"), 0);
    // An ended subscription is not one to cancel.
    assert_eq!(
        store
            .ban(&other.code, &[], at(NOW + 100))
            .unwrap()
            .subscriptions,
        Vec::new()
    );
    // Stripe renews the first all the same, until the operator cancels
    // it: the renewal is no order's, and its card is the banned one.
    record_all(
        &mut store,
        &[
            event(
                6,
                3_000,
                invoiced("in_9", "sub_1", BillingReason::Cycle, 750),
            ),
            event(7, 3_001, invoice_paid_by("in_9", "pi_9")),
            event(8, 3_002, charged("pi_9", 5)),
        ],
    );
    let strays = store.stray_payments().unwrap();
    assert_eq!(strays.len(), 1);
    assert_eq!(
        (strays[0].subscription.clone(), strays[0].card),
        (
            Some(sid("sub_1")),
            Some(portal_core::payment::CardVerdict::Banned)
        )
    );
}

#[test]
fn cards_found_later_are_added_to_a_ban_and_go_with_it() {
    let mut store = shop();
    let banned = register(&mut store, "banned@qq.com", 1);
    let also = register(&mut store, "also@qq.com", 2);
    store
        .ban(&banned.code, &[fingerprint(1).digest()], at(NOW))
        .unwrap();
    store
        .ban(
            &also.code,
            &[fingerprint(1).digest(), fingerprint(3).digest()],
            at(NOW),
        )
        .unwrap();
    // Only to an address that is banned.
    assert!(
        !store
            .ban_cards(&ban_digest("nobody@qq.com"), &[fingerprint(9).digest()])
            .unwrap()
    );
    assert_eq!(rows(&store, "banned_cards"), 3);
    assert!(
        store
            .ban_cards(
                &ban_digest("banned@qq.com"),
                &[fingerprint(2).digest(), fingerprint(1).digest()]
            )
            .unwrap()
    );
    assert_eq!(rows(&store, "banned_cards"), 4);
    let verdict = |store: &mut Store, n: u32, card: u8| -> String {
        record_all(store, &[event(n, 0, charged(&format!("pi_{n}"), card))]);
        store
            .conn
            .query_row(
                "SELECT card FROM stripe_payments WHERE payment_intent = ?1",
                [format!("pi_{n}")],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(verdict(&mut store, 1, 1), "banned");
    assert_eq!(verdict(&mut store, 2, 2), "banned");
    assert_eq!(verdict(&mut store, 3, 3), "banned");
    assert_eq!(verdict(&mut store, 4, 4), "clear");
    // Unbanning one address frees its cards, but not a card another
    // banned address also paid with.
    assert!(store.unban(&ban_digest("banned@qq.com")).unwrap());
    assert_eq!(rows(&store, "banned_cards"), 2);
    assert_eq!(verdict(&mut store, 5, 2), "clear");
    assert_eq!(verdict(&mut store, 6, 1), "banned");
    assert!(store.unban(&ban_digest("also@qq.com")).unwrap());
    assert_eq!(rows(&store, "banned_cards"), 0);
    assert_eq!(verdict(&mut store, 7, 1), "clear");
    // A digest that is not one, or a ban that does not exist, cannot be
    // stored.
    for refused in [
        "INSERT INTO banned_cards VALUES (x'00', x'00')",
        "INSERT INTO banned_cards VALUES (zeroblob(32), zeroblob(32))",
    ] {
        assert!(store.conn.execute(refused, []).is_err(), "{refused}");
    }
}
