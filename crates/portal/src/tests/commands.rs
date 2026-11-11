//! Listing, binding, unbinding and deleting accounts.

use std::process::ExitCode;

use portal_core::account::Email;
use portal_store::Subscription;

use super::{FailAfter, Fixture, contract_fixture, run_line};

#[test]
fn accounts_are_listed_with_their_bindings() {
    let fixture = Fixture::new();
    let empty = fixture.run(&["accounts"]);
    assert_eq!(empty.code, ExitCode::SUCCESS);
    assert_eq!(
        empty.out,
        "CODE            CREATED               MOORING USER  EMAIL\n0 account(s)\n"
    );
    let alice = fixture.account("alice@qq.com", 1);
    fixture.account("bob@163.com", 2);
    let bound = fixture.run(&[
        "bind",
        "--account",
        &alice.to_string(),
        "--user",
        "pilotusera",
    ]);
    assert_eq!(bound.code, ExitCode::SUCCESS, "{}", bound.err);
    assert_eq!(
        bound.out,
        "bound account 0000-0000-0001 to Mooring user pilotusera\n"
    );
    let listed = fixture.run(&["accounts"]);
    assert_eq!(
        listed.out,
        "CODE            CREATED               MOORING USER  EMAIL\n\
         0000-0000-0001  2026-11-05T12:00:00Z  pilotusera    alice@qq.com\n\
         0000-0000-0002  2026-11-05T12:00:00Z  -             bob@163.com\n\
         2 account(s)\n"
    );
}

#[test]
fn codes_may_be_typed_as_people_read_them() {
    let fixture = Fixture::new();
    fixture.account("alice@qq.com", 1);
    let outcome = fixture.run(&[
        "bind",
        "--account",
        "OOOO-OOOO-OOOI",
        "--user",
        "pilotusera",
    ]);
    assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
}

#[test]
fn binding_refusals_say_why() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    let bob = fixture.account("bob@163.com", 2).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    for (args, message) in [
        (
            vec![
                "bind",
                "--account",
                "0000-0000-0009",
                "--user",
                "pilotuserz",
            ],
            "mooring-portal: no account has the code 0000-0000-0009\n",
        ),
        (
            vec!["bind", "--account", &alice, "--user", "pilotuserb"],
            "mooring-portal: account 0000-0000-0001 is already bound to Mooring user pilotusera; unbind it first\n",
        ),
        (
            vec!["bind", "--account", &bob, "--user", "pilotusera"],
            "mooring-portal: Mooring user pilotusera is already bound to account 0000-0000-0001; unbind that account first\n",
        ),
        (
            vec!["unbind", "--account", &bob],
            "mooring-portal: account 0000-0000-0002 is not bound to a Mooring user\n",
        ),
        (
            vec!["delete-account", "--account", "0000-0000-0009"],
            "mooring-portal: no account has the code 0000-0000-0009\n",
        ),
    ] {
        let outcome = fixture.run(&args);
        assert_eq!(outcome.code, ExitCode::FAILURE, "{args:?}");
        assert_eq!(outcome.err, message, "{args:?}");
        assert!(outcome.out.is_empty());
    }
}

#[test]
fn unbinding_and_deleting_report_what_went() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    let unbound = fixture.run(&["unbind", "--account", &alice]);
    assert_eq!(unbound.code, ExitCode::SUCCESS);
    assert_eq!(
        unbound.out,
        "unbound account 0000-0000-0001 from Mooring user pilotusera; its subscription record, CFM profile and subscription link were deleted\n\
         erased the deleted rows from the database files\n"
    );
    let deleted = fixture.run(&["delete-account", "--account", &alice]);
    assert_eq!(deleted.code, ExitCode::SUCCESS);
    assert_eq!(
        deleted.out,
        "deleted account 0000-0000-0001 with its sessions, binding, subscription record, CFM profile, subscription link and orders\n\
         erased the deleted rows from the database files\n"
    );
    assert!(fixture.run(&["accounts"]).out.ends_with("0 account(s)\n"));
}

