//! The server and the operator's commands open the same file, possibly at
//! the same moment.

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use super::*;
use crate::schema::MIGRATIONS;
use crate::{OpenMode, StoreError};

/// How long the other connection keeps its lock: long enough for the
/// opener to be waiting for it, well within the busy timeout.
const HOLD: Duration = Duration::from_millis(300);

/// An empty owner-only file, as a first `serve` creates it.
fn new_file(dir: &tempfile::TempDir) -> PathBuf {
    let path = dir.path().join("portal.db");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    path
}

fn open_in_thread(path: &Path) -> thread::JoinHandle<Result<Store, StoreError>> {
    let path = path.to_owned();
    thread::spawn(move || Store::open(&path, OpenMode::Existing))
}

#[test]
fn a_file_another_opener_is_migrating_is_migrated_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    let mut other = Connection::open(&path).unwrap();
    other.pragma_update(None, "journal_mode", "wal").unwrap();
    let tx = other
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let opener = open_in_thread(&path);
    thread::sleep(HOLD);
    for sql in MIGRATIONS {
        tx.execute_batch(sql).unwrap();
    }
    tx.pragma_update(
        None,
        "user_version",
        i64::try_from(MIGRATIONS.len()).unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    let store = opener.join().unwrap().unwrap();
    assert!(store.accounts().unwrap().is_empty());
}

#[test]
fn opening_waits_while_another_opener_switches_the_file_to_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    let mut other = Connection::open(&path).unwrap();
    // A switch to WAL writes the file's header under an exclusive lock;
    // hold one for as long as a slow switch might.
    let tx = other
        .transaction_with_behavior(TransactionBehavior::Exclusive)
        .unwrap();
    let opener = open_in_thread(&path);
    thread::sleep(HOLD);
    tx.commit().unwrap();
    let store = opener.join().unwrap().unwrap();
    let journal: String = store
        .conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
}
