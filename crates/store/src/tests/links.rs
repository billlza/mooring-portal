use super::*;
use crate::{
    BoundAccount, CfmDelivery, LinkError, LinkedProfile, PasswordChange, ProfileImportError,
    RevokeError, StoreError, Withheld, WithheldLink,
};

/// The session digests of the two accounts of [`two_bound_accounts`].
const FIRST: u8 = 101;
const SECOND: u8 = 102;

/// Two signed-in accounts bound to pilotusera and pilotuserb, whose
/// profiles are imported.
fn two_bound_accounts() -> (Store, Account, Account) {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .start_session(&session(&first, FIRST, NOW, 10_000))
        .unwrap();
    store
        .start_session(&session(&second, SECOND, NOW, 10_000))
        .unwrap();
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

/// Makes the link `link` with the session `session` at [`NOW`].
fn make(store: &mut Store, session: u8, link: u8) -> Result<(), LinkError> {
    store.replace_subscription_link(&digest(session), &digest(link), at(NOW))
}

#[test]
fn a_link_serves_its_own_accounts_profile_until_replaced_or_revoked() {
    let (mut store, first, _) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    make(&mut store, SECOND, 2).unwrap();
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
        .replace_subscription_link(&digest(FIRST), &digest(3), at(NOW + 5))
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
    assert!(
        store
            .revoke_subscription_link(&digest(FIRST), at(NOW + 6))
            .unwrap()
    );
    assert!(
        !store
            .revoke_subscription_link(&digest(FIRST), at(NOW + 6))
            .unwrap()
    );
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
fn only_a_live_session_makes_or_removes_a_link() {
    let (mut store, first, _) = two_bound_accounts();
    // Unknown, expired, and ended by logging out.
    assert!(matches!(make(&mut store, 99, 1), Err(LinkError::SignedOut)));
    assert!(matches!(
        store.replace_subscription_link(&digest(FIRST), &digest(1), at(NOW + 10_000)),
        Err(LinkError::SignedOut)
    ));
    make(&mut store, FIRST, 1).unwrap();
    store.end_session(&digest(FIRST)).unwrap();
    assert!(matches!(
        make(&mut store, FIRST, 2),
        Err(LinkError::SignedOut)
    ));
    assert!(matches!(
        store.revoke_subscription_link(&digest(FIRST), at(NOW)),
        Err(RevokeError::SignedOut)
    ));
    // The link made while signed in stays until its owner acts.
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Profile(cfm("pilotusera"))
    );
    assert_eq!(
        store.subscription_link_created(first.id).unwrap(),
        Some(at(NOW))
    );
}

#[test]
fn a_link_whose_user_has_no_profile_serves_nothing() {
    let (mut store, _, _) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    store
        .conn
        .execute(
            "DELETE FROM cfm_profiles WHERE mooring_user = 'pilotusera'",
            [],
        )
        .unwrap();
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::NoProfile
    );
}

#[test]
fn only_a_bound_account_with_a_profile_has_a_link_and_unbinding_removes_it() {
    let (mut store, first, second) = two_bound_accounts();
    let loose = register(&mut store, "c@qq.com", 3);
    store
        .start_session(&session(&loose, 103, NOW, 10_000))
        .unwrap();
    assert!(matches!(make(&mut store, 103, 9), Err(LinkError::Unbound)));
    // Bound, but no bundle has delivered a profile to the user yet.
    store
        .bind(&loose.code, &user("pilotuserc"), at(NOW))
        .unwrap();
    assert!(matches!(
        make(&mut store, 103, 9),
        Err(LinkError::NoProfile)
    ));
    assert_eq!(store.subscription_link_created(loose.id).unwrap(), None);
    make(&mut store, FIRST, 1).unwrap();
    make(&mut store, SECOND, 2).unwrap();
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
    let (mut store, _, _) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    assert!(matches!(
        make(&mut store, SECOND, 1),
        Err(LinkError::Store(StoreError::Sqlite(_)))
    ));
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Profile(cfm("pilotusera"))
    );
}

