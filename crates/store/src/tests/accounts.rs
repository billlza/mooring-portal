use super::*;
use crate::{
    AccountSummary, Binding, DeleteError, RegisterError, StoreError, Subscription, UnknownAccount,
};

#[test]
fn registration_records_the_account_and_its_credentials() {
    let mut store = store();
    let account = register(&mut store, "lilei@qq.com", 1);
    assert_eq!(account.code, AccountCode::from_bits(1));
    assert_eq!(account.email, email("lilei@qq.com"));
    assert_eq!(account.created_at, at(NOW + 1));
    let found = store.credentials(&email("lilei@qq.com")).unwrap().unwrap();
    assert_eq!(found.account, account.id);
    assert_eq!(found.password_hash, hash("first"));
    assert_eq!(
        store.credentials_of(account.id).unwrap().password_hash,
        hash("first")
    );
    assert!(
        store
            .credentials(&email("hanmeimei@qq.com"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_email_registers_once() {
    let mut store = store();
    register(&mut store, "lilei@qq.com", 1);
    let again = store.register(
        NewAccount {
            code: AccountCode::from_bits(2),
            email: email("LiLei@QQ.com"),
            password_hash: hash("second"),
            created_at: at(NOW),
        },
        100,
    );
    assert!(matches!(again, Err(RegisterError::EmailTaken)));
    assert_eq!(store.accounts().unwrap().len(), 1);
}

#[test]
fn registration_stops_at_the_account_limit() {
    let mut store = store();
    register(&mut store, "a@qq.com", 1);
    let new = |bits| NewAccount {
        code: AccountCode::from_bits(bits),
        email: email(&format!("user{bits}@qq.com")),
        password_hash: hash("x"),
        created_at: at(NOW),
    };
    assert!(matches!(
        store.register(new(2), 1),
        Err(RegisterError::Full { limit: 1 })
    ));
    store.register(new(3), 2).unwrap();
    assert!(matches!(
        store.register(new(4), 0),
        Err(RegisterError::Full { limit: 0 })
    ));
}

#[test]
fn accounts_list_oldest_first_with_their_bindings() {
    let mut store = store();
    let second = register(&mut store, "b@qq.com", 2);
    let first = register(&mut store, "a@qq.com", 1);
    store
        .bind(&second.code, &user("pilotusera"), at(NOW + 10))
        .unwrap();
    assert_eq!(
        store.accounts().unwrap(),
        [
            AccountSummary {
                code: first.code,
                email: email("a@qq.com"),
                created_at: at(NOW + 1),
                binding: None,
            },
            AccountSummary {
                code: second.code,
                email: email("b@qq.com"),
                created_at: at(NOW + 2),
                binding: Some(Binding {
                    user: user("pilotusera"),
                    bound_at: at(NOW + 10),
                }),
            },
        ]
    );
}

#[test]
fn deleting_an_account_removes_its_sessions_binding_and_record() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    let other = register(&mut store, "b@qq.com", 2);
    store
        .start_session(&session(&account, 1, NOW, 3600))
        .unwrap();
    store.start_session(&session(&other, 2, NOW, 3600)).unwrap();
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .import(&snapshot(NOW, &[("pilotusera", Some(5))]), at(NOW))
        .unwrap();
    store.delete_account(&account.code).unwrap();
    assert!(store.credentials(&email("a@qq.com")).unwrap().is_none());
    assert!(
        store
            .session_account(&digest(1), at(NOW))
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store.credentials_of(account.id),
        Err(StoreError::Missing { what: "account" })
    ));
    let records: i64 = store
        .conn
        .query_row("SELECT count(*) FROM subscription_records", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(records, 0);
    // The other account is untouched, and the user may be bound again.
    assert_eq!(
        store.session_account(&digest(2), at(NOW)).unwrap(),
        Some(other.clone())
    );
    store
        .bind(&other.code, &user("pilotusera"), at(NOW))
        .unwrap();
    assert!(matches!(
        store.delete_account(&account.code),
        Err(DeleteError::Unknown(UnknownAccount(code))) if code == account.code
    ));
    assert!(matches!(
        store.subscription(other.id).unwrap(),
        Subscription::Bound { record: None, .. }
    ));
}

#[test]
fn rows_that_no_longer_parse_are_reported_as_corrupt() {
    let mut store = store();
    register(&mut store, "a@qq.com", 1);
    store
        .conn
        .execute("UPDATE accounts SET email = 'not an email'", [])
        .unwrap();
    assert!(matches!(
        store.accounts(),
        Err(StoreError::Corrupt {
            what: "email address",
            ..
        })
    ));
    store
        .conn
        .execute("UPDATE accounts SET email = 'a@qq.com', code = 'oops'", [])
        .unwrap();
    assert!(matches!(
        store.accounts(),
        Err(StoreError::Corrupt {
            what: "account code",
            ..
        })
    ));
    store
        .conn
        .execute(
            "UPDATE accounts SET code = '0000-0000-0001', created_at = -5",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.accounts(),
        Err(StoreError::Corrupt {
            what: "account creation time",
            ..
        })
    ));
}
