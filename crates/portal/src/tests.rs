//! Tests of the command line against database files.

mod commands;
mod import;
mod usage;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portal_core::account::{AccountCode, Email};
use portal_core::auth::StoredPasswordHash;
use portal_core::time::{ManualClock, UnixTime};
use portal_store::{NewAccount, OpenMode, Store};

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
        let mut store = self.store();
        let code = AccountCode::from_bits(bits);
        store
            .register(
                NewAccount {
                    code,
                    email: Email::parse(email).unwrap(),
                    password_hash: StoredPasswordHash::from_stored("$argon2id$x".to_owned()),
                    created_at: UnixTime::from_secs(NOW).unwrap(),
                },
                100,
            )
            .unwrap();
        code
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

/// The version-1 fixture `name`.
fn contract_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract/fixtures/v1")
        .join(name)
}
