//! Tests of the store against in-memory and on-disk databases.

mod accounts;
mod approval;
mod bans;
mod bindings;
mod erasure;
mod failures;
mod feedback;
mod file;
mod journal;
mod leaving;
mod links;
mod migration;
mod modes;
mod operator_actions;
mod operators;
mod orders;
mod paying;
mod payment_failures;
mod payments;
mod profile_for_account;
mod profiles;
mod releases;
mod renewals;
mod rentals;
mod review;
mod sessions;
mod sharing;
mod signals;
mod strays;
mod subscriptions;
mod translations;
mod translations_schema;

use portal_core::account::{AccountCode, Email};
use portal_core::auth::{BanDigest, DigestedEmail, StoredPasswordHash, TokenDigest};
use portal_core::profile::{CfmProfile, ProfileBundle, WithheldReason};
use portal_core::snapshot::{BillingCycle, MooringUserId, Snapshot, UserRecord, UserStatus};
use portal_core::time::{CalendarDate, UnixTime};

use crate::{Account, NewAccount, NewSession, ReplyError, Store, StoredProfile};

/// 2026-11-05T12:00:00Z.
const NOW: u64 = 1_793_880_000;

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

/// An empty owner-only file, as a first `serve` creates it.
fn new_file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::OpenOptionsExt as _;
    let path = dir.path().join("portal.db");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    path
}

/// Every table, index and trigger of the database at `path`, as SQLite
/// keeps their definitions.
fn schema(path: &std::path::Path) -> Vec<(String, String, Option<String>)> {
    rusqlite::Connection::open(path)
        .unwrap()
        .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn at(secs: u64) -> UnixTime {
    UnixTime::from_secs(secs).unwrap()
}

fn email(text: &str) -> Email {
    Email::parse(text).unwrap()
}

/// The ban list's digest of `text`.
fn ban_digest(text: &str) -> BanDigest {
    BanDigest::of(&email(text)).unwrap()
}

/// `text` with the ban list's digest of it, as registration takes it.
fn digested(text: &str) -> DigestedEmail {
    DigestedEmail::new(email(text)).unwrap()
}

fn user(text: &str) -> MooringUserId {
    MooringUserId::parse(text).unwrap()
}

fn hash(text: &str) -> StoredPasswordHash {
    StoredPasswordHash::from_stored(format!("$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ${text}"))
}

/// Registers `address` with code `bits`, at most 100 accounts.
fn register(store: &mut Store, address: &str, bits: u64) -> Account {
    store
        .register(
            NewAccount {
                code: AccountCode::from_bits(bits),
                email: digested(address),
                password_hash: hash("first"),
                created_at: at(NOW + bits),
            },
            100,
        )
        .unwrap()
}

fn digest(byte: u8) -> TokenDigest {
    TokenDigest::from_bytes([byte; 32])
}

fn session(account: &Account, byte: u8, created: u64, ttl: u64) -> NewSession {
    NewSession {
        digest: digest(byte),
        account: account.id,
        created_at: at(created),
        expires_at: at(created + ttl),
    }
}

/// The operator's `body` to the account with `code` at `now`, as the
/// operator pages send it but without their check of the session, which
/// `operator_actions` tests.
fn reply(
    store: &mut Store,
    code: &AccountCode,
    body: &portal_core::feedback::MessageBody,
    now: UnixTime,
) -> Result<(), ReplyError> {
    let tx = store
        .conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    crate::feedback::reply_on(&tx, code, body, now)?;
    tx.commit()?;
    Ok(())
}

/// A snapshot of November 2026 generated at `generated`, with usage
/// counted to it and one active record per listed user.
fn snapshot(generated: u64, users: &[(&str, Option<u64>)]) -> Snapshot {
    let cycle = BillingCycle::new(
        CalendarDate::parse("2026-11-01").unwrap(),
        CalendarDate::parse("2026-12-01").unwrap(),
    )
    .unwrap();
    Snapshot::new(
        at(generated),
        cycle,
        Some(at(generated)),
        users.iter().map(|(id, used)| {
            (
                user(id),
                UserRecord {
                    status: UserStatus::Active,
                    cycle_served_bytes: *used,
                    reality_downlink_counted_at_close: true,
                    quota: None,
                    expires_at: None,
                },
            )
        }),
    )
    .unwrap()
}

/// A one-outbound CFM document tagged `tag`.
fn cfm(tag: &str) -> CfmProfile {
    CfmProfile::parse(&format!(
        "{{\"outbounds\": [{{\"type\": \"vless\", \"tag\": \"{tag}\"}}]}}"
    ))
    .unwrap()
}

/// A bundle generated at `generated` with a document per listed user,
/// tagged by the user's id, that withholds no one.
fn bundle(generated: u64, users: &[&str]) -> ProfileBundle {
    withholding(generated, users, &[])
}

/// [`bundle`], also withholding the users `withheld` for the reasons
/// given.
fn withholding(
    generated: u64,
    users: &[&str],
    withheld: &[(&str, WithheldReason)],
) -> ProfileBundle {
    ProfileBundle::new(
        at(generated),
        users.iter().map(|id| (user(id), cfm(id))),
        withheld.iter().map(|(id, reason)| (user(id), *reason)),
    )
    .unwrap()
}
