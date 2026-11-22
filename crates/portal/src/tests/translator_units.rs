//! The units that run the translator and `translate` on the host
//! (`docs/translation.md`, section 5). No unit runs here, so these tests
//! hold the notes to the portal's command line, to the portal's own unit
//! and to the file the notices watch. The translator's own tests
//! (`crates/portal/translator/test/`) hold the translator's command line
//! in the notes to the script.

use std::path::Path;
use std::time::Duration;

use clap::Parser;
use portal_core::translation::MAX_TRANSLATION_CHARS;

use super::{block_after, setting};
use crate::cli::{Cli, Command};
use crate::translate::PATIENCE;

const NOTES: &str = include_str!("../../../../docs/translation.md");
/// How the portal itself is run.
const DEPLOYMENT: &str = include_str!("../../../../docs/deploy.md");
/// The notices watch the same file.
const NOTICES: &str = include_str!("../../../../docs/notices.md");
const TRANSLATOR: &str = include_str!("../../translator/translator.mjs");

/// The unit file `name` as the notes have it.
fn unit(name: &str) -> String {
    block_after(NOTES, &format!("`/etc/systemd/system/{name}`"), "```ini")
}

/// The portal's own unit.
fn portal() -> String {
    block_after(
        DEPLOYMENT,
        "`/etc/systemd/system/mooring-portal.service`",
        "```ini",
    )
}

/// The command line `ExecStart=` runs in `block`, as the portal reads it.
fn command_line(block: &str) -> Cli {
    let command = setting(block, "Service", "ExecStart=/usr/local/bin/mooring-portal ");
    Cli::try_parse_from(std::iter::once("mooring-portal").chain(command.split_whitespace()))
        .unwrap()
}

#[test]
fn translate_runs_as_the_portal_on_its_database_and_asks_at_the_translators_socket() {
    let portal = portal();
    let service = unit("mooring-portal-translate.service");
    let cli = command_line(&service);
    let Command::Translate { translator } = &cli.command else {
        panic!("not translate");
    };
    // The portal's database, as the portal's account, which may write
    // there and nowhere else.
    assert_eq!(cli.db, command_line(&portal).db);
    let account = setting(&portal, "Service", "User=");
    assert_eq!(setting(&service, "Service", "User="), account);
    let writable = setting(&portal, "Service", "ReadWritePaths=");
    assert_eq!(setting(&service, "Service", "ReadWritePaths="), writable);
    assert_eq!(cli.db.parent(), Some(Path::new(writable)));
    // It asks at the socket systemd listens on, which the portal's
    // account may reach and no other but root.
    let socket = unit("mooring-portal-translator.socket");
    assert_eq!(
        Path::new(setting(&socket, "Socket", "ListenStream=")),
        translator
    );
    assert_eq!(setting(&socket, "Socket", "SocketGroup="), account);
    assert_eq!(setting(&socket, "Socket", "SocketMode="), "0660");
    assert_eq!(
        setting(&service, "Unit", "Requires="),
        "mooring-portal-translator.socket"
    );
    assert_eq!(
        setting(&service, "Unit", "After="),
        "mooring-portal-translator.socket"
    );
    // A run may be long, and starting it often is no fault: limits are
    // settings of the unit and of its service, each in its place.
    assert_eq!(setting(&service, "Service", "Type="), "oneshot");
    assert_eq!(setting(&service, "Service", "TimeoutStartSec="), "1h");
    assert_eq!(setting(&service, "Unit", "StartLimitIntervalSec="), "0");
}

