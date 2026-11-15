//! Help, versions and malformed command lines.

use std::process::ExitCode;

use portal_core::time::{ManualClock, UnixTime};

use super::{Fixture, NOW, run_line};
use crate::CommandError;
use crate::cli::ServeArgs;
use crate::serve::config;

fn clock() -> ManualClock {
    ManualClock::new(UnixTime::from_secs(NOW).unwrap())
}

#[test]
fn help_and_version_answer_on_stdout() {
    let help = run_line(&["mooring-portal", "--help"], &clock());
    assert_eq!(help.code, ExitCode::SUCCESS);
    for command in [
        "serve",
        "accounts",
        "bind",
        "unbind",
        "import",
        "delete-account",
    ] {
        assert!(help.out.contains(command), "{command}");
    }
    assert!(help.err.is_empty());
    let version = run_line(&["mooring-portal", "--version"], &clock());
    assert_eq!(version.code, ExitCode::SUCCESS);
    assert_eq!(
        version.out,
        format!("mooring-portal {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn malformed_lines_are_usage_errors() {
    for line in [
        &["mooring-portal"][..],
        &["mooring-portal", "accounts"],
        &["mooring-portal", "--db", "x.db"],
        &["mooring-portal", "--db", "x.db", "frobnicate"],
        &[
            "mooring-portal",
            "--db",
            "x.db",
            "bind",
            "--account",
            "0000-0000-0001",
        ],
        &[
            "mooring-portal",
            "--db",
            "x.db",
            "bind",
            "--account",
            "nope",
            "--user",
            "pilotusera",
        ],
        &[
            "mooring-portal",
            "--db",
            "x.db",
            "bind",
            "--account",
            "0000-0000-0001",
            "--user",
            "Pilot-User",
        ],
        &[
            "mooring-portal",
            "--db",
            "x.db",
            "serve",
            "--origin",
            "https://p.example",
            "--listen",
            "nowhere",
        ],
    ] {
        let outcome = run_line(line, &clock());
        assert_eq!(outcome.code, ExitCode::from(2), "{line:?}");
        assert!(outcome.out.is_empty(), "{line:?}");
        assert!(
            outcome.err.contains("Usage:") || outcome.err.contains("error:"),
            "{line:?}: {}",
            outcome.err
        );
    }
    let named = run_line(
        &[
            "mooring-portal",
            "--db",
            "x.db",
            "bind",
            "--account",
            "nope",
            "--user",
            "pilotusera",
        ],
        &clock(),
    );
    assert!(
        named.err.contains("is not an account code"),
        "{}",
        named.err
    );
}

#[test]
fn serve_refuses_an_unusable_origin_before_starting() {
    let fixture = Fixture::new();
    let fresh = fixture.dir.path().join("fresh.db");
    let outcome = run_line(
        &[
            "mooring-portal",
            "--db",
            fresh.to_str().unwrap(),
            "serve",
            "--origin",
            "http://portal.example",
        ],
        &fixture.clock,
    );
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert_eq!(
        outcome.err,
        "mooring-portal: \"http://portal.example\" uses http for a host other than localhost; use https\n"
    );
    assert!(!fresh.exists(), "nothing was created");
}

#[test]
fn serve_options_become_the_web_configuration() {
    let args = |behind_proxy| ServeArgs {
        listen: "127.0.0.1:8080".parse().unwrap(),
        origin: "https://Portal.Example".to_owned(),
        behind_proxy,
        max_accounts: 7,
        payments: None,
        stripe_webhook_secret: None,
        new_message_signal: None,
    };
    let direct = config(&args(false)).unwrap();
    assert_eq!(direct.origin.as_str(), "https://portal.example");
    assert_eq!(direct.client_ip, portal_web::ClientIpSource::Peer);
    assert_eq!(direct.max_accounts, 7);
    assert!(direct.payments.is_none());
    let proxied = config(&args(true)).unwrap();
    assert_eq!(proxied.client_ip, portal_web::ClientIpSource::ForwardedFor);
    assert!(direct.new_message_signal.is_none());
    let signalling = config(&ServeArgs {
        new_message_signal: Some("/var/lib/mooring-portal/new-message".into()),
        ..args(true)
    })
    .unwrap();
    assert_eq!(
        signalling.new_message_signal.unwrap().path(),
        std::path::Path::new("/var/lib/mooring-portal/new-message")
    );
}

#[test]
fn serve_refuses_a_signal_file_it_cannot_write_before_starting() {
    let fixture = Fixture::new();
    let fresh = fixture.dir.path().join("fresh.db");
    let gone = fixture.dir.path().join("gone").join("new-message");
    for (signal, said) in [
        (
            "new-message".to_owned(),
            "mooring-portal: new-message is not an absolute path to a file: give the signal \
             file as /directory/name\n"
                .to_owned(),
        ),
        (
            gone.to_str().unwrap().to_owned(),
            format!(
                "mooring-portal: cannot use {} as the new-message signal file: No such file or \
                 directory (os error 2)\n",
                gone.display()
            ),
        ),
    ] {
        let outcome = run_line(
            &[
                "mooring-portal",
                "--db",
                fresh.to_str().unwrap(),
                "serve",
                "--origin",
                "http://localhost:8080",
                "--new-message-signal",
                &signal,
            ],
            &fixture.clock,
        );
        assert_eq!(outcome.code, ExitCode::FAILURE, "{signal}");
        assert_eq!(outcome.err, said, "{signal}");
        assert!(!fresh.exists(), "nothing was created");
    }
}

#[test]
fn error_messages_name_their_cause() {
    for (error, message) in [
        (
            CommandError::Read {
                path: "snapshot.json".into(),
                source: std::io::Error::other("disk on fire"),
            },
            "cannot read snapshot.json: disk on fire",
        ),
        (
            CommandError::Server(std::io::Error::other("address in use")),
            "the server failed: address in use",
        ),
        (
            CommandError::Logging("already set".to_owned()),
            "cannot set up logging: already set",
        ),
    ] {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn serve_reports_an_address_in_use() {
    let fixture = Fixture::new();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = taken.local_addr().unwrap().to_string();
    let outcome = fixture.run(&[
        "serve",
        "--listen",
        &address,
        "--origin",
        "http://localhost:8080",
    ]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome
            .err
            .starts_with("mooring-portal: the server failed: "),
        "{}",
        outcome.err
    );
    assert!(
        fixture.db().exists(),
        "the database was opened before binding"
    );
}
