//! A broken database is an error from every operation, never an empty
//! answer.

use portal_core::account::AccountCode;

use super::*;
use crate::{
    AccountProfileError, BanError, BindError, DeleteError, ImportError, LinkError, LinkedProfile,
    ProfileImportError, RegisterError, RevokeError, StoreError, UnbindError,
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
             DROP TABLE accounts; DROP TABLE banned_emails;",
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
    sqlite(store.cfm_delivery(account.id));
    sqlite(store.subscription_link_created(account.id));
    sqlite(store.linked_profile(&digest(1)));
}

#[test]
fn writes_fail_loudly() {
    let (mut store, account) = broken();
    let new = NewAccount {
        code: AccountCode::from_bits(2),
        email: digested("b@qq.com"),
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
        store.ban(&account.code, &[], at(NOW)),
        Err(BanError::Store(StoreError::Sqlite(_)))
    ));
    sqlite(store.unban(&ban_digest("a@qq.com")));
    assert!(matches!(
        store.import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW)),
        Err(ImportError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW)),
        Err(ProfileImportError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.import_profile_for(&account.code, &bundle(NOW, &["pilotusera"]), at(NOW)),
        Err(AccountProfileError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.replace_subscription_link(&digest(1), &digest(2), at(NOW)),
        Err(LinkError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.revoke_subscription_link(&digest(1), at(NOW)),
        Err(RevokeError::Store(StoreError::Sqlite(_)))
    ));
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
        email: digested("a@qq.com"),
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
fn a_binding_whose_write_fails_changes_nothing() {
    // Binding and unbinding check the account, then write: with the write
    // made to fail, each is an error and the accounts stay as they were.
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&second.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse_bind BEFORE INSERT ON bindings \
             BEGIN SELECT RAISE(ABORT, 'refused'); END; \
             CREATE TRIGGER refuse_unbind BEFORE DELETE ON bindings \
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.bind(&first.code, &user("pilotusera"), at(NOW)),
        Err(BindError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.unbind(&second.code),
        Err(UnbindError::Store(StoreError::Sqlite(_)))
    ));
    assert_eq!(store.binding(first.id).unwrap(), None);
    assert_eq!(
        store
            .binding(second.id)
            .unwrap()
            .map(|binding| binding.user),
        Some(user("pilotuserb"))
    );
}

#[test]
fn a_snapshot_import_that_fails_part_way_stores_nothing() {
    // With the insert of pilotuserb's record made to fail, the transaction
    // keeps no record, however far it got.
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&second.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON subscription_records \
             WHEN NEW.mooring_user = 'pilotuserb' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let both = snapshot(NOW, &[("pilotusera", Some(1)), ("pilotuserb", Some(2))]);
    assert!(matches!(
        store.import(&both, at(NOW)),
        Err(ImportError::Store(StoreError::Sqlite(_)))
    ));
    store.conn.execute_batch("DROP TRIGGER refuse;").unwrap();
    let kept: i64 = store
        .conn
        .query_row("SELECT count(*) FROM subscription_records", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kept, 0);
}

#[test]
fn a_profile_import_that_cannot_read_the_links_fails() {
    // Without the links table the import cannot tell whether a withheld
    // account has a link to pause or to end, and fails instead of guessing.
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW))
        .unwrap();
    store
        .conn
        .execute_batch("DROP TABLE subscription_links;")
        .unwrap();
    for reason in [WithheldReason::Suspended, WithheldReason::Revoked] {
        let withheld = withholding(NOW + 1, &[], &[("pilotusera", reason)]);
        assert!(
            matches!(
                store.import_profiles(&withheld, at(NOW + 1)),
                Err(ProfileImportError::Store(StoreError::Sqlite(_)))
            ),
            "{reason}"
        );
    }
    assert!(store.cfm_profile(account.id).unwrap().is_some());
}

#[test]
fn a_profile_import_that_fails_late_changes_nothing() {
    // The import replaces the profiles and ends a withheld account's link
    // before it writes the bundle's time. With that last write made to fail,
    // the profiles, the link and the time stay as they were.
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store.start_session(&session(&second, 2, NOW, 100)).unwrap();
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&second.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    store
        .import_profiles(&bundle(NOW, &["pilotusera", "pilotuserb"]), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(&digest(2), &digest(9), at(NOW))
        .unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse_insert BEFORE INSERT ON profile_bundle \
             BEGIN SELECT RAISE(ABORT, 'refused'); END; \
             CREATE TRIGGER refuse_update BEFORE UPDATE ON profile_bundle \
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    let renewed = withholding(
        NOW + 10,
        &["pilotusera"],
        &[("pilotuserb", WithheldReason::Revoked)],
    );
    assert!(matches!(
        store.import_profiles(&renewed, at(NOW + 10)),
        Err(ProfileImportError::Store(StoreError::Sqlite(_)))
    ));
    store
        .conn
        .execute_batch("DROP TRIGGER refuse_insert; DROP TRIGGER refuse_update;")
        .unwrap();
    for (account, id) in [(&first, "pilotusera"), (&second, "pilotuserb")] {
        assert_eq!(
            store.cfm_profile(account.id).unwrap(),
            Some(StoredProfile {
                profile: cfm(id),
                generated_at: at(NOW),
            }),
            "{id}"
        );
    }
    assert_eq!(
        store.linked_profile(&digest(9)).unwrap(),
        LinkedProfile::Profile(cfm("pilotuserb"))
    );
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
        .replace_subscription_link(&digest(1), &digest(5), at(NOW))
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
