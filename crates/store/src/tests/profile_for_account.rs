//! Importing a bundle Mooring exported for one user, for the account bound
//! to that user: it does to that account what a full import would, and
//! leaves every other account as it was.

use rusqlite::{Connection, params};

use super::*;
use crate::schema::MIGRATIONS;
use crate::{
    AccountId, AccountProfileError, AccountProfileImport, BoundAccount, BundleTimeError,
    CfmDelivery, MAX_CLOCK_SKEW_SECS, OpenMode, ProfileImportError, StoreError, UnknownAccount,
    Withheld, WithheldLink,
};

/// A bundle for `id` alone, delivering `profile`.
fn one(generated: u64, id: &str, profile: CfmProfile) -> ProfileBundle {
    ProfileBundle::new(
        at(generated),
        [(user(id), profile)],
        Vec::<(MooringUserId, WithheldReason)>::new(),
    )
    .unwrap()
}

/// The profile tagged `tag`, as stored from a bundle generated at
/// `generated`.
fn stored(tag: &str, generated: u64) -> Option<StoredProfile> {
    Some(StoredProfile {
        profile: cfm(tag),
        generated_at: at(generated),
    })
}

/// Two accounts bound to pilotusera and pilotuserb, each signed in (the
/// first with session 1, the second with 2) and each with a link, after a
/// full import at `NOW`; and a third account, unbound.
fn two_bound(store: &mut Store) -> (Account, Account, Account) {
    let first = register(store, "a@qq.com", 1);
    let second = register(store, "b@qq.com", 2);
    let third = register(store, "c@qq.com", 3);
    store.start_session(&session(&first, 1, NOW, 100)).unwrap();
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
        .replace_subscription_link(&digest(1), &digest(11), at(NOW))
        .unwrap();
    store
        .replace_subscription_link(&digest(2), &digest(12), at(NOW))
        .unwrap();
    (first, second, third)
}

/// What the second account's page shows: its profile from the full import
/// at `NOW`, and its link.
fn second_untouched(store: &Store, second: &Account) {
    assert_eq!(
        store.cfm_profile(second.id).unwrap(),
        stored("pilotuserb", NOW)
    );
    assert_eq!(
        store.cfm_delivery(second.id).unwrap(),
        CfmDelivery::Profile {
            generated_at: at(NOW),
            link_created_at: Some(at(NOW)),
        }
    );
}

#[test]
fn a_bundle_for_one_account_replaces_its_profile_alone() {
    let mut store = store();
    let (first, second, _) = two_bound(&mut store);
    assert_eq!(
        store
            .import_profile_for(
                &first.code,
                &one(NOW + 10, "pilotusera", cfm("renewed")),
                at(NOW + 15),
            )
            .unwrap(),
        AccountProfileImport::Stored(BoundAccount {
            account: first.code,
            user: user("pilotusera"),
        })
    );
    // The account has the new profile, with its bundle's time (not the
    // import's)...
    assert_eq!(
        store.cfm_profile(first.id).unwrap(),
        stored("renewed", NOW + 10)
    );
    assert_eq!(
        store.cfm_delivery(first.id).unwrap(),
        CfmDelivery::Profile {
            generated_at: at(NOW + 10),
            link_created_at: Some(at(NOW)),
        }
    );
    // ...and the other keeps its own, with the time it came with.
    second_untouched(&store, &second);
    // The bundle's time keeps imports in order: a full bundle made before
    // it would bring the old profile back, so it is refused...
    assert!(matches!(
        store.import_profiles(&bundle(NOW + 5, &["pilotusera", "pilotuserb"]), at(NOW + 20)),
        Err(ProfileImportError::Time(BundleTimeError::Older { stored, incoming }))
            if stored == at(NOW + 10) && incoming == at(NOW + 5)
    ));
    assert_eq!(
        store.cfm_profile(first.id).unwrap(),
        stored("renewed", NOW + 10)
    );
    // ...and a newer full bundle replaces both, as ever, with its time.
    store
        .import_profiles(
            &bundle(NOW + 20, &["pilotusera", "pilotuserb"]),
            at(NOW + 25),
        )
        .unwrap();
    assert_eq!(
        store.cfm_profile(first.id).unwrap(),
        stored("pilotusera", NOW + 20)
    );
    assert_eq!(
        store.cfm_profile(second.id).unwrap(),
        stored("pilotuserb", NOW + 20)
    );
}

