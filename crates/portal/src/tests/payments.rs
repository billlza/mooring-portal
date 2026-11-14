//! The operator's side of payments: what `serve` reads, Mooring's token,
//! the list of what waits, and the decisions on it.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::ExitCode;

use portal_core::auth::{SecretToken, TokenDigest};
use portal_core::payment::stripe::{EventKind, Refunded};
use portal_store::Progress;

use super::paying::*;
use super::{Fixture, run_line};
use crate::CommandError;
use crate::cli::ServeArgs;
use crate::serve::config;

fn serve_args(payments: Option<PathBuf>, secrets: Option<PathBuf>) -> ServeArgs {
    ServeArgs {
        listen: "127.0.0.1:8080".parse().unwrap(),
        origin: "https://portal.example".to_owned(),
        behind_proxy: false,
        max_accounts: 7,
        payments,
        stripe_webhook_secret: secrets,
    }
}

#[test]
fn serve_takes_the_catalogue_and_the_signing_secrets_together() {
    let fixture = Fixture::new();
    let db = fixture.db();
    let secret = fixture.file_with_mode("whsec", SECRET.as_bytes(), 0o600);
    let catalogue = demo();
    for extra in [
        vec!["--payments", catalogue.to_str().unwrap()],
        vec!["--stripe-webhook-secret", secret.to_str().unwrap()],
    ] {
        let mut line = vec![
            "mooring-portal",
            "--db",
            db.to_str().unwrap(),
            "serve",
            "--origin",
            "http://localhost:8080",
        ];
        line.extend(extra);
        let outcome = run_line(&line, &fixture.clock);
        assert_eq!(outcome.code, ExitCode::from(2), "{line:?}");
        assert!(outcome.err.contains("required"), "{}", outcome.err);
    }
    // Both: the portal sells, under one secret or two.
    let sells = config(&serve_args(Some(fixture.catalogue()), Some(secret))).unwrap();
    assert!(sells.payments.is_some());
    let rolled = fixture.file_with_mode(
        "rolled",
        format!("\n{SECRET}\n\n  {OTHER_SECRET}  \n").as_bytes(),
        0o400,
    );
    assert!(
        config(&serve_args(Some(fixture.catalogue()), Some(rolled)))
            .unwrap()
            .payments
            .is_some()
    );
    // A secret is in no printed form of the configuration.
    assert!(!format!("{sells:?}").contains("whsec_"));
}

#[test]
fn serve_refuses_a_catalogue_or_secrets_it_cannot_use_and_repeats_no_secret() {
    let fixture = Fixture::new();
    let good = fixture.file_with_mode("whsec", SECRET.as_bytes(), 0o600);
    let refused = |payments: PathBuf, secrets: PathBuf| -> String {
        config(&serve_args(Some(payments), Some(secrets)))
            .unwrap_err()
            .to_string()
    };
    // The catalogue.
    let missing = fixture.dir.path().join("nowhere.json");
    assert!(
        refused(missing, good.clone()).starts_with("cannot read "),
        "a catalogue that is not there"
    );
    let not_one = fixture.file("not.json", b"{\"format\": \"something-else\"}");
    assert!(refused(not_one, good.clone()).ends_with(
        "not.json: the catalogue does not parse: a key is missing or unknown, or a value is of \
         the wrong type or invalid"
    ));
    let not_text = fixture.file("binary.json", b"\xff\xfe{}");
    assert!(
        refused(not_text, good.clone())
            .ends_with("binary.json: the catalogue does not parse: it is not JSON")
    );
    let large = fixture.file("large.json", &vec![b' '; 64 * 1024 + 1]);
    assert!(refused(large, good).ends_with("large.json: the catalogue is larger than 65536 bytes"));
    // The secrets.
    let demo = || fixture.catalogue();
    let nowhere = fixture.dir.path().join("nowhere");
    assert!(refused(demo(), nowhere).starts_with("cannot read "));
    for mode in [0o644, 0o640, 0o604, 0o660] {
        let open = fixture.file_with_mode("open", SECRET.as_bytes(), mode);
        let said = refused(demo(), open);
        assert!(
            said.starts_with("the signing secret file ")
                && said.ends_with(&format!(
                    "has mode {mode:o}, so other users may read it; restrict it with `chmod 600`"
                )),
            "{said}"
        );
    }
    for (name, contents, said) in [
        (
            "none",
            "\n\n",
            "give one signing secret, or two while one is being rolled, not 0",
        ),
        (
            "three",
            "whsec_aaaaaaaaaaaaaaaaaaaa\nwhsec_bbbbbbbbbbbbbbbbbbbb\nwhsec_cccccccccccccccccccc\n",
            "give one signing secret, or two while one is being rolled, not 3",
        ),
        (
            "wrong",
            "whsec_aaaaaaaaaaaaaaaaaaaa\n\nsk_live_not_this_one_please\n",
            "line 3: not a signing secret: one begins with whsec_ and is letters, digits and _ \
             after it",
        ),
    ] {
        let file = fixture.file_with_mode(name, contents.as_bytes(), 0o600);
        let told = refused(demo(), file);
        assert!(told.ends_with(said), "{told}");
        // What the file holds is not repeated.
        assert!(
            !told.contains("sk_live") && !told.contains("whsec_a"),
            "{told}"
        );
    }
    let binary = fixture.file_with_mode("binary", b"whsec_\xff\xfe", 0o600);
    assert!(refused(demo(), binary).contains("is not text"));
    // Something that opens but cannot be read: a directory.
    let directory = fixture.dir.path().join("secrets.d");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(refused(demo(), directory).starts_with("cannot read "));
}

