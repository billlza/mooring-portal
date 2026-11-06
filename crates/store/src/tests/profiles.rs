use super::*;
use crate::{MAX_CLOCK_SKEW_SECS, ProfileImportError, ProfileImportReport, StoreError, Unlisted};

#[test]
fn only_bound_users_profiles_are_kept_and_the_rest_reported() {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    let unbound = register(&mut store, "c@qq.com", 3);
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&second.code, &user("pilotuserz"), at(NOW))
        .unwrap();
    let report = store
        .import_profiles(&bundle(NOW, &["pilotusera", "pilotuserb"]), at(NOW))
        .unwrap();
    assert_eq!(
        report,
        ProfileImportReport {
            stored: vec![user("pilotusera")],
            unbound: 1,
            missing: vec![Unlisted {
                account: second.code,
                user: user("pilotuserz"),
                link_revoked: false,
            }],
        }
    );
    assert_eq!(
        store.cfm_profile(first.id).unwrap(),
        Some(StoredProfile {
            profile: cfm("pilotusera"),
            generated_at: at(NOW),
        })
    );
    assert_eq!(store.cfm_profile(second.id).unwrap(), None);
    assert_eq!(store.cfm_profile(unbound.id).unwrap(), None);
    let kept: i64 = store
        .conn
        .query_row("SELECT count(*) FROM cfm_profiles", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kept, 1, "pilotuserb's profile is not held");
}

#[test]
fn a_bundle_replaces_every_profile_and_an_older_one_changes_nothing() {
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
    let newer = ProfileBundle::new(at(NOW + 20), [(user("pilotusera"), cfm("renewed"))]).unwrap();
    let report = store.import_profiles(&newer, at(NOW + 20)).unwrap();
    assert_eq!(
        report.missing,
        vec![Unlisted {
            account: second.code,
            user: user("pilotuserb"),
            link_revoked: false,
        }]
    );
    let renewed = Some(StoredProfile {
        profile: cfm("renewed"),
        generated_at: at(NOW + 20),
    });
    assert_eq!(store.cfm_profile(first.id).unwrap(), renewed);
    // A user the newer bundle leaves out is no longer served.
    assert_eq!(store.cfm_profile(second.id).unwrap(), None);
    assert!(matches!(
        store.import_profiles(&bundle(NOW + 10, &["pilotusera", "pilotuserb"]), at(NOW + 30)),
        Err(ProfileImportError::Older { stored, incoming })
            if stored == at(NOW + 20) && incoming == at(NOW + 10)
    ));
    assert_eq!(store.cfm_profile(first.id).unwrap(), renewed);
    assert_eq!(store.cfm_profile(second.id).unwrap(), None);
    // The same bundle again is no older than itself.
    store.import_profiles(&newer, at(NOW + 30)).unwrap();
    assert_eq!(store.cfm_profile(first.id).unwrap(), renewed);
}

#[test]
fn a_bundle_from_beyond_the_clock_skew_is_refused() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    let edge = NOW + MAX_CLOCK_SKEW_SECS;
    assert!(matches!(
        store.import_profiles(&bundle(edge + 1, &["pilotusera"]), at(NOW)),
        Err(ProfileImportError::FromTheFuture { generated_at, now })
            if generated_at == at(edge + 1) && now == at(NOW)
    ));
    assert_eq!(store.cfm_profile(account.id).unwrap(), None);
    store
        .import_profiles(&bundle(edge, &["pilotusera"]), at(NOW))
        .unwrap();
    assert!(store.cfm_profile(account.id).unwrap().is_some());
}

#[test]
fn unbinding_or_deleting_an_account_removes_its_profile() {
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
    store.unbind(&first.code).unwrap();
    store.delete_account(&second.code).unwrap();
    let kept: i64 = store
        .conn
        .query_row("SELECT count(*) FROM cfm_profiles", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kept, 0);
    // Binding again serves nothing until a bundle lists the user again.
    store
        .bind(&first.code, &user("pilotusera"), at(NOW + 1))
        .unwrap();
    assert_eq!(store.cfm_profile(first.id).unwrap(), None);
}

#[test]
fn a_stored_profile_that_no_longer_reads_is_corrupt() {
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
        .execute(
            "UPDATE cfm_profiles SET document = '{\"outbounds\": []}'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.cfm_profile(account.id),
        Err(StoreError::Corrupt {
            what: "CFM profile",
            ..
        })
    ));
    store
        .import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW))
        .unwrap();
    store
        .conn
        .execute("DELETE FROM profile_bundle", [])
        .unwrap();
    assert!(matches!(
        store.cfm_profile(account.id),
        Err(StoreError::Corrupt {
            what: "profile bundle time",
            ..
        })
    ));
    store
        .conn
        .execute(
            "INSERT INTO profile_bundle (id, generated_at) VALUES (1, -5)",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.import_profiles(&bundle(NOW, &["pilotusera"]), at(NOW)),
        Err(ProfileImportError::Store(StoreError::Corrupt {
            what: "profile bundle time",
            ..
        }))
    ));
}
