use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use super::*;
use crate::schema::MIGRATIONS;
use crate::{OpenMode, StoreError};

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_new_database_is_owner_only_with_its_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    let mut store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
    register(&mut store, "a@qq.com", 1);
    assert_eq!(mode(&path), 0o600);
    for suffix in ["-wal", "-shm"] {
        let side = dir.path().join(format!("portal.db{suffix}"));
        assert_eq!(mode(&side), 0o600, "{suffix}");
    }
    drop(store);
    // The data and the schema survive reopening.
    let store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
    assert_eq!(store.accounts().unwrap().len(), 1);
    let journal: String = store
        .conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    let foreign_keys: bool = store
        .conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    // Each connection zeroes what it deletes, the free pages included:
    // 1 is "on", where 2 ("fast") would leave free pages as they were.
    let secure_delete: i64 = store
        .conn
        .pragma_query_value(None, "secure_delete", |row| row.get(0))
        .unwrap();
    assert_eq!(secure_delete, 1);
    // Temporary databases, such as the copy VACUUM builds, stay in memory:
    // 2 is "memory".
    let temp_store: i64 = store
        .conn
        .pragma_query_value(None, "temp_store", |row| row.get(0))
        .unwrap();
    assert_eq!(temp_store, 2);
}

#[test]
fn a_database_others_can_read_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    drop(Store::open(&path, OpenMode::CreateIfMissing).unwrap());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Store::open(&path, OpenMode::CreateIfMissing),
        Err(StoreError::InsecurePermissions { mode: 0o644, .. })
    ));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    Store::open(&path, OpenMode::CreateIfMissing).unwrap();
}

#[test]
fn only_an_opener_that_may_create_a_database_creates_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    assert!(matches!(
        Store::open(&path, OpenMode::Existing),
        Err(StoreError::NoDatabase { path: missing }) if missing == path
    ));
    assert!(!path.exists(), "nothing is created");
    drop(Store::open(&path, OpenMode::CreateIfMissing).unwrap());
    Store::open(&path, OpenMode::Existing).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(
        Store::open(&path, OpenMode::Existing),
        Err(StoreError::InsecurePermissions { mode: 0o640, .. })
    ));
}

#[test]
fn an_unreachable_path_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent").join("portal.db");
    assert!(matches!(
        Store::open(&absent, OpenMode::CreateIfMissing),
        Err(StoreError::File { .. })
    ));
    // Below a plain file, even looking the path up fails.
    let plain = dir.path().join("plain");
    fs::write(&plain, b"").unwrap();
    for mode in [OpenMode::CreateIfMissing, OpenMode::Existing] {
        assert!(
            matches!(
                Store::open(&plain.join("portal.db"), mode),
                Err(StoreError::File { .. })
            ),
            "{mode:?}"
        );
    }
}

#[test]
fn schema_versions_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    let store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
    let version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, i64::try_from(MIGRATIONS.len()).unwrap());
    store
        .conn
        .pragma_update(None, "user_version", version + 1)
        .unwrap();
    drop(store);
    assert!(matches!(
        Store::open(&path, OpenMode::CreateIfMissing),
        Err(StoreError::SchemaTooNew { found, supported }) if found == version + 1 && supported == version
    ));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", -1).unwrap();
    drop(conn);
    assert!(matches!(
        Store::open(&path, OpenMode::CreateIfMissing),
        Err(StoreError::Corrupt {
            what: "schema version",
            ..
        })
    ));
}
