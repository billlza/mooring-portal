//! Whether a bound account without a profile has paid since the last
//! bundle that withheld its Mooring user: the subscription page then says
//! that the profile is being prepared.

use portal_core::payment::{OrderId, OrderTerms};
use rusqlite::{Connection, params};

use super::paying::*;
use super::*;
use crate::schema::MIGRATIONS;
use crate::{AccountId, CfmDelivery, OpenMode, Progress};

/// What [`Store::cfm_delivery`] says of `account` having paid since its
/// user was last withheld; `None` once it has a profile.
fn paid_since_withheld(store: &Store, account: &Account) -> Option<bool> {
    match store.cfm_delivery(account.id).unwrap() {
        CfmDelivery::NoProfile {
            paid_since_withheld,
            ..
        } => Some(paid_since_withheld),
        CfmDelivery::Profile { .. } => None,
    }
}

/// The time of the last bundle that withheld the user of `account`, as its
/// binding keeps it.
fn withheld_at(store: &Store, account: AccountId) -> Option<u64> {
    store
        .conn
        .query_row(
            "SELECT withheld_at FROM bindings WHERE account_id = ?1",
            [account.0],
            |row| row.get::<_, Option<i64>>(0),
        )
        .unwrap()
        .map(i64::cast_unsigned)
}

/// Imports, five seconds after it was made, a bundle made at `generated`
/// that withholds pilotusera, the only bound user, as Mooring withholds a
/// user who has not paid.
fn withheld(store: &mut Store, generated: u64) {
    let bundle = withholding(generated, &[], &[("pilotusera", WithheldReason::Suspended)]);
    store.import_profiles(&bundle, at(generated + 5)).unwrap();
}

/// An order on `terms` of the customer signed in with session `bits`,
/// paid `after` seconds after [`NOW`] by the checkout numbered `n` and its
/// payment `pi_n`, with a card that passes.
fn paid(store: &mut Store, bits: u8, terms: &OrderTerms, n: u8, after: u64) -> OrderId {
    let order = place(store, bits, terms, n);
    record_all(
        store,
        &[
            event(u32::from(n) * 10, after, completed(n, order, terms)),
            event(u32::from(n) * 10 + 1, after, charged(&format!("pi_{n}"), 1)),
        ],
    );
    order
}

#[test]
fn a_payment_counts_when_made_after_the_last_bundle_that_withheld_the_user() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    withheld(&mut store, NOW + 10);
    assert_eq!(withheld_at(&store, account.id), Some(NOW + 10));
    assert_eq!(paid_since_withheld(&store, &account), Some(false));
    // Paid at NOW + 60: it counts from the checkout, before the card is
    // checked...
    let order = place(&mut store, 1, &month(), 1);
    record_all(&mut store, &[event(10, 60, completed(1, order, &month()))]);
    assert_eq!(progress(&store, &account, order), Progress::Checking);
    assert_eq!(paid_since_withheld(&store, &account), Some(true));
    // ...and after it passed.
    record_all(&mut store, &[event(11, 61, charged("pi_1", 1))]);
    assert_eq!(
        progress(&store, &account, order),
        Progress::Granted { pulled: false }
    );
    assert_eq!(paid_since_withheld(&store, &account), Some(true));
    // A bundle made since then that still withholds the user has answered
    // the payment, even one made in the same second.
    withheld(&mut store, NOW + 60);
    assert_eq!(paid_since_withheld(&store, &account), Some(false));
    // Paying again counts again, until the next such bundle.
    paid(&mut store, 1, &pack(), 2, 90);
    assert_eq!(paid_since_withheld(&store, &account), Some(true));
    withheld(&mut store, NOW + 100);
    assert_eq!(paid_since_withheld(&store, &account), Some(false));
    // A bundle that delivers: a profile, and the user withheld no more.
    store
        .import_profiles(&bundle(NOW + 200, &["pilotusera"]), at(NOW + 205))
        .unwrap();
    assert_eq!(paid_since_withheld(&store, &account), None);
    assert_eq!(withheld_at(&store, account.id), None);
}

#[test]
fn any_payment_counts_while_no_bundle_has_withheld_the_user() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    assert_eq!(withheld_at(&store, account.id), None);
    // An order placed and not paid is no payment.
    let order = place(&mut store, 1, &month(), 1);
    assert_eq!(progress(&store, &account, order), Progress::Pending);
    assert_eq!(paid_since_withheld(&store, &account), Some(false));
    record_all(
        &mut store,
        &[
            event(10, 60, completed(1, order, &month())),
            event(11, 60, charged("pi_1", 1)),
        ],
    );
    assert_eq!(paid_since_withheld(&store, &account), Some(true));
}

#[test]
fn a_payment_in_review_refunded_in_full_or_disputed_does_not_count() {
    let mut store = shop();
    // Paid at the wrong price: it waits for the operator, who may refund
    // it.
    let reviewed = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = short_paid(&mut store, 1, 1);
    assert!(matches!(
        progress(&store, &reviewed, order),
        Progress::Review(_)
    ));
    assert_eq!(paid_since_withheld(&store, &reviewed), Some(false));
    // Refunded in part, it still counts; in full, it does not.
    let refunded_account = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    paid(&mut store, 2, &month(), 2, 60);
    record_all(&mut store, &[event(22, 70, refunded("pi_2", 100, false))]);
    assert_eq!(paid_since_withheld(&store, &refunded_account), Some(true));
    record_all(&mut store, &[event(23, 80, refunded("pi_2", 800, true))]);
    assert_eq!(paid_since_withheld(&store, &refunded_account), Some(false));
    // Disputed by its payer.
    let disputed_account = customer(&mut store, "c@qq.com", 3, "pilotuserc");
    paid(&mut store, 3, &month(), 3, 60);
    assert_eq!(paid_since_withheld(&store, &disputed_account), Some(true));
    record_all(&mut store, &[event(32, 70, disputed("pi_3", 800))]);
    assert_eq!(paid_since_withheld(&store, &disputed_account), Some(false));
}