#[test]
fn changing_the_password_removes_the_link() {
    let (mut store, first, _) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    make(&mut store, SECOND, 2).unwrap();
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
    make(&mut store, FIRST, 1).unwrap();
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

#[test]
fn a_revoked_user_loses_the_link_for_good() {
    let (mut store, first, second) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    make(&mut store, SECOND, 2).unwrap();
    let report = store
        .import_profiles(
            &withholding(
                NOW + 10,
                &["pilotuserb"],
                &[("pilotusera", WithheldReason::Revoked)],
            ),
            at(NOW + 10),
        )
        .unwrap();
    assert_eq!(
        report.withheld,
        vec![Withheld {
            account: BoundAccount {
                account: first.code,
                user: user("pilotusera"),
            },
            reason: WithheldReason::Revoked,
            link: WithheldLink::Ended,
        }]
    );
    assert_eq!(store.subscription_link_created(first.id).unwrap(), None);
    // Delivering to the user again does not bring the old link back.
    let report = store
        .import_profiles(
            &bundle(NOW + 20, &["pilotusera", "pilotuserb"]),
            at(NOW + 20),
        )
        .unwrap();
    assert!(report.withheld.is_empty());
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Unknown
    );
    // A delivered user's link is untouched by either import.
    assert_eq!(
        store.linked_profile(&digest(2)).unwrap(),
        LinkedProfile::Profile(cfm("pilotuserb"))
    );
    assert_eq!(
        store.subscription_link_created(second.id).unwrap(),
        Some(at(NOW))
    );
    // Withheld without a link, the report says no link was ended.
    let report = store
        .import_profiles(
            &withholding(
                NOW + 30,
                &["pilotuserb"],
                &[("pilotusera", WithheldReason::Suspended)],
            ),
            at(NOW + 30),
        )
        .unwrap();
    assert_eq!(
        report.withheld,
        vec![Withheld {
            account: BoundAccount {
                account: first.code,
                user: user("pilotusera"),
            },
            reason: WithheldReason::Suspended,
            link: WithheldLink::NoLink,
        }]
    );
}

#[test]
fn a_user_withheld_for_a_while_keeps_a_paused_link() {
    let (mut store, first, _) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    make(&mut store, SECOND, 2).unwrap();
    for (generated, reason) in [
        (NOW + 10, WithheldReason::Suspended),
        (NOW + 20, WithheldReason::AgreementNotAccepted),
    ] {
        let report = store
            .import_profiles(
                &withholding(generated, &["pilotuserb"], &[("pilotusera", reason)]),
                at(generated),
            )
            .unwrap();
        assert_eq!(
            report.withheld,
            vec![Withheld {
                account: BoundAccount {
                    account: first.code,
                    user: user("pilotusera"),
                },
                reason,
                link: WithheldLink::Paused,
            }]
        );
        // The link serves nothing, the page says it is paused, and no new
        // link can be made meanwhile.
        assert_eq!(
            store.linked_profile(&digest(1)).unwrap(),
            LinkedProfile::NoProfile
        );
        assert_eq!(
            store.cfm_delivery(first.id).unwrap(),
            CfmDelivery::NoProfile {
                paused_link_created_at: Some(at(NOW)),
                paid_since_withheld: false,
            }
        );
        assert!(matches!(
            make(&mut store, FIRST, 3),
            Err(LinkError::NoProfile)
        ));
        // Delivered to again, the same link serves the profile.
        store
            .import_profiles(
                &bundle(generated + 5, &["pilotusera", "pilotuserb"]),
                at(generated + 5),
            )
            .unwrap();
        assert_eq!(
            store.linked_profile(&digest(1)).unwrap(),
            LinkedProfile::Profile(cfm("pilotusera"))
        );
        assert_eq!(
            store.subscription_link_created(first.id).unwrap(),
            Some(at(NOW))
        );
    }
    // Its owner can still revoke a paused link, and it stays gone.
    store
        .import_profiles(
            &withholding(
                NOW + 40,
                &["pilotuserb"],
                &[("pilotusera", WithheldReason::Suspended)],
            ),
            at(NOW + 40),
        )
        .unwrap();
    assert!(
        store
            .revoke_subscription_link(&digest(FIRST), at(NOW + 40))
            .unwrap()
    );
    store
        .import_profiles(
            &bundle(NOW + 50, &["pilotusera", "pilotuserb"]),
            at(NOW + 50),
        )
        .unwrap();
    assert_eq!(
        store.linked_profile(&digest(1)).unwrap(),
        LinkedProfile::Unknown
    );
    // The other account's link is untouched throughout.
    assert_eq!(
        store.linked_profile(&digest(2)).unwrap(),
        LinkedProfile::Profile(cfm("pilotuserb"))
    );
}

