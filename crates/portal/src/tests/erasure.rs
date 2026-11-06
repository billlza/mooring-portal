//! What the operator's commands leave in the database files once they have
//! deleted or overwritten rows: nothing of what went, although the server
//! holds the database, and with it SQLite's write-ahead log, open
//! throughout.

use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use portal_core::account::Email;
use portal_core::auth::{BanDigest, TokenDigest};
use portal_core::time::UnixTime;
use portal_store::Store;

use super::{FailAfter, Fixture, contract_fixture, profile_fixture};

/// 2026-11-06T09:00:00Z, when the pilot bundle was generated.
const GENERATED: u64 = 1_793_955_600;

/// What the files hold of one account: its address and code, its Mooring
/// user, its profile's credential, and its session's and link's token
/// digests.
struct Traces {
    email: &'static str,
    code: &'static str,
    user: &'static str,
    uuid: &'static str,
    session: [u8; 32],
    link: [u8; 32],
}

const ALICE: Traces = Traces {
    email: "alice@qq.com",
    code: "0000-0000-0001",
    user: "pilotusera",
    uuid: "00000000-0000-4000-8000-00000000000a",
    session: [1; 32],
    link: [0xC3; 32],
};

const BOB: Traces = Traces {
    email: "bob@163.com",
    code: "0000-0000-0002",
    user: "pilotuserb",
    uuid: "00000000-0000-4000-8000-00000000000b",
    session: [2; 32],
    link: [0xD4; 32],
};

/// Every part of [`Traces`].
const ALL: &[&str] = &["email", "code", "user", "uuid", "session", "link"];

/// Alice and Bob, each signed in and bound, with a snapshot record, a CFM
/// profile and a subscription link; and the server's connection, which
/// stays open.
fn two_users() -> (Fixture, Store) {
    let fixture = Fixture::new();
    let mut server = fixture.store();
    for (traces, bits) in [(&ALICE, 1), (&BOB, 2)] {
        let (code, session) = fixture.signed_in(traces.email, bits);
        assert_eq!(code.to_string(), traces.code);
        assert_eq!(session.as_bytes(), &traces.session);
        let bound = fixture.run(&["bind", "--account", traces.code, "--user", traces.user]);
        assert_eq!(bound.code, ExitCode::SUCCESS, "{}", bound.err);
    }
    let generated = UnixTime::from_secs(GENERATED).unwrap();
    fixture.clock.set(generated);
    let snapshot = contract_fixture("valid/pilot.json");
    let bundle = profile_fixture("valid/pilot.json");
    for import in [
        ["import", snapshot.to_str().unwrap()],
        ["import-profiles", bundle.to_str().unwrap()],
    ] {
        let imported = fixture.run(&import);
        assert_eq!(imported.code, ExitCode::SUCCESS, "{}", imported.err);
    }
    for traces in [&ALICE, &BOB] {
        server
            .replace_subscription_link(
                &TokenDigest::from_bytes(traces.session),
                &TokenDigest::from_bytes(traces.link),
                generated,
            )
            .unwrap();
    }
    // The search finds every trace before anything goes.
    for traces in [&ALICE, &BOB] {
        assert_held(&fixture, traces, ALL);
    }
    (fixture, server)
}

/// Asserts that the files hold exactly the parts of `traces` named in
/// `kept`.
fn assert_held(fixture: &Fixture, traces: &Traces, kept: &[&str]) {
    let parts: [(&str, &[u8]); 6] = [
        ("email", traces.email.as_bytes()),
        ("code", traces.code.as_bytes()),
        ("user", traces.user.as_bytes()),
        ("uuid", traces.uuid.as_bytes()),
        ("session", &traces.session),
        ("link", &traces.link),
    ];
    for (name, bytes) in parts {
        assert_eq!(
            fixture.files_hold(bytes),
            kept.contains(&name),
            "{}: {name}",
            traces.email
        );
    }
}

#[test]
fn deleting_or_banning_an_account_leaves_nothing_of_it() {
    for command in ["delete-account", "ban"] {
        let (fixture, server) = two_users();
        let removed = fixture.run(&[command, "--account", ALICE.code]);
        assert_eq!(removed.code, ExitCode::SUCCESS, "{}", removed.err);
        if command == "ban" {
            assert!(
                removed.out.contains("(the link has ended for good)"),
                "{}",
                removed.out
            );
        }
        assert_held(&fixture, &ALICE, &[]);
        assert_held(&fixture, &BOB, ALL);
        drop(server);
    }
}

#[test]
fn unbinding_leaves_nothing_of_the_binding() {
    let (fixture, server) = two_users();
    let unbound = fixture.run(&["unbind", "--account", ALICE.code]);
    assert_eq!(unbound.code, ExitCode::SUCCESS, "{}", unbound.err);
    // The account stays, signed in; its user, profile and link go.
    assert_held(&fixture, &ALICE, &["email", "code", "session"]);
    assert_held(&fixture, &BOB, ALL);
    drop(server);
}

