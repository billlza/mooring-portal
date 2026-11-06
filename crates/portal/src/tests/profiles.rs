//! Importing CFM profile bundles.

use std::process::ExitCode;

use portal_core::auth::TokenDigest;
use portal_core::profile::{MAX_BUNDLE_BYTES, ProfileBundle};
use portal_core::time::UnixTime;

use super::{Fixture, profile_fixture};

/// 2026-11-06T09:00:00Z, when the pilot bundle was generated.
const GENERATED: u64 = 1_793_955_600;

/// What importing the pilot bundle says when no account is bound to
/// pilotuserb.
const UNBOUND_PILOTUSERB: &str = "warning: no account is bound to pilotuserb: their profiles \
                                  were not kept, but their credentials passed through this \
                                  host. Delete the bundle file now, and leave these users out \
                                  of the next export\n";

#[test]
fn the_pilot_bundle_stores_bound_users_profiles_and_reports_the_rest() {
    let fixture = Fixture::new();
    fixture.clock.set(UnixTime::from_secs(GENERATED).unwrap());
    let alice = fixture.account("alice@qq.com", 1).to_string();
    let bob = fixture.account("bob@163.com", 2).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    fixture.run(&["bind", "--account", &bob, "--user", "pilotuserc"]);
    let file = profile_fixture("valid/pilot.json");
    let outcome = fixture.run(&["import-profiles", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert_eq!(
        outcome.out,
        format!(
            "profile bundle generated at 2026-11-06T09:00:00Z: stored 1 profile(s): pilotusera\n\
             withheld by Mooring (suspended): account 0000-0000-0002 (Mooring user pilotuserc) \
             has no profile now\n{UNBOUND_PILOTUSERB}erased the deleted rows from the database files\n"
        )
    );
    // The same bundle again is accepted and changes nothing.
    let again = fixture.run(&["import-profiles", file.to_str().unwrap()]);
    assert_eq!(again.code, ExitCode::SUCCESS, "{}", again.err);
    assert_eq!(again.out, outcome.out);
}

#[test]
fn invalid_bundles_are_refused_with_their_name_and_no_credential() {
    let fixture = Fixture::new();
    let invalid = profile_fixture("invalid/full-sing-box-config.json");
    let outcome = fixture.run(&["import-profiles", invalid.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.starts_with("mooring-portal: ")
            && outcome.err.contains(
                "full-sing-box-config.json: the document of user pilotuserb is refused: the \
                 document is not a sing-box node list"
            ),
        "{}",
        outcome.err
    );
    assert!(
        !outcome.err.contains("00000000-0000-4000-8000"),
        "{}",
        outcome.err
    );
    assert!(outcome.out.is_empty());

    let snapshot = super::contract_fixture("valid/pilot.json");
    let outcome = fixture.run(&["import-profiles", snapshot.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.contains(
            "pilot.json: the file's format is \"mooring-portal-snapshot\", not \
             \"mooring-portal-cfm-profiles\""
        ),
        "{}",
        outcome.err
    );

    let huge = fixture.file("huge.json", &vec![b' '; MAX_BUNDLE_BYTES + 10]);
    let outcome = fixture.run(&["import-profiles", huge.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome
            .err
            .contains("larger than the 1048576 bytes a profile bundle may have"),
        "{}",
        outcome.err
    );

    let missing = fixture.dir.path().join("absent.json");
    let outcome = fixture.run(&["import-profiles", missing.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.starts_with("mooring-portal: cannot read ")
            && outcome.err.contains("absent.json"),
        "{}",
        outcome.err
    );
}

#[test]
fn bundles_from_the_future_or_older_than_the_last_are_refused() {
    let fixture = Fixture::new();
    let pilot = profile_fixture("valid/pilot.json");
    fixture
        .clock
        .set(UnixTime::from_secs(GENERATED - 301).unwrap());
    let early = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(early.code, ExitCode::FAILURE);
    assert!(
        early
            .err
            .contains("more than 300 s after this host's clock"),
        "{}",
        early.err
    );
    fixture.clock.set(UnixTime::from_secs(GENERATED).unwrap());
    assert_eq!(
        fixture
            .run(&["import-profiles", pilot.to_str().unwrap()])
            .code,
        ExitCode::SUCCESS
    );
    let older = std::fs::read_to_string(&pilot).unwrap().replace(
        "\"generated_at\": 1793955600",
        "\"generated_at\": 1793955000",
    );
    let file = fixture.file("older.json", older.as_bytes());
    let outcome = fixture.run(&["import-profiles", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.contains(
            "the last bundle imported was generated at 2026-11-06T09:00:00Z, after this one \
             (2026-11-06T08:50:00Z)"
        ),
        "{}",
        outcome.err
    );
}

#[test]
fn a_bundle_that_withholds_a_linked_user_says_what_became_of_its_link() {
    let fixture = Fixture::new();
    fixture.clock.set(UnixTime::from_secs(GENERATED).unwrap());
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    let alice = alice.to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    let pilot = profile_fixture("valid/pilot.json");
    fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    let link = TokenDigest::from_bytes([7; 32]);
    let now = UnixTime::from_secs(GENERATED).unwrap();
    fixture
        .store()
        .replace_subscription_link(&session, &link, now)
        .unwrap();
    // A bundle that withholds pilotusera as suspended pauses the link.
    let paused = format!(
        "{{\"format\": \"mooring-portal-cfm-profiles\", \"version\": 2, \"generated_at\": {}, \
         \"profiles\": [], \"withheld\": [{{\"user\": \"pilotusera\", \"reason\": \
         \"suspended\"}}]}}",
        GENERATED + 30
    );
    let file = fixture.file("paused.json", paused.as_bytes());
    fixture
        .clock
        .set(UnixTime::from_secs(GENERATED + 30).unwrap());
    let outcome = fixture.run(&["import-profiles", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert_eq!(
        outcome.out,
        "profile bundle generated at 2026-11-06T09:00:30Z: stored 0 profile(s)\n\
         withheld by Mooring (suspended): account 0000-0000-0001 (Mooring user pilotusera) has \
         no profile now, and its subscription link is paused until Mooring delivers to the \
         user again\n\
         erased the deleted rows from the database files\n"
    );
    assert_eq!(
        fixture.store().linked_profile(&link).unwrap(),
        portal_store::LinkedProfile::NoProfile
    );
    // A later bundle that withholds pilotusera as revoked ends it.
    let later = format!(
        "{{\"format\": \"mooring-portal-cfm-profiles\", \"version\": 2, \"generated_at\": {}, \
         \"profiles\": [{{\"user\": \"pilotuserb\", \"document\": {{\"outbounds\": \
         [{{\"type\": \"vless\", \"tag\": \"lon1-reality\"}}]}}}}], \"withheld\": \
         [{{\"user\": \"pilotusera\", \"reason\": \"revoked\"}}]}}",
        GENERATED + 60
    );
    assert!(ProfileBundle::parse(later.as_bytes()).is_ok());
    let file = fixture.file("later.json", later.as_bytes());
    fixture
        .clock
        .set(UnixTime::from_secs(GENERATED + 60).unwrap());
    let outcome = fixture.run(&["import-profiles", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert_eq!(
        outcome.out,
        format!(
            "profile bundle generated at 2026-11-06T09:01:00Z: stored 0 profile(s)\n\
             withheld by Mooring (revoked): account 0000-0000-0001 (Mooring user pilotusera) \
             has no profile now, and its subscription link has ended for good\n\
             {UNBOUND_PILOTUSERB}erased the deleted rows from the database files\n"
        )
    );
    assert_eq!(
        fixture.store().linked_profile(&link).unwrap(),
        portal_store::LinkedProfile::Unknown
    );
}

#[test]
fn a_bundle_that_leaves_a_bound_account_out_is_refused_and_keeps_every_link() {
    let fixture = Fixture::new();
    fixture.clock.set(UnixTime::from_secs(GENERATED).unwrap());
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    let alice = alice.to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    let pilot = profile_fixture("valid/pilot.json");
    let first = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(
        first.out,
        format!(
            "profile bundle generated at 2026-11-06T09:00:00Z: stored 1 profile(s): pilotusera\n\
             {UNBOUND_PILOTUSERB}warning: no account is bound to pilotuserc, whom the bundle \
             withholds; leave these users out of the next export\nerased the deleted rows from the database files\n"
        )
    );
    let link = TokenDigest::from_bytes([7; 32]);
    let now = UnixTime::from_secs(GENERATED).unwrap();
    fixture
        .store()
        .replace_subscription_link(&session, &link, now)
        .unwrap();
    // Bob is bound to a user the pilot bundle does not name.
    let bob = fixture.account("bob@163.com", 2).to_string();
    fixture.run(&["bind", "--account", &bob, "--user", "pilotuserd"]);
    let outcome = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert_eq!(
        outcome.err,
        "mooring-portal: the bundle does not name the Mooring user of 0000-0000-0002 (Mooring \
         user pilotuserd); nothing was imported. Export the bundle again naming every bound \
         account's user. Unbind an account only if it is not to be served any more: unbinding \
         deletes its subscription link\n"
    );
    assert!(outcome.out.is_empty());
    assert!(matches!(
        fixture.store().linked_profile(&link).unwrap(),
        portal_store::LinkedProfile::Profile(_)
    ));
    // With Bob unbound, the same bundle imports, and Alice keeps her link.
    fixture.run(&["unbind", "--account", &bob]);
    let outcome = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert!(matches!(
        fixture.store().linked_profile(&link).unwrap(),
        portal_store::LinkedProfile::Profile(_)
    ));
}

#[test]
fn bound_users_names_every_bound_account_for_the_export_in_id_order() {
    let fixture = Fixture::new();
    let empty = fixture.run(&["bound-users"]);
    assert_eq!(empty.code, ExitCode::SUCCESS, "{}", empty.err);
    assert_eq!(empty.out, "\n");
    let alice = fixture.account("alice@qq.com", 1).to_string();
    let bob = fixture.account("bob@163.com", 2).to_string();
    fixture.account("carol@126.com", 3);
    fixture.run(&["bind", "--account", &alice, "--user", "pilotuserz"]);
    fixture.run(&["bind", "--account", &bob, "--user", "pilotuserb"]);
    let outcome = fixture.run(&["bound-users"]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert_eq!(outcome.out, "--user pilotuserb --user pilotuserz\n");
}
