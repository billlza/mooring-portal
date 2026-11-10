//! The release on offer: one row, replaced whole, and checked when read.

use portal_core::release::{CfmRelease, Version};
use rusqlite::Connection;

use super::*;
use crate::schema::MIGRATIONS;
use crate::{OfferedRelease, OpenMode, ReleaseChange, StoreError};

const SHA_A: &str = "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f";
const SHA_B: &str = "fc9a1e62700a1b5f66ec42fcda0e5eb90efc052251321927f52b97c9223b7b93";

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn release(number: &str, published: u64, sha: &str) -> CfmRelease {
    CfmRelease::from_parts(version(number), at(published), sha).unwrap()
}

#[test]
fn nothing_is_on_offer_until_a_release_is_imported() {
    assert_eq!(store().offered_release().unwrap(), None);
}

#[test]
fn each_import_replaces_the_release_and_says_what_changed() {
    let mut store = store();
    let first = release("0.5.9", NOW - 1000, SHA_A);
    assert_eq!(
        store.offer_release(&first, at(NOW)).unwrap(),
        ReleaseChange::First
    );
    assert_eq!(
        store.offered_release().unwrap(),
        Some(OfferedRelease {
            release: first.clone(),
            checked_at: at(NOW)
        })
    );
    // Found to be the latest again: only the time of the check moves.
    assert_eq!(
        store.offer_release(&first, at(NOW + 300)).unwrap(),
        ReleaseChange::Unchanged
    );
    assert_eq!(
        store.offered_release().unwrap(),
        Some(OfferedRelease {
            release: first.clone(),
            checked_at: at(NOW + 300)
        })
    );
    // A newer one, by number: 0.5.10 follows 0.5.9.
    let second = release("0.5.10", NOW, SHA_B);
    assert_eq!(
        store.offer_release(&second, at(NOW + 600)).unwrap(),
        ReleaseChange::Newer {
            from: version("0.5.9")
        }
    );
    assert_eq!(
        store.offered_release().unwrap(),
        Some(OfferedRelease {
            release: second,
            checked_at: at(NOW + 600)
        })
    );
    // The newer one withdrawn: the page goes back with GitHub.
    assert_eq!(
        store.offer_release(&first, at(NOW + 900)).unwrap(),
        ReleaseChange::Older {
            from: version("0.5.10")
        }
    );
    assert_eq!(store.offered_release().unwrap().unwrap().release, first);
    // The same version with another disk image, or another time of
    // publication, was published again.
    let swapped = release("0.5.9", NOW - 1000, SHA_B);
    assert_eq!(
        store.offer_release(&swapped, at(NOW + 1200)).unwrap(),
        ReleaseChange::Republished
    );
    let redated = release("0.5.9", NOW - 500, SHA_B);
    assert_eq!(
        store.offer_release(&redated, at(NOW + 1500)).unwrap(),
        ReleaseChange::Republished
    );
    assert_eq!(
        store.offered_release().unwrap(),
        Some(OfferedRelease {
            release: redated,
            checked_at: at(NOW + 1500)
        })
    );
    let rows: i64 = store
        .conn
        .query_row("SELECT count(*) FROM cfm_release", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn a_row_that_no_longer_reads_is_corrupt_not_absent() {
    for (change, what) in [
        ("version = '0.5'", "release version"),
        ("version = 'v0.5.1'", "release version"),
        ("published_at = -1", "release publication time"),
        ("checked_at = -1", "release check time"),
        (
            "dmg_sha256 = upper(dmg_sha256)",
            "release disk image digest",
        ),
    ] {
        let mut store = store();
        store
            .offer_release(&release("0.5.1", NOW, SHA_A), at(NOW))
            .unwrap();
        store
            .conn
            .execute(&format!("UPDATE cfm_release SET {change}"), [])
            .unwrap();
        match store.offered_release() {
            Err(StoreError::Corrupt { what: found, .. }) => assert_eq!(found, what, "{change}"),
            other => panic!("{change}: {other:?}"),
        }
        // Nor is a row that does not read replaced as if nothing were there.
        assert!(matches!(
            store.offer_release(&release("0.5.2", NOW, SHA_B), at(NOW)),
            Err(StoreError::Corrupt { .. })
        ));
    }
    // The table holds one row, and a digest of 64 characters.
    let store = store();
    for refused in [
        format!("INSERT INTO cfm_release VALUES (2, '0.5.1', 0, '{SHA_A}', 0)"),
        format!(
            "INSERT INTO cfm_release VALUES (1, '0.5.1', 0, '{}', 0)",
            &SHA_A[1..]
        ),
    ] {
        assert!(store.conn.execute(&refused, []).is_err(), "{refused}");
    }
}

#[test]
fn a_database_from_before_releases_gains_the_table_and_keeps_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    {
        let conn = Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute(
            "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
             VALUES ('0000-0000-0001', 'a@qq.com', ?1, ?2, ?2)",
            rusqlite::params![hash("first").as_str(), NOW.cast_signed()],
        )
        .unwrap();
    }
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let found: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(usize::try_from(found).unwrap(), MIGRATIONS.len());
    assert_eq!(MIGRATIONS.len(), 4);
    assert_eq!(store.accounts().unwrap().len(), 1);
    assert_eq!(store.offered_release().unwrap(), None);
    let offered = release("0.5.1", NOW, SHA_A);
    store.offer_release(&offered, at(NOW)).unwrap();
    // Another connection, as the server's is to an operator's command,
    // reads what was committed.
    let reader = Store::open(&path, OpenMode::Existing).unwrap();
    assert_eq!(reader.offered_release().unwrap().unwrap().release, offered);
}