#[test]
fn a_profile_a_bundle_withholds_leaves_nothing() {
    let (fixture, server) = two_users();
    let revoked = format!(
        "{{\"format\": \"mooring-portal-cfm-profiles\", \"version\": 2, \"generated_at\": {}, \
         \"profiles\": [{{\"user\": \"{}\", \"document\": {{\"outbounds\": [{{\"type\": \
         \"vless\", \"tag\": \"lon1-reality\", \"uuid\": \"{}\"}}]}}}}], \"withheld\": \
         [{{\"user\": \"{}\", \"reason\": \"revoked\"}}]}}",
        GENERATED + 60,
        BOB.user,
        BOB.uuid,
        ALICE.user
    );
    fixture
        .clock
        .set(UnixTime::from_secs(GENERATED + 60).unwrap());
    let file = fixture.file("revoked.json", revoked.as_bytes());
    let imported = fixture.run(&["import-profiles", file.to_str().unwrap()]);
    assert_eq!(imported.code, ExitCode::SUCCESS, "{}", imported.err);
    // Alice stays bound, with no profile, and her link has ended.
    assert_held(&fixture, &ALICE, &["email", "code", "user", "session"]);
    assert_held(&fixture, &BOB, ALL);
    drop(server);
}

#[test]
fn a_newer_snapshot_leaves_nothing_of_the_record_it_replaces() {
    let (fixture, server) = two_users();
    let pilot = std::fs::read_to_string(contract_fixture("valid/pilot.json")).unwrap();
    // Alice's usage in the pilot snapshot, and in a later one: shorter, so
    // that SQLite cannot overwrite the record where it is.
    let (before, after) = ("12345678901", "2222222");
    assert!(pilot.contains(before));
    let newer = pilot
        .replace(
            "\"generated_at\": 1793880000",
            "\"generated_at\": 1793880060",
        )
        .replace(before, after);
    let file = fixture.file("newer.json", newer.as_bytes());
    let imported = fixture.run(&["import", file.to_str().unwrap()]);
    assert_eq!(imported.code, ExitCode::SUCCESS, "{}", imported.err);
    assert!(!fixture.files_hold(before.as_bytes()));
    assert!(fixture.files_hold(after.as_bytes()));
    drop(server);
}

#[test]
fn unbanning_leaves_no_digest_of_the_address() {
    let (fixture, server) = two_users();
    let digest = BanDigest::of(&Email::parse(ALICE.email).unwrap()).unwrap();
    let banned = fixture.run(&["ban", "--account", ALICE.code]);
    assert_eq!(banned.code, ExitCode::SUCCESS, "{}", banned.err);
    assert!(fixture.files_hold(digest.as_bytes()));
    let unbanned = fixture.run(&["unban", "--email", ALICE.email]);
    assert_eq!(unbanned.code, ExitCode::SUCCESS, "{}", unbanned.err);
    assert!(!fixture.files_hold(digest.as_bytes()));
    drop(server);
}

#[test]
fn erase_deleted_says_what_it_did() {
    let fixture = Fixture::new();
    let erased = fixture.run(&["erase-deleted"]);
    assert_eq!(erased.code, ExitCode::SUCCESS, "{}", erased.err);
    assert_eq!(
        erased.out,
        "erased the deleted rows from the database files\n"
    );
}

#[test]
fn output_that_fails_skips_no_erasure() {
    let (fixture, server) = two_users();
    let db = fixture.db();
    let mut err = Vec::new();
    let code = crate::run(
        [
            "mooring-portal",
            "--db",
            db.to_str().unwrap(),
            "delete-account",
            "--account",
            ALICE.code,
        ],
        &fixture.clock,
        &mut FailAfter(0),
        &mut err,
    );
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "mooring-portal: cannot write the output: pipe closed\n"
    );
    assert_held(&fixture, &ALICE, &[]);
    assert_held(&fixture, &BOB, ALL);
    drop(server);
}

/// A connection to the database on its own thread that begins a read and
/// keeps it open until told to end it; returns once the read has begun.
fn hold_a_read(fixture: &Fixture) -> (mpsc::Sender<()>, thread::JoinHandle<()>) {
    let path = fixture.db();
    let (began, begun) = mpsc::channel();
    let (end, ended) = mpsc::channel::<()>();
    let holder = thread::spawn(move || {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        let tx = conn.transaction().unwrap();
        let _: i64 = tx
            .query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))
            .unwrap();
        began.send(()).unwrap();
        ended.recv().unwrap();
        tx.commit().unwrap();
    });
    begun.recv().unwrap();
    (end, holder)
}

#[test]
fn a_command_that_cannot_erase_says_what_it_did_and_how_to_finish() {
    let (fixture, server) = two_users();
    // The read begins while the log holds the links' pages, so the log
    // cannot be emptied until it ends.
    let (end, reader) = hold_a_read(&fixture);
    let deleted = fixture.run(&["delete-account", "--account", ALICE.code]);
    end.send(()).unwrap();
    reader.join().unwrap();
    assert_eq!(deleted.code, ExitCode::FAILURE);
    assert_eq!(
        deleted.out,
        "deleted account 0000-0000-0001 with its sessions, binding, subscription record, CFM \
         profile and subscription link\n"
    );
    assert_eq!(
        deleted.err,
        "mooring-portal: another connection kept the database busy for 5 seconds, so copies of \
         deleted rows may still be in its files; run erase-deleted once the database is idle\n"
    );
    let erased = fixture.run(&["erase-deleted"]);
    assert_eq!(erased.code, ExitCode::SUCCESS, "{}", erased.err);
    assert_held(&fixture, &ALICE, &[]);
    assert_held(&fixture, &BOB, ALL);
    drop(server);
}
