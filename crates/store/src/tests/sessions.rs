use super::*;
use crate::{MAX_SESSIONS_PER_ACCOUNT, StoreError};

#[test]
fn a_session_names_its_account_until_it_expires() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .start_session(&session(&account, 7, NOW, 100))
        .unwrap();
    assert_eq!(
        store.session_account(&digest(7), at(NOW)).unwrap(),
        Some(account.clone())
    );
    assert_eq!(
        store.session_account(&digest(7), at(NOW + 99)).unwrap(),
        Some(account)
    );
    assert_eq!(
        store.session_account(&digest(7), at(NOW + 100)).unwrap(),
        None
    );
    assert_eq!(store.session_account(&digest(8), at(NOW)).unwrap(), None);
}

#[test]
fn ending_a_session_logs_out_that_browser_only() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .start_session(&session(&account, 1, NOW, 100))
        .unwrap();
    store
        .start_session(&session(&account, 2, NOW, 100))
        .unwrap();
    store.end_session(&digest(1)).unwrap();
    store.end_session(&digest(9)).unwrap();
    assert_eq!(store.session_account(&digest(1), at(NOW)).unwrap(), None);
    assert!(
        store
            .session_account(&digest(2), at(NOW))
            .unwrap()
            .is_some()
    );
}

#[test]
fn an_account_keeps_only_its_newest_sessions() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    let other = register(&mut store, "b@qq.com", 2);
    store
        .start_session(&session(&other, 200, NOW, 1000))
        .unwrap();
    let total = u8::try_from(MAX_SESSIONS_PER_ACCOUNT).unwrap() + 2;
    for byte in 1..=total {
        store
            .start_session(&session(&account, byte, NOW + u64::from(byte), 1000))
            .unwrap();
    }
    for byte in [1, 2] {
        assert_eq!(
            store.session_account(&digest(byte), at(NOW + 20)).unwrap(),
            None
        );
    }
    for byte in 3..=total {
        assert!(
            store
                .session_account(&digest(byte), at(NOW + 20))
                .unwrap()
                .is_some()
        );
    }
    // Another account's sessions do not count against it.
    assert!(
        store
            .session_account(&digest(200), at(NOW + 20))
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_new_session_sweeps_out_expired_ones() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    let other = register(&mut store, "b@qq.com", 2);
    store.start_session(&session(&other, 1, NOW, 10)).unwrap();
    store
        .start_session(&session(&account, 2, NOW + 10, 10))
        .unwrap();
    let rows: i64 = store
        .conn
        .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn changing_the_password_replaces_every_session() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .start_session(&session(&account, 1, NOW, 1000))
        .unwrap();
    store
        .start_session(&session(&account, 2, NOW, 1000))
        .unwrap();
    store
        .change_password(&hash("second"), &session(&account, 3, NOW + 5, 1000))
        .unwrap();
    assert_eq!(
        store.credentials_of(account.id).unwrap().password_hash,
        hash("second")
    );
    assert_eq!(
        store.session_account(&digest(1), at(NOW + 6)).unwrap(),
        None
    );
    assert_eq!(
        store.session_account(&digest(2), at(NOW + 6)).unwrap(),
        None
    );
    assert_eq!(
        store.session_account(&digest(3), at(NOW + 6)).unwrap(),
        Some(account.clone())
    );
    let changed_at: i64 = store
        .conn
        .query_row("SELECT password_changed_at FROM accounts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(changed_at, i64::try_from(NOW + 5).unwrap());
    store.delete_account(&account.code).unwrap();
    assert!(matches!(
        store.change_password(&hash("third"), &session(&account, 4, NOW + 9, 1000)),
        Err(StoreError::Missing { what: "account" })
    ));
}
