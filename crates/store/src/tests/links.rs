use super::*;
use crate::{LinkError, LinkedProfile, PasswordChange, StoreError};

/// Two accounts bound to pilotusera and pilotuserb, whose profiles are
/// imported.
fn two_bound_accounts() -> (Store, Account, Account) {
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
        .import_profiles(&bundle(NOW, &["pilotusera", "pilotuserb"]), at(NOW))
        .unwrap();
    (store, first, second)
}

#[test]
fn a_link_serves_its_own_accounts_profile_until_replaced_or_revoked() {
    let (mut store, first, second) = two_bound_accounts();
    store
        .replace_subscription_link(first.id, &digest(1), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(second.id, &digest(2), at(NOW + 1))
        .unwrap();
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Profile(cfm("pilotusera"))
    );
    assert_eq!(
        store.linked_profile(&digest(2)).unwrap(),
        LinkedProfile::Profile(cfm("pilotuserb"))
    );
    assert_eq!(
        store.subscription_link_created(first.id).unwrap(),
        Some(at(NOW))
    );
    // A new link replaces the old one.
    store
        .replace_subscription_link(first.id, &digest(3), at(NOW + 5))
        .unwrap();
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Unknown
    );
    assert_eq!(
        store.linked_profile(&digest(3)).unwrap(),
        LinkedProfile::Profile(cfm("pilotusera"))
    );
    assert_eq!(
        store.subscription_link_created(first.id).unwrap(),
        Some(at(NOW + 5))
    );
    assert!(store.revoke_subscription_link(first.id).unwrap());
    assert!(!store.revoke_subscription_link(first.id).unwrap());
    assert_eq!(
        store.linked_profile(&digest(3)).unwrap(),
        LinkedProfile::Unknown
    );
    assert_eq!(store.subscription_link_created(first.id).unwrap(), None);
    // The other account's link is untouched throughout.
    assert_eq!(
        store.linked_profile(&digest(2)).unwrap(),
        LinkedProfile::Profile(cfm("pilotuserb"))
    );
}

#[test]
fn a_link_whose_user_has_no_profile_serves_nothing() {
    let (mut store, first, _) = two_bound_accounts();
    store
        .replace_subscription_link(first.id, &digest(1), at(NOW))
        .unwrap();
    store
        .import_profiles(&bundle(NOW + 1, &["pilotuserb"]), at(NOW + 1))
        .unwrap();
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::NoProfile
    );
}

#[test]
fn only_a_bound_account_has_a_link_and_unbinding_removes_it() {
    let (mut store, first, second) = two_bound_accounts();
    let loose = register(&mut store, "c@qq.com", 3);
    assert!(matches!(
        store.replace_subscription_link(loose.id, &digest(9), at(NOW)),
        Err(LinkError::Unbound)
    ));
    // Bound, but the last bundle did not list the user.
    store
        .bind(&loose.code, &user("pilotuserc"), at(NOW))
        .unwrap();
    assert!(matches!(
        store.replace_subscription_link(loose.id, &digest(9), at(NOW)),
        Err(LinkError::NoProfile)
    ));
    assert_eq!(store.subscription_link_created(loose.id).unwrap(), None);
    store
        .replace_subscription_link(first.id, &digest(1), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(second.id, &digest(2), at(NOW))
        .unwrap();
    store.unbind(&first.code).unwrap();
    store.delete_account(&second.code).unwrap();
    for byte in [1, 2, 9] {
        assert_eq!(
            store.linked_profile(&digest(byte)).unwrap(),
            LinkedProfile::Unknown
        );
    }
    // A new binding does not bring the old link back.
    store
        .bind(&first.code, &user("pilotusera"), at(NOW + 1))
        .unwrap();
    assert_eq!(store.subscription_link_created(first.id).unwrap(), None);
}

#[test]
fn no_two_accounts_share_a_token() {
    let (mut store, first, second) = two_bound_accounts();
    store
        .replace_subscription_link(first.id, &digest(1), at(NOW))
        .unwrap();
    assert!(matches!(
        store.replace_subscription_link(second.id, &digest(1), at(NOW)),
        Err(LinkError::Store(StoreError::Sqlite(_)))
    ));
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Profile(cfm("pilotusera"))
    );
}

#[test]
fn changing_the_password_removes_the_link() {
    let (mut store, first, second) = two_bound_accounts();
    store
        .replace_subscription_link(first.id, &digest(1), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(second.id, &digest(2), at(NOW))
        .unwrap();
    assert_eq!(
        store
            .change_password(&hash("second"), &session(&first, 7, NOW + 1, 100))
            .unwrap(),
        PasswordChange { link_revoked: true }
    );
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Unknown
    );
    assert_eq!(
        store
            .change_password(&hash("third"), &session(&first, 8, NOW + 2, 100))
            .unwrap(),
        PasswordChange {
            link_revoked: false
        }
    );
    assert_eq!(
        store.linked_profile(&digest(2)).unwrap(),
        LinkedProfile::Profile(cfm("pilotuserb"))
    );
}

#[test]
fn a_linked_profile_that_no_longer_reads_is_corrupt() {
    let (mut store, first, _) = two_bound_accounts();
    store
        .replace_subscription_link(first.id, &digest(1), at(NOW))
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE cfm_profiles SET document = 'not json' WHERE mooring_user = 'pilotusera'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.linked_profile(&digest(1)),
        Err(StoreError::Corrupt {
            what: "CFM profile",
            ..
        })
    ));
    store
        .conn
        .execute("UPDATE subscription_links SET created_at = -1", [])
        .unwrap();
    assert!(matches!(
        store.subscription_link_created(first.id),
        Err(StoreError::Corrupt {
            what: "link creation time",
            ..
        })
    ));
}
