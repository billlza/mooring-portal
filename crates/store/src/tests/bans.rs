use super::*;
use crate::{Ban, BanError, LinkedProfile, RegisterError, StoreError, UnknownAccount};

/// Registers `address` with code `bits`, or says why not.
fn try_register(store: &mut Store, address: &str, bits: u64) -> Result<Account, RegisterError> {
    store.register(
        NewAccount {
            code: AccountCode::from_bits(bits),
            email: digested(address),
            password_hash: hash("first"),
            created_at: at(NOW + bits),
        },
        100,
    )
}

#[test]
fn a_ban_deletes_the_account_and_keeps_its_address_from_registering_again() {
    let mut store = store();
    let banned = register(&mut store, "Li.Lei@qq.com", 1);
    let other = register(&mut store, "han@qq.com", 2);
    store.start_session(&session(&banned, 1, NOW, 100)).unwrap();
    store
        .bind(&banned.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&other.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    store
        .import_profiles(&bundle(NOW, &["pilotusera", "pilotuserb"]), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(&digest(1), &digest(9), at(NOW))
        .unwrap();
    assert_eq!(
        store.ban(&banned.code, at(NOW + 5)).unwrap(),
        Ban {
            user: Some(user("pilotusera")),
            link_ended: true,
        }
    );
    // No login, no session, no link: the account is gone.
    assert!(
        store
            .credentials(&email("li.lei@qq.com"))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .session_account(&digest(1), at(NOW + 5))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.linked_profile(&digest(9)).unwrap(),
        LinkedProfile::Unknown
    );
    // The address cannot register again, however it is written...
    for variant in ["li.lei@qq.com", "  LI.LEI@QQ.COM "] {
        assert!(
            matches!(
                try_register(&mut store, variant, 3),
                Err(RegisterError::Banned)
            ),
            "{variant}"
        );
    }
    // ...while another address, even one some services read as the same,
    // registers.
    try_register(&mut store, "lilei@qq.com", 4).unwrap();
    assert!(store.credentials(&email("han@qq.com")).unwrap().is_some());
    // Unbanned, the address can register again.
    assert!(store.unban(&ban_digest("LI.LEI@qq.com")).unwrap());
    assert!(!store.unban(&ban_digest("li.lei@qq.com")).unwrap());
    try_register(&mut store, "li.lei@qq.com", 5).unwrap();
}

#[test]
fn a_ban_says_what_it_removed_and_refuses_an_unknown_code() {
    let mut store = store();
    let loose = register(&mut store, "a@qq.com", 1);
    assert_eq!(
        store.ban(&loose.code, at(NOW)).unwrap(),
        Ban {
            user: None,
            link_ended: false,
        }
    );
    assert!(matches!(
        store.ban(&loose.code, at(NOW)),
        Err(BanError::Unknown(UnknownAccount(code))) if code == loose.code
    ));
}

#[test]
fn a_ban_that_fails_late_changes_nothing() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE DELETE ON accounts BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.ban(&account.code, at(NOW)),
        Err(BanError::Store(StoreError::Sqlite(_)))
    ));
    store.conn.execute_batch("DROP TRIGGER refuse;").unwrap();
    assert!(store.credentials(&email("a@qq.com")).unwrap().is_some());
    assert!(
        !store.unban(&ban_digest("a@qq.com")).unwrap(),
        "the address was not banned"
    );
}
