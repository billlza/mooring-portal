//! A database the first release left, at schema version 1, is brought to
//! the current version with its data, and the new tables cascade from it.

use rusqlite::{Connection, params};

use super::paying::{
    auto, charged, customer, event, invoice_paid_by, invoiced, oid, record_all, sid,
};
use super::*;
use crate::schema::MIGRATIONS;
use crate::{CfmDelivery, LinkedProfile, OpenMode, Progress, Subscription};
use portal_core::payment::OrderId;
use portal_core::payment::stripe::BillingReason;

#[test]
fn a_version_1_database_keeps_its_data_and_gains_profiles_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
             VALUES ('0000-0000-0001', 'a@qq.com', ?1, ?2, ?2)",
            params![hash("first").as_str(), NOW.cast_signed()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (token_digest, account_id, created_at, expires_at)
             VALUES (?1, 1, ?2, ?3)",
            params![
                digest(1).as_bytes(),
                NOW.cast_signed(),
                (NOW + 10_000).cast_signed()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bindings (account_id, mooring_user, bound_at) VALUES (1, 'pilotusera', ?1)",
            [NOW.cast_signed()],
        )
        .unwrap();
    }
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
    let account = store.session_account(&digest(1), at(NOW)).unwrap().unwrap();
    assert!(matches!(
        store.subscription(account.id).unwrap(),
        Subscription::Bound { record: None, .. }
    ));
    store
        .import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(&digest(1), &digest(2), at(NOW))
        .unwrap();
    assert!(matches!(
        store.cfm_delivery(account.id).unwrap(),
        CfmDelivery::Profile {
            link_created_at: Some(_),
            ..
        }
    ));
    // Unbinding cascades to the profile and the link of the new tables.
    store.unbind(&account.code).unwrap();
    assert_eq!(
        store.linked_profile(&digest(2)).unwrap(),
        LinkedProfile::Unknown
    );
    let left: i64 = store
        .conn
        .query_row(
            "SELECT (SELECT count(*) FROM cfm_profiles) + (SELECT count(*) FROM subscription_links)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
    let violations: i64 = store
        .conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

#[test]
fn a_version_5_subscription_still_renews_at_its_original_price() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    let first = oid(1);
    let account = {
        let conn = Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        let mut store = Store { conn };
        store.enter_payment_mode(false).unwrap();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        store.conn.execute(
            "INSERT INTO orders (id, account_id, mooring_user, plan, plan_kind, days, traffic_bytes,
                 amount, currency, payment_link_id, created_at, state, paid_at, subscription)
             VALUES (?1, ?2, 'pilotusera', 'auto', 'month_auto', 30, 100000000000,
                 750, 'gbp', 'plink_1auto', ?3, 'paid', ?3, 'sub_1')",
            params![first.as_str(), account.id.0, NOW.cast_signed()],
        ).unwrap();
        account.id
    };
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let order = store.order_of(account, &first).unwrap().unwrap();
    assert_eq!(order.terms, auto());
    let cycle = 30 * 86_400;
    record_all(
        &mut store,
        &[
            event(
                1,
                cycle,
                invoiced("in_2", "sub_1", BillingReason::Cycle, 750),
            ),
            event(2, cycle, invoice_paid_by("in_2", "pi_2")),
            event(3, cycle, charged("pi_2", 1)),
        ],
    );
    let renewal = store
        .order_of(account, &OrderId::for_renewal(&sid("in_2")))
        .unwrap()
        .unwrap();
    assert_eq!(renewal.terms, auto());
    assert_eq!(renewal.renewal_of, Some(first));
    assert_eq!(renewal.progress, Progress::Granted { pulled: false });
}
