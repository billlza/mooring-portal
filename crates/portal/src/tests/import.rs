//! Importing snapshot files.

use std::process::ExitCode;

use portal_core::snapshot::MAX_DOCUMENT_BYTES;
use portal_core::time::UnixTime;

use super::{Fixture, contract_fixture};

#[test]
fn the_pilot_snapshot_stores_bound_users_and_reports_the_rest() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    let bob = fixture.account("bob@163.com", 2).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    fixture.run(&["bind", "--account", &bob, "--user", "pilotuserz"]);
    let file = contract_fixture("valid/pilot.json");
    let outcome = fixture.run(&["import", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
    assert_eq!(
        outcome.out,
        "snapshot generated at 2026-11-05T12:00:00Z: stored 1 record(s): pilotusera\n\
         not kept: 6 user(s) no account is bound to\n\
         warning: account 0000-0000-0002 is bound to Mooring user pilotuserz, which this snapshot does not list\n"
    );
}

#[test]
fn a_snapshot_with_no_bound_user_stores_nothing() {
    let fixture = Fixture::new();
    let file = contract_fixture("valid/no-usage-report.json");
    let outcome = fixture.run(&["import", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::SUCCESS);
    assert_eq!(
        outcome.out,
        "snapshot generated at 2026-11-05T12:00:00Z: stored 0 record(s)\n\
         not kept: 2 user(s) no account is bound to\n"
    );
}

#[test]
fn unreadable_and_invalid_files_are_refused_with_their_name() {
    let fixture = Fixture::new();
    let missing = fixture.dir.path().join("absent.json");
    let outcome = fixture.run(&["import", missing.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.starts_with("mooring-portal: cannot read "),
        "{}",
        outcome.err
    );
    assert!(outcome.err.contains("absent.json"));

    let invalid = contract_fixture("invalid/unknown-key-alias.json");
    let outcome = fixture.run(&["import", invalid.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome
            .err
            .contains("unknown-key-alias.json: the document is not a valid snapshot")
    );
    assert!(
        outcome.err.contains("unknown field `alias`"),
        "{}",
        outcome.err
    );

    let huge = fixture.file("huge.json", &vec![b' '; MAX_DOCUMENT_BYTES + 10]);
    let outcome = fixture.run(&["import", huge.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome
            .err
            .contains("larger than the 1048576 bytes a snapshot may have"),
        "{}",
        outcome.err
    );
}

#[test]
fn snapshots_from_the_future_or_older_than_the_store_are_refused() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    let pilot = contract_fixture("valid/pilot.json");
    fixture
        .clock
        .set(UnixTime::from_secs(1_793_880_000 - 301).unwrap());
    let early = fixture.run(&["import", pilot.to_str().unwrap()]);
    assert_eq!(early.code, ExitCode::FAILURE);
    assert!(
        early
            .err
            .contains("more than 300 s after this host's clock"),
        "{}",
        early.err
    );
    fixture
        .clock
        .set(UnixTime::from_secs(1_793_880_000).unwrap());
    assert_eq!(
        fixture.run(&["import", pilot.to_str().unwrap()]).code,
        ExitCode::SUCCESS
    );
    let older = std::fs::read_to_string(&pilot)
        .unwrap()
        .replace(
            "\"generated_at\": 1793880000",
            "\"generated_at\": 1793879800",
        )
        .replace(
            "\"counted_through\": 1793879700",
            "\"counted_through\": 1793879600",
        );
    let file = fixture.file("older.json", older.as_bytes());
    let outcome = fixture.run(&["import", file.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome
            .err
            .contains("was generated at 2026-11-05T12:00:00Z, after this snapshot"),
        "{}",
        outcome.err
    );
}
