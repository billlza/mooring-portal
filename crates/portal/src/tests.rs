//! Tests of the command line against database files.

mod approving;
mod commands;
mod erasure;
mod import;
mod orders_list;
mod paying;
mod payments;
mod profiles;
mod release;
mod usage;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portal_core::account::{AccountCode, Email};
use portal_core::auth::{DigestedEmail, StoredPasswordHash, TokenDigest};
use portal_core::time::{ManualClock, UnixTime};
use portal_store::{Account, NewAccount, NewSession, OpenMode, Store};

/// 2026-11-05T12:00:00Z.
const NOW: u64 = 1_793_880_000;

/// A database file in a temporary directory, as the server's first start
/// leaves it.
struct Fixture {
    dir: tempfile::TempDir,
    clock: ManualClock,
}

/// What a command printed, and how it ended.
#[derive(Debug)]
struct Outcome {
    code: ExitCode,
    out: String,
    err: String,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().unwrap(),
            clock: ManualClock::new(UnixTime::from_secs(NOW).unwrap()),
        };
        drop(Store::open(&fixture.db(), OpenMode::CreateIfMissing).unwrap());
        fixture
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("portal.db")
    }

    /// A file next to the database.
    fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// A connection to the database, as the server holds one.
    fn store(&self) -> Store {
        Store::open(&self.db(), OpenMode::Existing).unwrap()
    }

    /// Registers `email` with the code `bits`.
    fn account(&self, email: &str, bits: u64) -> AccountCode {
        self.register(&mut self.store(), email, bits).code
    }

    /// Registers `email` with the code `bits` and signs it in for thirty
    /// days from [`NOW`]; the session token's digest.
    fn signed_in(&self, email: &str, bits: u64) -> (AccountCode, TokenDigest) {
        let mut store = self.store();
        let account = self.register(&mut store, email, bits);
        let digest = TokenDigest::from_bytes([u8::try_from(bits).unwrap(); 32]);
        let now = UnixTime::from_secs(NOW).unwrap();
        store
            .start_session(&NewSession {
                digest,
                account: account.id,
                created_at: now,
                expires_at: now.checked_add_secs(30 * 24 * 3600).unwrap(),
            })
            .unwrap();
        (account.code, digest)
    }

    fn register(&self, store: &mut Store, email: &str, bits: u64) -> Account {
        store
            .register(
                NewAccount {
                    code: AccountCode::from_bits(bits),
                    email: DigestedEmail::new(Email::parse(email).unwrap()).unwrap(),
                    password_hash: StoredPasswordHash::from_stored("$argon2id$x".to_owned()),
                    created_at: UnixTime::from_secs(NOW).unwrap(),
                },
                100,
            )
            .unwrap()
    }

    /// Whether the database or its write-ahead log holds `trace`.
    fn files_hold(&self, trace: &[u8]) -> bool {
        ["portal.db", "portal.db-wal"].into_iter().any(|name| {
            match std::fs::read(self.dir.path().join(name)) {
                Ok(bytes) => bytes.windows(trace.len()).any(|window| window == trace),
                // The last connection to close removes the log.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => panic!("cannot read {name}: {error}"),
            }
        })
    }

    /// Runs `mooring-portal --db <file> <args…>`.
    fn run(&self, args: &[&str]) -> Outcome {
        let db = self.db();
        let mut line: Vec<&str> = vec!["mooring-portal", "--db", db.to_str().unwrap()];
        line.extend_from_slice(args);
        run_line(&line, &self.clock)
    }
}

fn run_line(line: &[&str], clock: &ManualClock) -> Outcome {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = crate::run(line.iter().copied(), clock, &mut out, &mut err);
    Outcome {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// The version-1 snapshot fixture `name`.
fn contract_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract/fixtures/v1")
        .join(name)
}

/// The version-2 profile bundle fixture `name`.
fn profile_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract/fixtures/cfm-profiles-v2")
        .join(name)
}

/// Output that accepts `budget` bytes, then fails.
struct FailAfter(usize);

impl std::io::Write for FailAfter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("pipe closed"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