#[test]
fn serve_reads_a_catalogue_that_only_its_owner_can_change() {
    let fixture = Fixture::new();
    let secret = fixture.file_with_mode("whsec", SECRET.as_bytes(), 0o600);
    let written = std::fs::read(demo()).unwrap();
    let with_mode = |mode: u32| {
        let catalogue = fixture.file_with_mode(&format!("sold-{mode:o}.json"), &written, mode);
        config(&serve_args(Some(catalogue), Some(secret.clone())))
    };
    // Whoever writes it decides what is sold, and on whose payment pages.
    for mode in [0o664, 0o646, 0o666, 0o620, 0o602] {
        let said = with_mode(mode).unwrap_err().to_string();
        assert!(
            said.starts_with("the catalogue file ")
                && said.ends_with(&format!(
                    "sold-{mode:o}.json has mode {mode:o}, so other users may change what is sold and \
                     where it is paid for; restrict it with `chmod go-w`"
                )),
            "{said}"
        );
    }
    // Reading it gives nothing away: it is what the plans page shows.
    for mode in [0o644, 0o640, 0o600, 0o444, 0o755] {
        assert!(with_mode(mode).unwrap().payments.is_some(), "{mode:o}");
    }
}

#[test]
fn a_file_of_signing_secrets_is_read_whole_or_not_at_all() {
    let fixture = Fixture::new();
    // Two secrets and blank lines, to a length in bytes.
    let padded = |length: usize| {
        let mut contents = format!("{SECRET}\n{OTHER_SECRET}\n").into_bytes();
        contents.resize(length, b'\n');
        let file = fixture.file_with_mode("padded", &contents, 0o600);
        config(&serve_args(Some(fixture.catalogue()), Some(file)))
    };
    assert!(padded(4_096).unwrap().payments.is_some());
    let said = padded(4_097).unwrap_err().to_string();
    assert!(
        said.starts_with("the signing secret file ")
            && said.ends_with(
                "padded is larger than 4096 bytes; it holds one secret, or two while one is \
                 being rolled, a line each"
            ),
        "{said}"
    );
    // A secret that the limit would cut short is not read as a shorter
    // one: nothing of the file is used.
    let mut cut = vec![b'\n'; 4_090];
    cut.extend_from_slice(SECRET.as_bytes());
    let file = fixture.file_with_mode("cut", &cut, 0o600);
    let said = config(&serve_args(Some(fixture.catalogue()), Some(file)))
        .unwrap_err()
        .to_string();
    assert!(said.ends_with("cut is larger than 4096 bytes; it holds one secret, or two while one is being rolled, a line each"), "{said}");
    assert!(!said.contains("whsec_"), "{said}");
}

#[test]
fn moorings_token_is_made_shown_once_and_revoked() {
    let fixture = Fixture::new();
    let made = fixture.run(&["pull-token"]);
    assert_eq!(made.code, ExitCode::SUCCESS, "{}", made.err);
    let lines: Vec<&str> = made.out.lines().collect();
    assert_eq!(
        lines[0],
        "Mooring's pull token, shown only this once (any token made before no longer works):"
    );
    let token = SecretToken::parse(lines[1]).unwrap();
    assert_eq!(lines.len(), 2);
    // The database holds its digest, not the token.
    assert!(fixture.store().is_pull_token(&token.digest()).unwrap());
    assert!(!fixture.files_hold(lines[1].as_bytes()));
    // A second one takes its place.
    let again = fixture.run(&["pull-token"]);
    let second = SecretToken::parse(again.out.lines().nth(1).unwrap()).unwrap();
    assert!(!second.matches(&token));
    assert!(!fixture.store().is_pull_token(&token.digest()).unwrap());
    assert!(fixture.store().is_pull_token(&second.digest()).unwrap());
    let revoked = fixture.run(&["revoke-pull-token"]);
    assert_eq!(
        revoked.out,
        "revoked Mooring's pull token; Mooring cannot pull until a new one is made\n"
    );
    assert!(!fixture.store().is_pull_token(&second.digest()).unwrap());
    let nothing = fixture.run(&["revoke-pull-token"]);
    assert_eq!(nothing.code, ExitCode::FAILURE);
    assert_eq!(
        nothing.err,
        "mooring-portal: there is no pull token; nothing changed\n"
    );
}

