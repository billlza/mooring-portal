//! A broken database is an error from every operation, never an empty
//! answer.

use portal_core::account::AccountCode;

use super::*;
use crate::{
    BindError, DeleteError, ImportError, LinkError, ProfileImportError, RegisterError, StoreError,
    UnbindError,
};

/// A store whose tables are gone after `account` registered.
fn broken() -> (Store, Account) {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .conn
        .execute_batch(
            "DROP TABLE subscription_links; DROP TABLE cfm_profiles; DROP TABLE profile_bundle; \
             DROP TABLE subscription_records; DROP TABLE bindings; DROP TABLE sessions; \
             DROP TABLE accounts;",
        )
        .unwrap();
    (store, account)
}

fn sqlite<T: std::fmt::Debug>(result: Result<T, StoreError>) {
    assert!(matches!(result, Err(StoreError::Sqlite(_))), "{result:?}");
}

#[test]
fn reads_fail_loudly() {
    let (store, account) = broken();
    sqlite(store.accounts());
    sqlite(store.credentials(&email("a@qq.com")));
    sqlite(store.credentials_of(account.id));
    sqlite(store.session_account(&digest(1), at(NOW)));
    sqlite(store.binding(account.id));
    sqlite(store.subscription(account.id));
    sqlite(store.cfm_profile(account.id));
    sqlite(store.subscription_link_created(account.id));
    sqlite(store.linked_profile(&digest(1)));
}

#[test]
fn writes_fail_loudly() {
    let (mut store, account) = broken();
    let new = NewAccount {
        code: AccountCode::from_bits(2),
        email: email("b@qq.com"),
        password_hash: hash("x"),
        created_at: at(NOW),
    };
    assert!(matches!(
        store.register(new, 10),
        Err(RegisterError::Store(StoreError::Sqlite(_)))
    ));
    sqlite(store.start_session(&session(&account, 1, NOW, 10)));
    sqlite(store.end_session(&digest(1)));
    sqlite(store.change_password(&hash("y"), &session(&account, 2, NOW, 10)));
    assert!(matches!(
        store.bind(&account.code, &user("pilotusera"), at(NOW)),
        Err(BindError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.unbind(&account.code),
        Err(UnbindError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.delete_account(&account.code),
        Err(DeleteError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW)),
        Err(ImportError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW)),
        Err(ProfileImportError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.replace_subscription_link(account.id, &digest(1), at(NOW)),
        Err(LinkError::Store(StoreError::Sqlite(_)))
    ));
    sqlite(store.revoke_subscription_link(account.id));
}

#[test]
fn a_failure_part_way_through_leaves_nothing_behind() {
    // Registration checks, then inserts: with the insert made to fail, the
    // transaction leaves no account.
    let mut store = store();
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON accounts BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let new = NewAccount {
        code: AccountCode::from_bits(1),
        email: email("a@qq.com"),
        password_hash: hash("x"),
        created_at: at(NOW),
    };
    assert!(matches!(
        store.register(new, 10),
        Err(RegisterError::Store(_))
    ));
    store.conn.execute_batch("DROP TRIGGER refuse;").unwrap();
    assert!(store.accounts().unwrap().is_empty());
}

#[test]
fn a_password_change_that_fails_late_changes_nothing() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .start_session(&session(&account, 1, NOW, 100))
        .unwrap();
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(account.id, &digest(5), at(NOW))
        .unwrap();
    // The update of the hash and the deletion of the sessions and the link
    // run before the new session's insertion fails.
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON sessions BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    sqlite(store.change_password(&hash("second"), &session(&account, 2, NOW + 1, 100)));
    store.conn.execute_batch("DROP TRIGGER refuse;").unwrap();
    assert_eq!(
        store
            .credentials_of(account.id)
            .unwrap()
            .password_hash
            .as_str(),
        hash("first").as_str()
    );
    assert_eq!(
        store.session_account(&digest(1), at(NOW + 1)).unwrap(),
        Some(account.clone())
    );
    assert_eq!(
        store.subscription_link_created(account.id).unwrap(),
        Some(at(NOW))
    );
}

#[test]
fn a_binding_row_with_half_its_columns_is_corrupt() {
    use crate::bindings::binding_from_sql;
    assert!(matches!(
        binding_from_sql(Some("pilotusera".to_owned()), None),
        Err(StoreError::Corrupt {
            what: "binding",
            ..
        })
    ));
    assert!(matches!(
        binding_from_sql(None, Some(1)),
        Err(StoreError::Corrupt {
            what: "binding",
            ..
        })
    ));
    assert_eq!(binding_from_sql(None, None).unwrap(), None);
}
