use super::*;
use crate::{BindError, Binding, StoreError, UnbindError, UnknownAccount};

#[test]
fn an_account_binds_to_one_user_and_back() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    assert_eq!(store.binding(account.id).unwrap(), None);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    assert_eq!(
        store.binding(account.id).unwrap(),
        Some(Binding {
            user: user("pilotusera"),
            bound_at: at(NOW)
        })
    );
    assert_eq!(store.unbind(&account.code).unwrap(), user("pilotusera"));
    assert_eq!(store.binding(account.id).unwrap(), None);
}

#[test]
fn bindings_are_one_to_one() {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    assert!(matches!(
        store.bind(&first.code, &user("pilotuserb"), at(NOW)),
        Err(BindError::AccountBound { account, user: bound })
            if account == first.code && bound == user("pilotusera")
    ));
    assert!(matches!(
        store.bind(&second.code, &user("pilotusera"), at(NOW)),
        Err(BindError::UserBound { user: bound, account })
            if account == first.code && bound == user("pilotusera")
    ));
    store
        .bind(&second.code, &user("pilotuserb"), at(NOW))
        .unwrap();
}

#[test]
fn unknown_and_unbound_accounts_are_named() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    let stranger = AccountCode::from_bits(99);
    assert!(matches!(
        store.bind(&stranger, &user("pilotusera"), at(NOW)),
        Err(BindError::UnknownAccount(UnknownAccount(code))) if code == stranger
    ));
    assert!(matches!(
        store.unbind(&stranger),
        Err(UnbindError::UnknownAccount(UnknownAccount(code))) if code == stranger
    ));
    assert!(matches!(
        store.unbind(&account.code),
        Err(UnbindError::NotBound(code)) if code == account.code
    ));
}

#[test]
fn a_corrupt_binding_is_reported() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .conn
        .execute("UPDATE bindings SET mooring_user = 'Not-An-Id'", [])
        .unwrap();
    assert!(matches!(
        store.binding(account.id),
        Err(StoreError::Corrupt {
            what: "Mooring user id",
            ..
        })
    ));
    assert!(matches!(
        store.bind(&account.code, &user("pilotuserb"), at(NOW)),
        Err(BindError::Store(StoreError::Corrupt { .. }))
    ));
    assert!(matches!(
        store.unbind(&account.code),
        Err(UnbindError::Store(StoreError::Corrupt { .. }))
    ));
}
