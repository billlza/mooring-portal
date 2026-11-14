//! Importing a bundle Mooring exported for one user, for the account bound
//! to that user.

use std::process::ExitCode;

use portal_core::auth::TokenDigest;
use portal_core::time::UnixTime;
use portal_store::{Account, LinkedProfile};

use super::{Fixture, profile_fixture};

/// 2026-11-06T09:00:00Z, when the pilot bundle was generated.
const GENERATED: u64 = 1_793_955_600;
/// 2026-11-06T10:00:00Z, when the one-user bundle was generated.
const ONE_USER: u64 = GENERATED + 3600;

fn at(secs: u64) -> UnixTime {
    UnixTime::from_secs(secs).unwrap()
}

/// Alice bound to pilotusera and Bob to pilotuserb, with the pilot
/// bundle's profiles, and Carol, unbound; the clock at the pilot bundle's
/// time.
fn three(fixture: &Fixture) -> [Account; 3] {
    let mut store = fixture.store();
    let accounts = [
        fixture.register(&mut store, "alice@qq.com", 1),
        fixture.register(&mut store, "bob@163.com", 2),
        fixture.register(&mut store, "carol@qq.com", 3),
    ];
    drop(store);
    for (account, user) in [(&accounts[0], "pilotusera"), (&accounts[1], "pilotuserb")] {
        let code = account.code.to_string();
        let bound = fixture.run(&["bind", "--account", &code, "--user", user]);
        assert_eq!(bound.code, ExitCode::SUCCESS, "{}", bound.err);
    }
    fixture.clock.set(at(GENERATED));
    let pilot = profile_fixture("valid/pilot.json");
    let imported = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(imported.code, ExitCode::SUCCESS, "{}", imported.err);
    accounts
}

/// When the bundle that `account`'s profile came in was generated, if it
/// has one.
fn profile_time(fixture: &Fixture, account: &Account) -> Option<UnixTime> {
    fixture
        .store()
        .cfm_profile(account.id)
        .unwrap()
        .map(|stored| stored.generated_at)
}

#[test]
fn a_bundle_for_one_account_stores_its_profile_and_no_other() {
    let fixture = Fixture::new();
    let [alice, bob, _] = three(&fixture);
    fixture.clock.set(at(ONE_USER));
    let file = profile_fixture("valid/one-user.json");
    let line = [
        "import-profile",
        "--account",
        "0000-0000-0001",
        file.to_str().unwrap(),
    ];
    let outcome = fixture.run(&line);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert_eq!(
        outcome.out,
        "profile bundle generated at 2026-11-06T10:00:00Z: stored the profile of account \
         0000-0000-0001 (Mooring user pilotusera); no other account changed\n\
         erased the deleted rows from the database files\n"
    );
    assert_eq!(profile_time(&fixture, &alice), Some(at(ONE_USER)));
    assert_eq!(profile_time(&fixture, &bob), Some(at(GENERATED)));
    // The same bundle again is no older than itself...
    let again = fixture.run(&line);
    assert_eq!(again.code, ExitCode::SUCCESS, "{}", again.err);
    assert_eq!(again.out, outcome.out);
    // ...but the pilot bundle is older now, and would bring Alice's old
    // profile back.
    let pilot = profile_fixture("valid/pilot.json");
    let older = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(older.code, ExitCode::FAILURE);
    assert_eq!(
        older.err,
        "mooring-portal: the last bundle imported was generated at 2026-11-06T10:00:00Z, after \
         this one (2026-11-06T09:00:00Z); import a newer bundle\n"
    );
}