#[test]
fn a_stray_payment_is_taken_off_the_list_by_its_number() {
    let fixture = busy();
    let dismissed = fixture.run(&["dismiss-payment", "--id", "1"]);
    assert_eq!(dismissed.code, ExitCode::SUCCESS, "{}", dismissed.err);
    assert_eq!(
        dismissed.out,
        "took payment #1 off the list\nerased the deleted rows from the database files\n"
    );
    assert!(
        fixture
            .run(&["orders"])
            .out
            .contains("0 payment(s) that are no order's")
    );
    for id in ["1", "7"] {
        let nothing = fixture.run(&["dismiss-payment", "--id", id]);
        assert_eq!(nothing.code, ExitCode::FAILURE);
        assert_eq!(
            nothing.err,
            format!(
                "mooring-portal: there is no payment numbered {id} on the list; nothing changed\n"
            )
        );
    }
    assert_eq!(
        fixture.run(&["dismiss-payment", "--id", "one"]).code,
        ExitCode::from(2)
    );
}

#[test]
fn a_ban_keeps_the_cards_it_is_given_and_names_the_subscriptions_to_cancel() {
    let fixture = busy();
    let (first, second) = (fingerprint(2), fingerprint(3));
    let banned = fixture.run(&[
        "ban",
        "--account",
        "0000-0000-0002",
        "--card",
        &first,
        "--card",
        &second,
    ]);
    assert_eq!(banned.code, ExitCode::SUCCESS, "{}", banned.err);
    assert_eq!(
        banned.out,
        "banned account 0000-0000-0002: deleted it with its sessions, binding, subscription \
         record, CFM profile, subscription link, orders, Mac bookings and feedback; its email \
         address cannot register again\n\
         kept 2 card fingerprint(s) with the ban: payments made with them will not be granted\n\
         Stripe may still charge subscription sub_5: cancel it at Stripe, and delete the card \
         saved with it\n\
         it was bound to Mooring user pilotuserb: revoke that user in Mooring too, since the \
         nodes serve it until then\n\
         erased the deleted rows from the database files\n"
    );
    // Neither fingerprint is in the database files, only their digests.
    assert!(!fixture.files_hold(first.as_bytes()));
    assert!(!fixture.files_hold(second.as_bytes()));
    // The other account pays with one of the cards: not granted.
    let alice = TokenDigest::from_bytes([1; 32]);
    let order = fixture.order(&alice, "month-100", 8);
    fixture.told(50, vec![paid(8, order, "month-100"), charged(8, 3)]);
    let listed = fixture.run(&["orders"]).out;
    assert!(
        listed.contains("waits for you: it was paid with a card of a banned account"),
        "{listed}"
    );
    // A card found later is added to the ban; one more payment waits.
    let later = fingerprint(4);
    let added = fixture.run(&["ban-cards", "--email", "Bob@163.com", "--card", &later]);
    assert_eq!(
        added.out,
        "kept 1 card fingerprint(s) with the ban of that email address: payments made with \
         them will not be granted\n"
    );
    let order = fixture.order(&alice, "month-100", 9);
    fixture.told(60, vec![paid(9, order, "month-100"), charged(9, 4)]);
    let store = fixture.store();
    let waiting: Vec<_> = store
        .orders_waiting()
        .unwrap()
        .into_iter()
        .filter(|listed| matches!(listed.order.progress, Progress::Review(_)))
        .map(|listed| listed.order.id)
        .collect();
    assert_eq!(waiting, [oid(2), oid(8), oid(9)]);
    drop(store);
    // Only to an address that is banned, and only with a card named.
    let nobody = fixture.run(&["ban-cards", "--email", "carol@qq.com", "--card", &later]);
    assert_eq!(nobody.code, ExitCode::FAILURE);
    assert_eq!(
        nobody.err,
        "mooring-portal: that email address is not banned; nothing changed\n"
    );
    assert_eq!(
        fixture.run(&["ban-cards", "--email", "bob@163.com"]).code,
        ExitCode::from(2)
    );
    let malformed = fixture.run(&["ban", "--account", "0000-0000-0001", "--card", "no card"]);
    assert_eq!(malformed.code, ExitCode::from(2));
    assert!(
        malformed.err.contains("not a card fingerprint"),
        "{}",
        malformed.err
    );
    // Unbanned, the cards pay again.
    fixture.run(&["unban", "--email", "bob@163.com"]);
    let order = fixture.order(&alice, "month-100", 6);
    fixture.told(70, vec![paid(6, order, "month-100"), charged(6, 4)]);
    let listed = fixture.run(&["orders"]).out;
    assert!(
        listed.contains("66666666666666666666  0000-0000-0001  2026-11-05T12:00:00Z  granted "),
        "{listed}"
    );
}