#[test]
fn a_bundle_withholding_the_accounts_user_does_what_a_full_import_would() {
    let mut store = store();
    let (first, second, _) = two_bound(&mut store);
    // Withheld for a while: no profile, and the link waits.
    let paused = withholding(NOW + 10, &[], &[("pilotusera", WithheldReason::Suspended)]);
    assert_eq!(
        store
            .import_profile_for(&first.code, &paused, at(NOW + 10))
            .unwrap(),
        AccountProfileImport::Withheld(Withheld {
            account: BoundAccount {
                account: first.code,
                user: user("pilotusera"),
            },
            reason: WithheldReason::Suspended,
            link: WithheldLink::Paused,
        })
    );
    assert_eq!(store.cfm_profile(first.id).unwrap(), None);
    assert_eq!(
        store.cfm_delivery(first.id).unwrap(),
        CfmDelivery::NoProfile {
            paused_link_created_at: Some(at(NOW)),
        }
    );
    second_untouched(&store, &second);
    // Delivered again, the same link serves the new profile.
    store
        .import_profile_for(
            &first.code,
            &bundle(NOW + 20, &["pilotusera"]),
            at(NOW + 20),
        )
        .unwrap();
    assert_eq!(
        store.cfm_delivery(first.id).unwrap(),
        CfmDelivery::Profile {
            generated_at: at(NOW + 20),
            link_created_at: Some(at(NOW)),
        }
    );
    // Revoked: the link ends for good. An account without a link says so.
    let revoked = withholding(NOW + 30, &[], &[("pilotusera", WithheldReason::Revoked)]);
    let AccountProfileImport::Withheld(withheld) = store
        .import_profile_for(&first.code, &revoked, at(NOW + 30))
        .unwrap()
    else {
        panic!("withheld");
    };
    assert_eq!(withheld.link, WithheldLink::Ended);
    assert_eq!(store.subscription_link_created(first.id).unwrap(), None);
    assert_eq!(store.cfm_profile(first.id).unwrap(), None);
    let again = withholding(NOW + 40, &[], &[("pilotusera", WithheldReason::Revoked)]);
    let AccountProfileImport::Withheld(withheld) = store
        .import_profile_for(&first.code, &again, at(NOW + 40))
        .unwrap()
    else {
        panic!("withheld");
    };
    assert_eq!(withheld.link, WithheldLink::NoLink);
    second_untouched(&store, &second);
}

#[test]
fn a_bundle_for_another_account_or_for_several_changes_nothing() {
    let mut store = store();
    let (first, second, third) = two_bound(&mut store);
    let both = bundle(NOW + 10, &["pilotusera", "pilotuserb"]);
    let error = store
        .import_profile_for(&first.code, &both, at(NOW + 10))
        .unwrap_err();
    assert!(
        matches!(error, AccountProfileError::NamesMany { count: 2 }),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "the bundle names 2 Mooring users; nothing was imported. Export it for the account's \
         user alone (`mooringctl user export-profiles --user <id>`), or import a bundle that \
         names every bound account's user with `import-profiles`"
    );
    let theirs = bundle(NOW + 10, &["pilotuserb"]);
    let error = store
        .import_profile_for(&first.code, &theirs, at(NOW + 10))
        .unwrap_err();
    let AccountProfileError::OtherUser {
        account,
        bound,
        named,
    } = &error
    else {
        panic!("{error}");
    };
    assert_eq!(
        (account, bound, named),
        (&first.code, &user("pilotusera"), &user("pilotuserb"))
    );
    assert_eq!(
        error.to_string(),
        "the bundle names Mooring user pilotuserb, but account 0000-0000-0001 is bound to \
         Mooring user pilotusera; nothing was imported"
    );
    // A bundle withholding the other account's user is refused too, before
    // it could end this account's link.
    let revoked = withholding(NOW + 10, &[], &[("pilotuserb", WithheldReason::Revoked)]);
    assert!(matches!(
        store.import_profile_for(&first.code, &revoked, at(NOW + 10)),
        Err(AccountProfileError::OtherUser { .. })
    ));
    let unbound = store
        .import_profile_for(
            &third.code,
            &bundle(NOW + 10, &["pilotuserc"]),
            at(NOW + 10),
        )
        .unwrap_err();
    assert!(
        matches!(&unbound, AccountProfileError::NotBound(code) if *code == third.code),
        "{unbound}"
    );
    assert_eq!(
        unbound.to_string(),
        "account 0000-0000-0003 is not bound to a Mooring user; nothing was imported"
    );
    let nobody = AccountCode::from_bits(9);
    assert!(matches!(
        store.import_profile_for(&nobody, &bundle(NOW + 10, &["pilotusera"]), at(NOW + 10)),
        Err(AccountProfileError::UnknownAccount(UnknownAccount(code))) if code == nobody
    ));
    let edge = NOW + 10 + MAX_CLOCK_SKEW_SECS;
    assert!(matches!(
        store.import_profile_for(&first.code, &bundle(edge + 1, &["pilotusera"]), at(NOW + 10)),
        Err(AccountProfileError::Time(BundleTimeError::FromTheFuture { generated_at, now }))
            if generated_at == at(edge + 1) && now == at(NOW + 10)
    ));
    assert!(matches!(
        store.import_profile_for(&first.code, &bundle(NOW - 1, &["pilotusera"]), at(NOW + 10)),
        Err(AccountProfileError::Time(BundleTimeError::Older { stored, incoming }))
            if stored == at(NOW) && incoming == at(NOW - 1)
    ));
    // Nothing changed, not even the last bundle's time: a bundle older
    // than every refused one still imports.
    assert_eq!(
        store.cfm_profile(first.id).unwrap(),
        stored("pilotusera", NOW)
    );
    assert_eq!(
        store.cfm_delivery(first.id).unwrap(),
        CfmDelivery::Profile {
            generated_at: at(NOW),
            link_created_at: Some(at(NOW)),
        }
    );
    second_untouched(&store, &second);
    store
        .import_profile_for(&first.code, &bundle(NOW + 5, &["pilotusera"]), at(NOW + 10))
        .unwrap();
}