#[test]
fn the_translator_answers_one_connection_at_a_time_and_ends_before_the_portal_gives_up() {
    let socket = unit("mooring-portal-translator.socket");
    assert_eq!(setting(&socket, "Socket", "Accept="), "yes");
    assert_eq!(setting(&socket, "Socket", "MaxConnections="), "1");
    let service = unit("mooring-portal-translator@.service");
    let limit = setting(&service, "Service", "RuntimeMaxSec=")
        .parse()
        .unwrap();
    assert!(Duration::from_secs(limit) < PATIENCE);
    assert_eq!(setting(&service, "Service", "MemoryMax="), "900M");
    assert_eq!(setting(&service, "Service", "MemorySwapMax="), "0");
    // An instance's settings, each in its place.
    assert_eq!(
        setting(&service, "Unit", "CollectMode="),
        "inactive-or-failed"
    );
    assert_eq!(setting(&socket, "Install", "WantedBy="), "sockets.target");
}

#[test]
fn the_translator_reaches_neither_the_network_nor_the_portals_files() {
    let service = unit("mooring-portal-translator@.service");
    assert_eq!(setting(&service, "Service", "DynamicUser="), "yes");
    assert_eq!(setting(&service, "Service", "PrivateNetwork="), "yes");
    assert_eq!(
        setting(&service, "Service", "RestrictAddressFamilies="),
        "AF_UNIX"
    );
    let hidden: Vec<&str> = setting(&service, "Service", "InaccessiblePaths=")
        .split_whitespace()
        .collect();
    let writable = format!("-{}", setting(&portal(), "Service", "ReadWritePaths="));
    // The portal's data, and the notices' credentials.
    assert_eq!(hidden, [writable.as_str(), "-/etc/mooring-portal"]);
    assert!(NOTICES.contains("LoadCredential=telegram:/etc/mooring-portal/telegram"));
    assert!(NOTICES.contains("LoadCredential=feishu:/etc/mooring-portal/feishu"));
    // The script it runs is the one the notes install.
    let command = setting(&service, "Service", "ExecStart=");
    let mut words = command.split_whitespace();
    assert_eq!(words.next(), Some("/usr/bin/node"));
    let script = words.next().unwrap();
    assert!(NOTES.contains(&format!(
        "/home/builder/portal/crates/portal/translator/translator.mjs {script}\n"
    )));
}

#[test]
fn translate_runs_when_the_portal_writes_its_new_message_file_and_after_each_run_on_a_timer() {
    // The notices' path unit watches the file the portal writes
    // (`notices.rs`); this one watches the same.
    let notice = block_after(
        NOTICES,
        "`/etc/systemd/system/mooring-portal-notify.path`",
        "```ini",
    );
    let watch = unit("mooring-portal-translate.path");
    assert_eq!(
        setting(&watch, "Path", "PathChanged="),
        setting(&notice, "Path", "PathChanged=")
    );
    assert_eq!(
        setting(&watch, "Path", "Unit="),
        "mooring-portal-translate.service"
    );
    let timer = unit("mooring-portal-translate.timer");
    assert_eq!(
        setting(&timer, "Timer", "Unit="),
        "mooring-portal-translate.service"
    );
    assert_eq!(setting(&timer, "Timer", "OnUnitInactiveSec="), "15min");
    assert_eq!(setting(&watch, "Install", "WantedBy="), "paths.target");
    assert_eq!(setting(&timer, "Install", "WantedBy="), "timers.target");
    // The notes enable what they install.
    for line in [
        "systemctl enable --now mooring-portal-translator.socket\n",
        "systemctl enable --now mooring-portal-translate.path mooring-portal-translate.timer\n",
    ] {
        assert!(NOTES.contains(line), "{line}");
    }
}

#[test]
fn the_translator_takes_the_longest_text_the_portal_asks_about() {
    let bytes: usize = TRANSLATOR
        .lines()
        .find_map(|line| line.strip_prefix("export const MAX_TEXT_BYTES = "))
        .unwrap()
        .trim_end_matches(';')
        .replace('_', "")
        .parse()
        .unwrap();
    // The longest is a translation from English, kept only if it has so
    // many characters at most, of four bytes at most each.
    assert_eq!(bytes, 4 * MAX_TRANSLATION_CHARS);
}