#[test]
fn a_bundle_that_leaves_a_linked_account_out_keeps_every_link() {
    let (mut store, first, second) = two_bound_accounts();
    make(&mut store, FIRST, 1).unwrap();
    make(&mut store, SECOND, 2).unwrap();
    // pilotuserb's link would end if the bundle were imported, but the bundle
    // leaves pilotusera out, so nothing happens.
    assert!(matches!(
        store.import_profiles(
            &withholding(NOW + 10, &[], &[("pilotuserb", WithheldReason::Revoked)]),
            at(NOW + 10)
        ),
        Err(ProfileImportError::LeavesOut { accounts })
            if accounts.len() == 1 && accounts[0].account == first.code
    ));
    for (byte, id) in [(1, "pilotusera"), (2, "pilotuserb")] {
        assert_eq!(
            store.linked_profile(&digest(byte)).unwrap(),
            LinkedProfile::Profile(cfm(id)),
            "{id}"
        );
    }
    for account in [&first, &second] {
        assert_eq!(
            store.subscription_link_created(account.id).unwrap(),
            Some(at(NOW))
        );
    }
}

#[test]
fn the_delivery_reads_the_profile_and_the_link_together() {
    let (mut store, first, _) = two_bound_accounts();
    let loose = register(&mut store, "c@qq.com", 3);
    assert_eq!(
        store.cfm_delivery(loose.id).unwrap(),
        CfmDelivery::NoProfile {
            paused_link_created_at: None,
            paid_since_withheld: false,
        }
    );
    assert_eq!(
        store.cfm_delivery(first.id).unwrap(),
        CfmDelivery::Profile {
            generated_at: at(NOW),
            link_created_at: None
        }
    );
    store
        .replace_subscription_link(&digest(FIRST), &digest(1), at(NOW + 3))
        .unwrap();
    assert_eq!(
        store.cfm_delivery(first.id).unwrap(),
        CfmDelivery::Profile {
            generated_at: at(NOW),
            link_created_at: Some(at(NOW + 3))
        }
    );
    store
        .conn
        .execute("UPDATE subscription_links SET created_at = -1", [])
        .unwrap();
    assert!(matches!(
        store.cfm_delivery(first.id),
        Err(StoreError::Corrupt {
            what: "link creation time",
            ..
        })
    ));
    // A paused link's time is read without a profile, and checked as well.
    store
        .conn
        .execute(
            "DELETE FROM cfm_profiles WHERE mooring_user = 'pilotusera'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.cfm_delivery(first.id),
        Err(StoreError::Corrupt {
            what: "link creation time",
            ..
        })
    ));
    // With the link sound again, a profile whose time is no time is
    // corrupt.
    store
        .import_profiles(&bundle(NOW, &["pilotusera", "pilotuserb"]), at(NOW))
        .unwrap();
    store
        .conn
        .execute("UPDATE subscription_links SET created_at = 1", [])
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE cfm_profiles SET generated_at = -5 WHERE mooring_user = 'pilotusera'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.cfm_delivery(first.id),
        Err(StoreError::Corrupt {
            what: "CFM profile time",
            ..
        })
    ));
}