#[test]
fn commands_refuse_a_missing_database_and_create_none() {
    let fixture = Fixture::new();
    let typo = fixture.dir.path().join("portl.db");
    let typo_text = typo.to_str().unwrap();
    let pilot = contract_fixture("valid/pilot.json");
    let bundle = super::profile_fixture("valid/pilot.json");
    for args in [
        vec!["accounts"],
        vec![
            "bind",
            "--account",
            "0000-0000-0001",
            "--user",
            "pilotusera",
        ],
        vec!["unbind", "--account", "0000-0000-0001"],
        vec!["import", pilot.to_str().unwrap()],
        vec!["import-profiles", bundle.to_str().unwrap()],
        vec!["delete-account", "--account", "0000-0000-0001"],
    ] {
        let mut line = vec!["mooring-portal", "--db", typo_text];
        line.extend_from_slice(&args);
        let outcome = run_line(&line, &fixture.clock);
        assert_eq!(outcome.code, ExitCode::FAILURE, "{args:?}");
        assert_eq!(
            outcome.err,
            format!("mooring-portal: there is no database at {typo_text}\n"),
            "{args:?}"
        );
        assert!(outcome.out.is_empty());
        assert!(!typo.exists(), "{args:?}");
    }
}

#[test]
fn commands_take_effect_while_the_server_holds_the_database() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    // The server keeps one connection open from its start, and has read.
    let server = fixture.store();
    assert_eq!(server.accounts().unwrap()[0].binding, None);
    let bound = fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    assert_eq!(bound.code, ExitCode::SUCCESS, "{}", bound.err);
    let pilot = contract_fixture("valid/pilot.json");
    let imported = fixture.run(&["import", pilot.to_str().unwrap()]);
    assert_eq!(imported.code, ExitCode::SUCCESS, "{}", imported.err);
    let account = server
        .credentials(&Email::parse("alice@qq.com").unwrap())
        .unwrap()
        .unwrap()
        .account;
    assert!(matches!(
        server.subscription(account).unwrap(),
        Subscription::Bound {
            record: Some(_),
            ..
        }
    ));
}

#[test]
fn an_unusable_database_is_reported() {
    let fixture = Fixture::new();
    std::fs::write(fixture.db(), b"").unwrap();
    std::fs::set_permissions(
        fixture.db(),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    let outcome = fixture.run(&["accounts"]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(outcome.err.contains("has mode 644"), "{}", outcome.err);
}

#[test]
fn every_failing_line_of_output_is_an_error() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    let pilot = contract_fixture("valid/pilot.json");
    let pilot = pilot.to_str().unwrap();
    let db = fixture.db();
    let db = db.to_str().unwrap();
    let run = |args: &[&str], budget: usize| {
        let mut line = vec!["mooring-portal", "--db", db];
        line.extend_from_slice(args);
        let mut err = Vec::new();
        let code = crate::run(line, &fixture.clock, &mut FailAfter(budget), &mut err);
        (code, String::from_utf8(err).unwrap())
    };
    let lost = "mooring-portal: cannot write the output: pipe closed\n";
    // The header (58 bytes), the row (65) and the count (13) each fail.
    for budget in [0, 60, 125] {
        assert_eq!(
            run(&["accounts"], budget),
            (ExitCode::FAILURE, lost.to_owned()),
            "{budget}"
        );
    }
    // Each command takes effect; only its report is lost.
    assert_eq!(
        run(&["bind", "--account", &alice, "--user", "pilotuserz"], 0).1,
        lost
    );
    assert_eq!(run(&["unbind", "--account", &alice], 0).1, lost);
    assert_eq!(
        fixture
            .run(&["bind", "--account", &alice, "--user", "pilotusera"])
            .code,
        ExitCode::SUCCESS
    );
    // The first line of the report, then the second.
    for budget in [0, 90] {
        assert_eq!(run(&["import", pilot], budget).1, lost, "{budget}");
    }
    // The deletion's line, then the erasure's.
    let bob = fixture.account("bob@163.com", 2).to_string();
    let deletion = "deleted account 0000-0000-0002 with its sessions, binding, subscription \
                    record, CFM profile, subscription link and orders\n";
    for (account, budget) in [(&alice, 0), (&bob, deletion.len())] {
        assert_eq!(
            run(&["delete-account", "--account", account], budget).1,
            lost,
            "{budget}"
        );
    }
    assert!(fixture.run(&["accounts"]).out.ends_with("0 account(s)\n"));
    let help = crate::run(
        ["mooring-portal", "--help"],
        &fixture.clock,
        &mut FailAfter(0),
        &mut Vec::new(),
    );
    assert_eq!(
        help,
        ExitCode::FAILURE,
        "even the help text must be written"
    );
}

#[test]
fn a_failing_import_report_line_after_the_first_is_an_error() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotuserz"]);
    let pilot = contract_fixture("valid/pilot.json");
    let db = fixture.db();
    // The first line fits; the warning about pilotuserz does not.
    let mut err = Vec::new();
    let code = crate::run(
        [
            "mooring-portal",
            "--db",
            db.to_str().unwrap(),
            "import",
            pilot.to_str().unwrap(),
        ],
        &fixture.clock,
        &mut FailAfter(130),
        &mut err,
    );
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "mooring-portal: cannot write the output: pipe closed\n"
    );
}