#[test]
fn only_the_accounts_own_payment_for_its_present_user_counts() {
    let mut store = shop();
    let first = customer(&mut store, "a@qq.com", 1, "pilotusera");
    paid(&mut store, 1, &month(), 1, 60);
    assert_eq!(paid_since_withheld(&store, &first), Some(true));
    // Bound to another user, the account has paid nothing for that one.
    store.unbind(&first.code).unwrap();
    store
        .bind(&first.code, &user("pilotuserb"), at(NOW + 100))
        .unwrap();
    assert_eq!(paid_since_withheld(&store, &first), Some(false));
    // Nor does another account bound to the first user have that payment.
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&second.code, &user("pilotusera"), at(NOW + 100))
        .unwrap();
    assert_eq!(paid_since_withheld(&store, &second), Some(false));
}

#[test]
fn each_import_records_when_it_withheld_a_user_and_a_delivery_clears_it() {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    for (account, id) in [(&first, "pilotusera"), (&second, "pilotuserb")] {
        store.bind(&account.code, &user(id), at(NOW)).unwrap();
    }
    let times = |store: &Store| (withheld_at(store, first.id), withheld_at(store, second.id));
    assert_eq!(times(&store), (None, None));
    // A full import: the first delivered, the second withheld. Each import
    // runs five seconds after its bundle was made, and the bundle's time
    // is what counts.
    let suspended = WithheldReason::Suspended;
    store
        .import_profiles(
            &withholding(NOW + 10, &["pilotusera"], &[("pilotuserb", suspended)]),
            at(NOW + 15),
        )
        .unwrap();
    assert_eq!(times(&store), (None, Some(NOW + 10)));
    // For one account: the first withheld, the second as it was.
    let unagreed = WithheldReason::AgreementNotAccepted;
    store
        .import_profile_for(
            &first.code,
            &withholding(NOW + 20, &[], &[("pilotusera", unagreed)]),
            at(NOW + 25),
        )
        .unwrap();
    assert_eq!(times(&store), (Some(NOW + 20), Some(NOW + 10)));
    // For the other: delivered, so withheld no more.
    store
        .import_profile_for(
            &second.code,
            &bundle(NOW + 30, &["pilotuserb"]),
            at(NOW + 35),
        )
        .unwrap();
    assert_eq!(times(&store), (Some(NOW + 20), None));
    // A full import withholding both, then one delivering to both.
    let both = [
        ("pilotusera", suspended),
        ("pilotuserb", WithheldReason::Revoked),
    ];
    store
        .import_profiles(&withholding(NOW + 40, &[], &both), at(NOW + 45))
        .unwrap();
    assert_eq!(times(&store), (Some(NOW + 40), Some(NOW + 40)));
    store
        .import_profiles(
            &bundle(NOW + 50, &["pilotusera", "pilotuserb"]),
            at(NOW + 55),
        )
        .unwrap();
    assert_eq!(times(&store), (None, None));
    // The time goes with the binding.
    store
        .import_profiles(&withholding(NOW + 60, &[], &both), at(NOW + 65))
        .unwrap();
    store.unbind(&first.code).unwrap();
    store
        .bind(&first.code, &user("pilotusera"), at(NOW + 70))
        .unwrap();
    assert_eq!(times(&store), (None, Some(NOW + 60)));
}

/// A database at schema version 7 with two accounts, the first bound to
/// pilotusera and the second to pilotuserb; given the last bundle's time,
/// with the first one's profile from that bundle.
fn version_7(path: &std::path::Path, bundle_time: Option<u64>) {
    let conn = Connection::open(path).unwrap();
    for sql in &MIGRATIONS[..7] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", 7).unwrap();
    for (code, email, user) in [
        ("0000-0000-0001", "a@qq.com", "pilotusera"),
        ("0000-0000-0002", "b@qq.com", "pilotuserb"),
    ] {
        conn.execute(
            "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![code, email, hash("x").as_str(), NOW.cast_signed()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bindings (account_id, mooring_user, bound_at)
             SELECT id, ?2, ?3 FROM accounts WHERE code = ?1",
            params![code, user, NOW.cast_signed()],
        )
        .unwrap();
    }
    if let Some(generated) = bundle_time {
        conn.execute(
            "INSERT INTO profile_bundle (id, generated_at) VALUES (1, ?1)",
            [generated.cast_signed()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cfm_profiles (mooring_user, document, generated_at)
             VALUES ('pilotusera', ?1, ?2)",
            params![cfm("pilotusera").as_str(), generated.cast_signed()],
        )
        .unwrap();
    }
}

#[test]
fn a_version_7_database_takes_its_accounts_without_a_profile_as_withheld_by_the_last_bundle() {
    for bundle_time in [Some(NOW - 60), None] {
        let dir = tempfile::tempdir().unwrap();
        let path = new_file(&dir);
        version_7(&path, bundle_time);
        let store = Store::open(&path, OpenMode::Existing).unwrap();
        // The account with a profile is withheld by nothing; the one
        // without, by the last bundle, if there was one.
        assert_eq!(withheld_at(&store, AccountId(1)), None, "{bundle_time:?}");
        assert_eq!(withheld_at(&store, AccountId(2)), bundle_time);
        let version: i64 = store
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
    }
}
