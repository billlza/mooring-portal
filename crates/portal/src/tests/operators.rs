//! Making an account an operator, which only the host does, and no longer
//! one.

use std::process::ExitCode;

use super::Fixture;

#[test]
fn operators_are_made_and_unmade_on_the_host() {
    let fixture = Fixture::new();
    let alice = fixture.account("alice@qq.com", 1).to_string();
    fixture.account("bob@163.com", 2);
    let granted = fixture.run(&["grant-operator", "--account", &alice]);
    assert_eq!(granted.code, ExitCode::SUCCESS, "{}", granted.err);
    assert_eq!(
        granted.out,
        "account 0000-0000-0001 can now use the operator pages at /operator, after confirming \
         its password there\n"
    );
    let again = fixture.run(&["grant-operator", "--account", &alice]);
    assert_eq!(again.code, ExitCode::SUCCESS, "{}", again.err);
    assert_eq!(
        again.out,
        "account 0000-0000-0001 could already use the operator pages; nothing changed\n"
    );
    assert_eq!(
        fixture.run(&["accounts"]).out,
        "CODE            CREATED               MOORING USER  EMAIL\n\
         0000-0000-0001  2026-11-05T12:00:00Z  -             alice@qq.com  (operator)\n\
         0000-0000-0002  2026-11-05T12:00:00Z  -             bob@163.com\n\
         2 account(s)\n"
    );
    let revoked = fixture.run(&["revoke-operator", "--account", &alice]);
    assert_eq!(revoked.code, ExitCode::SUCCESS, "{}", revoked.err);
    assert_eq!(
        revoked.out,
        "account 0000-0000-0001 can no longer use the operator pages\n\
         erased the deleted rows from the database files\n"
    );
    let not_one = fixture.run(&["revoke-operator", "--account", &alice]);
    assert_eq!(not_one.code, ExitCode::SUCCESS, "{}", not_one.err);
    assert_eq!(
        not_one.out,
        "account 0000-0000-0001 was not an operator; nothing changed\n"
    );
    assert!(!fixture.run(&["accounts"]).out.contains("(operator)"));
}

#[test]
fn only_an_account_is_made_an_operator() {
    let fixture = Fixture::new();
    for command in ["grant-operator", "revoke-operator"] {
        let outcome = fixture.run(&[command, "--account", "0000-0000-0009"]);
        assert_eq!(outcome.code, ExitCode::FAILURE, "{command}");
        assert_eq!(
            outcome.err, "mooring-portal: no account has the code 0000-0000-0009\n",
            "{command}"
        );
        assert!(outcome.out.is_empty());
    }
}