#[test]
fn an_import_for_one_account_that_fails_late_changes_nothing() {
    // The import replaces the profile, or ends a revoked user's link,
    // before it writes the bundle's time. With that last write made to
    // fail, the profile, the link and the time stay as they were.
    let mut store = store();
    let (first, second, _) = two_bound(&mut store);
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse_update BEFORE UPDATE ON profile_bundle \
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
    for bundle in [
        one(NOW + 10, "pilotusera", cfm("renewed")),
        withholding(NOW + 10, &[], &[("pilotusera", WithheldReason::Revoked)]),
    ] {
        assert!(matches!(
            store.import_profile_for(&first.code, &bundle, at(NOW + 10)),
            Err(AccountProfileError::Store(StoreError::Sqlite(_)))
        ));
        assert_eq!(
            store.cfm_delivery(first.id).unwrap(),
            CfmDelivery::Profile {
                generated_at: at(NOW),
                link_created_at: Some(at(NOW)),
            }
        );
        assert_eq!(
            store.cfm_profile(first.id).unwrap(),
            stored("pilotusera", NOW)
        );
    }
    second_untouched(&store, &second);
}

/// A database at schema version 6 holding pilotusera's profile for account
/// 0000-0000-0001, and, if `bundle_time` is given, the last bundle's time.
fn version_6(path: &std::path::Path, bundle_time: Option<u64>) {
    let conn = Connection::open(path).unwrap();
    for sql in &MIGRATIONS[..6] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", 6).unwrap();
    conn.execute(
        "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
         VALUES ('0000-0000-0001', 'a@qq.com', ?1, ?2, ?2)",
        params![hash("first").as_str(), NOW.cast_signed()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO bindings (account_id, mooring_user, bound_at)
         VALUES (1, 'pilotusera', ?1)",
        [NOW.cast_signed()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cfm_profiles (mooring_user, document) VALUES ('pilotusera', ?1)",
        [cfm("pilotusera").as_str()],
    )
    .unwrap();
    if let Some(generated) = bundle_time {
        conn.execute(
            "INSERT INTO profile_bundle (id, generated_at) VALUES (1, ?1)",
            [generated.cast_signed()],
        )
        .unwrap();
    }
}

/// The schema version of the database at `path`.
fn schema_version(path: &std::path::Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn a_version_6_database_keeps_its_profiles_with_their_bundles_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    version_6(&path, Some(NOW - 60));
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    assert_eq!(
        store.cfm_profile(AccountId(1)).unwrap(),
        stored("pilotusera", NOW - 60)
    );
    // The last bundle's time still keeps imports in order.
    assert!(matches!(
        store.import_profile_for(
            &AccountCode::from_bits(1),
            &bundle(NOW - 61, &["pilotusera"]),
            at(NOW),
        ),
        Err(AccountProfileError::Time(BundleTimeError::Older { .. }))
    ));
    drop(store);
    assert_eq!(
        usize::try_from(schema_version(&path)).unwrap(),
        MIGRATIONS.len()
    );
}

#[test]
fn a_version_6_database_with_a_profile_but_no_bundle_time_is_not_opened() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    version_6(&path, None);
    // The copy leaves the profile's time empty, which the table refuses.
    assert!(matches!(
        Store::open(&path, OpenMode::Existing),
        Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(failure, _)))
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL
    ));
    // The migration rolled back whole: the profile is still there, under
    // version 6, for the operator to look into.
    assert_eq!(schema_version(&path), 6);
    let kept: String = Connection::open(&path)
        .unwrap()
        .query_row("SELECT document FROM cfm_profiles", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kept, cfm("pilotusera").as_str());
}
