//! A database the first release left, at schema version 1, is brought to
//! the current version with its data, and the new tables cascade from it.

use rusqlite::{Connection, params};

use super::*;
use crate::schema::MIGRATIONS;
use crate::{CfmDelivery, LinkedProfile, OpenMode, Subscription};

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