#[test]
fn a_bundle_for_the_wrong_account_or_for_several_is_refused_and_changes_nothing() {
    let fixture = Fixture::new();
    let [alice, bob, _] = three(&fixture);
    let one_user = profile_fixture("valid/one-user.json");
    let pilot = profile_fixture("valid/pilot.json");
    let invalid = profile_fixture("invalid/full-sing-box-config.json");
    let refused = |account: &str, file: &std::path::Path| {
        let outcome = fixture.run(&[
            "import-profile",
            "--account",
            account,
            file.to_str().unwrap(),
        ]);
        assert_eq!(outcome.code, ExitCode::FAILURE, "{account}");
        assert!(outcome.out.is_empty(), "{}", outcome.out);
        outcome.err
    };
    // The clock is still at the pilot bundle's time: an hour short.
    assert_eq!(
        refused("0000-0000-0001", &one_user),
        "mooring-portal: the bundle was generated at 2026-11-06T10:00:00Z, more than 300 s \
         after this host's clock (2026-11-06T09:00:00Z); check both clocks\n"
    );
    fixture.clock.set(at(ONE_USER));
    for (account, file, said) in [
        (
            "0000-0000-0002",
            &one_user,
            "the bundle names Mooring user pilotusera, but account 0000-0000-0002 is bound to \
             Mooring user pilotuserb; nothing was imported",
        ),
        (
            "0000-0000-0003",
            &one_user,
            "account 0000-0000-0003 is not bound to a Mooring user; nothing was imported",
        ),
        (
            "0000-0000-0009",
            &one_user,
            "no account has the code 0000-0000-0009",
        ),
        (
            "0000-0000-0001",
            &pilot,
            "the bundle names 3 Mooring users; nothing was imported. Export it for the \
             account's user alone (`mooringctl user export-profiles --user <id>`), or import a \
             bundle that names every bound account's user with `import-profiles`",
        ),
    ] {
        assert_eq!(
            refused(account, file),
            format!("mooring-portal: {said}\n"),
            "{account}"
        );
    }
    let err = refused("0000-0000-0001", &invalid);
    assert!(
        err.contains(
            "full-sing-box-config.json: the document of user pilotuserb is refused: the \
             document is not a sing-box node list"
        ),
        "{err}"
    );
    // A lone document, as `mooringctl user export` writes it, is no bundle:
    // it never passed the agreement's gate in Mooring's bundle exporter.
    let lone = fixture.file(
        "export.json",
        b"{\"outbounds\": [{\"type\": \"vless\", \"tag\": \"lon1-reality\", \"uuid\": \
          \"00000000-0000-4000-8000-00000000000a\"}]}",
    );
    let err = refused("0000-0000-0001", &lone);
    assert!(
        err.contains(
            "export.json: the file is not a valid profile bundle: a key is missing or unknown"
        ),
        "{err}"
    );
    assert!(!err.contains("00000000-0000-4000-8000"), "{err}");
    // A missing or malformed account is a usage error.
    let file = one_user.to_str().unwrap();
    for line in [
        &["import-profile", file][..],
        &["import-profile", "--account", "alice", file],
        &["import-profile", "--account", "0000-0000-0001"],
    ] {
        assert_eq!(fixture.run(line).code, ExitCode::from(2), "{line:?}");
    }
    // Nothing changed, not even the last bundle's time.
    assert_eq!(profile_time(&fixture, &alice), Some(at(GENERATED)));
    assert_eq!(profile_time(&fixture, &bob), Some(at(GENERATED)));
    let imported = fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    assert_eq!(imported.code, ExitCode::SUCCESS, "{}", imported.err);
}

#[test]
fn a_bundle_withholding_the_accounts_user_says_what_became_of_its_link() {
    let fixture = Fixture::new();
    fixture.clock.set(at(GENERATED));
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    let alice = alice.to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    let pilot = profile_fixture("valid/pilot.json");
    fixture.run(&["import-profiles", pilot.to_str().unwrap()]);
    let link = TokenDigest::from_bytes([7; 32]);
    fixture
        .store()
        .replace_subscription_link(&session, &link, at(GENERATED))
        .unwrap();
    for (offset, reason, generated, became, served) in [
        (
            30,
            "suspended",
            "2026-11-06T09:00:30Z",
            ", and its subscription link is paused until Mooring delivers to the user again",
            LinkedProfile::NoProfile,
        ),
        (
            60,
            "revoked",
            "2026-11-06T09:01:00Z",
            ", and its subscription link has ended for good",
            LinkedProfile::Unknown,
        ),
    ] {
        let bundle = format!(
            "{{\"format\": \"mooring-portal-cfm-profiles\", \"version\": 2, \"generated_at\": \
             {}, \"profiles\": [], \"withheld\": [{{\"user\": \"pilotusera\", \"reason\": \
             \"{reason}\"}}]}}",
            GENERATED + offset
        );
        let file = fixture.file(&format!("{reason}.json"), bundle.as_bytes());
        fixture.clock.set(at(GENERATED + offset));
        let outcome = fixture.run(&[
            "import-profile",
            "--account",
            &alice,
            file.to_str().unwrap(),
        ]);
        assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
        assert_eq!(
            outcome.out,
            format!(
                "profile bundle generated at {generated}: stored no profile; no other account \
                 changed\n\
                 withheld by Mooring ({reason}): account 0000-0000-0001 (Mooring user \
                 pilotusera) has no profile now{became}\n\
                 erased the deleted rows from the database files\n"
            )
        );
        assert_eq!(fixture.store().linked_profile(&link).unwrap(), served);
    }
}