#[test]
fn ban_deletes_the_account_and_unban_lets_its_address_register_again() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    let bob = fixture.account("bob@163.com", 2).to_string();
    fixture.run(&["bind", "--account", &alice, "--user", "pilotusera"]);
    let banned = fixture.run(&["ban", "--account", &alice]);
    assert_eq!(banned.code, ExitCode::SUCCESS, "{}", banned.err);
    assert_eq!(
        banned.out,
        "banned account 0000-0000-0001: deleted it with its sessions, binding, subscription \
         record, CFM profile, subscription link and orders; its email address cannot register \
         again\n\
         it was bound to Mooring user pilotusera: revoke that user in Mooring too, since the \
         nodes serve it until then\n\
         erased the deleted rows from the database files\n"
    );
    let unbound = fixture.run(&["ban", "--account", &bob]);
    assert_eq!(
        unbound.out,
        "banned account 0000-0000-0002: deleted it with its sessions, binding, subscription \
         record, CFM profile, subscription link and orders; its email address cannot register \
         again\n\
         erased the deleted rows from the database files\n"
    );
    assert!(fixture.run(&["accounts"]).out.ends_with("0 account(s)\n"));
    let again = fixture.run(&["ban", "--account", &alice]);
    assert_eq!(again.code, ExitCode::FAILURE);
    assert_eq!(
        again.err,
        "mooring-portal: no account has the code 0000-0000-0001\n"
    );
    let unbanned = fixture.run(&["unban", "--email", "Alice@QQ.com"]);
    assert_eq!(unbanned.code, ExitCode::SUCCESS, "{}", unbanned.err);
    assert_eq!(
        unbanned.out,
        "the email address can register again\nerased the deleted rows from the database files\n"
    );
    let missing = fixture.run(&["unban", "--email", "alice@qq.com"]);
    assert_eq!(missing.code, ExitCode::FAILURE);
    assert_eq!(
        missing.err,
        "mooring-portal: that email address is not banned; nothing changed\n"
    );
    assert!(missing.out.is_empty());
}

#[test]
fn a_ban_report_line_that_cannot_be_written_is_an_error() {
    let fixture = Fixture::new();
    let db = fixture.db();
    let banned = "banned account 0000-0000-0001: deleted it with its sessions, binding, \
                  subscription record, CFM profile, subscription link and orders; its email \
                  address cannot register again\n"
        .len();
    let bound = "it was bound to Mooring user pilotusera: revoke that user in Mooring too, \
                 since the nodes serve it until then\n"
        .len();
    // The first line, the reminder to revoke, then the erasure's line.
    for (bits, user, budget) in [
        (1, "pilotusera", 0),
        (2, "pilotuserb", banned),
        (3, "pilotuserc", banned + bound),
    ] {
        let account = fixture
            .account(&format!("user{bits}@qq.com"), bits)
            .to_string();
        fixture.run(&["bind", "--account", &account, "--user", user]);
        let mut err = Vec::new();
        let code = crate::run(
            [
                "mooring-portal",
                "--db",
                db.to_str().unwrap(),
                "ban",
                "--account",
                &account,
            ],
            &fixture.clock,
            &mut FailAfter(budget),
            &mut err,
        );
        assert_eq!(code, ExitCode::FAILURE, "{budget}");
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "mooring-portal: cannot write the output: pipe closed\n",
            "{budget}"
        );
    }
    // Each ban took effect; only its report was lost.
    assert!(fixture.run(&["accounts"]).out.ends_with("0 account(s)\n"));
}