#[test]
fn an_account_is_not_deleted_while_its_money_is_in_the_air() {
    let fixture = busy();
    for (account, said) in [
        (
            "0000-0000-0001",
            "mooring-portal: account 0000-0000-0001 has orders that are not settled \
             (22222222222222222222): a paid order is settled once Mooring has pulled it, one in \
             review by a full refund at Stripe or by `approve-order` (`orders` shows where each \
             stands)\n",
        ),
        (
            "0000-0000-0002",
            "mooring-portal: account 0000-0000-0002 has a subscription that Stripe may charge \
             again (sub_5): cancel it at Stripe, and delete the account once Stripe has told the \
             portal that it ended (`orders` shows when)\n",
        ),
    ] {
        let refused = fixture.run(&["delete-account", "--account", account]);
        assert_eq!(refused.code, ExitCode::FAILURE, "{account}");
        assert_eq!(refused.err, said);
        assert!(refused.out.is_empty());
    }
    assert!(fixture.run(&["accounts"]).out.ends_with("2 account(s)\n"));
    // Unbinding does not wait, but says what Stripe may still do.
    let unbound = fixture.run(&["unbind", "--account", "0000-0000-0002"]);
    assert_eq!(unbound.code, ExitCode::SUCCESS, "{}", unbound.err);
    assert_eq!(
        unbound.out,
        "unbound account 0000-0000-0002 from Mooring user pilotuserb; its subscription record, \
         CFM profile and subscription link were deleted\n\
         warning: Stripe may still charge subscription sub_5 of this account, which no longer \
         has a Mooring user to grant it to; cancel it at Stripe unless the account is to be \
         bound to pilotuserb again\n\
         erased the deleted rows from the database files\n"
    );
    // An account without a subscription is told of none: the other
    // account's is not its concern.
    let quiet = fixture.run(&["unbind", "--account", "0000-0000-0001"]);
    assert_eq!(
        quiet.out,
        "unbound account 0000-0000-0001 from Mooring user pilotusera; its subscription record, \
         CFM profile and subscription link were deleted\n\
         erased the deleted rows from the database files\n"
    );
    // The first account's order in review is refunded in full: it goes.
    fixture.told(
        80,
        vec![EventKind::ChargeRefunded(Refunded {
            payment_intent: sid("pi_2"),
            amount_refunded: 250,
            in_full: true,
        })],
    );
    let deleted = fixture.run(&["delete-account", "--account", "0000-0000-0001"]);
    assert_eq!(deleted.code, ExitCode::SUCCESS, "{}", deleted.err);
    // Nothing of its payments is left in the files.
    for trace in ["11111111111111111111", "cs_1", "pi_2"] {
        assert!(!fixture.files_hold(trace.as_bytes()), "{trace}");
    }
}

#[test]
fn the_payment_commands_report_a_failing_output() {
    let fixture = busy();
    let db = fixture.db();
    let line = |args: &[&str]| -> Vec<String> {
        let mut line = vec![
            "mooring-portal".to_owned(),
            "--db".to_owned(),
            db.to_str().unwrap().to_owned(),
        ];
        line.extend(args.iter().map(|arg| (*arg).to_owned()));
        line
    };
    for args in [
        &["orders"][..],
        &["pull-token"],
        &["approve-order", "--order", "22222222222222222222"],
        &["revoke-pull-token"],
    ] {
        let mut err = Vec::new();
        let code = crate::run(
            line(args),
            &fixture.clock,
            &mut super::FailAfter(0),
            &mut err,
        );
        assert_eq!(code, ExitCode::FAILURE, "{args:?}");
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "mooring-portal: cannot write the output: pipe closed\n",
            "{args:?}"
        );
    }
    let error = CommandError::from(portal_store::DismissError::Unknown(4));
    assert_eq!(
        error.to_string(),
        "there is no payment numbered 4 on the list; nothing changed"
    );
}
