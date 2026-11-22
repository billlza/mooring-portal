//! Translations in the database's files: a stored row is checked as it
//! is read, the tables hold one translation of a message at most in each
//! language of the pages, a broken database is an error, and version 9
//! comes of version 8 and goes back to it.

use portal_core::translation::PageLanguage;
use rusqlite::Connection;

use super::translations::{body, examined, rows, signed_in, translation, translations, waiting};
use super::*;
use crate::schema::MIGRATIONS;
use crate::{FeedbackId, OpenMode, StoreError, TranslationId, Waiting};

#[test]
fn what_is_stored_is_checked_as_it_is_read() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    let every = Waiting::since(at(NOW));
    let insert = "INSERT INTO feedback (id, account_id, from_operator, body, sent_at)
                  VALUES (?1, ?2, 0, ?3, ?4)";
    // A message no page would have taken.
    store
        .conn
        .execute(
            insert,
            rusqlite::params![1, account.id.0, "a\u{1b}[31mb", NOW.cast_signed()],
        )
        .unwrap();
    assert!(matches!(
        store.untranslated(&every, 10),
        Err(StoreError::Corrupt {
            what: "feedback message",
            ..
        })
    ));
    // A message numbered as SQLite numbers no row, whatever is asked for.
    store.conn.execute("DELETE FROM feedback", []).unwrap();
    for id in [0, -1] {
        store
            .conn
            .execute(
                insert,
                rusqlite::params![id, account.id.0, "x", NOW.cast_signed()],
            )
            .unwrap();
        for which in [
            every,
            Waiting {
                newer_than: FeedbackId::new(5),
                ..every
            },
        ] {
            assert!(
                matches!(
                    store.untranslated(&which, 10),
                    Err(StoreError::Corrupt {
                        what: "feedback number",
                        ..
                    })
                ),
                "{id} {which:?}"
            );
        }
        store.conn.execute("DELETE FROM feedback", []).unwrap();
    }
    // A translation no page could show, and one numbered as none is.
    store
        .send_feedback(&digest(1), &body("Hello"), at(NOW))
        .unwrap();
    for (id, text, what) in [
        (1, " ", "translation"),
        (1, "a\u{202e}b", "translation"),
        (0, "你好", "translation number"),
        (-1, "你好", "translation number"),
    ] {
        store
            .conn
            .execute(
                "INSERT INTO feedback_translations (id, feedback_id, language, body, made_at)
                 VALUES (?1, 1, 'zh', ?2, ?3)",
                rusqlite::params![id, text, NOW.cast_signed()],
            )
            .unwrap();
        let mut read = |after| {
            store.conversation_translated(
                &account.code,
                FeedbackId::START,
                PageLanguage::Chinese,
                TranslationId::new(after),
            )
        };
        let whole = read(0);
        assert!(
            matches!(whole, Err(StoreError::Corrupt { what: found, .. }) if found == what),
            "{id} {text:?}: {whole:?}"
        );
        // One numbered as none is is reported whatever is asked for; one
        // a page has passed is not read again.
        let since = read(5);
        if id < 1 {
            assert!(
                matches!(since, Err(StoreError::Corrupt { .. })),
                "{id}: {since:?}"
            );
        } else {
            assert_eq!(since.unwrap().unwrap().translations, []);
        }
        store
            .conn
            .execute("DELETE FROM feedback_translations", [])
            .unwrap();
    }
}

#[test]
fn a_message_has_one_translation_at_most_in_each_language_of_the_pages() {
    let mut store = store();
    signed_in(&mut store, "a@qq.com", 1, 1);
    store
        .send_feedback(&digest(1), &body("こんにちは"), at(NOW))
        .unwrap();
    let hello = examined(Some("ja"), &[(PageLanguage::Chinese, "你好")]);
    assert!(
        store
            .record_examined(&waiting(1, "こんにちは"), &hello, at(NOW))
            .unwrap()
    );
    // Whatever writes to the tables: a message is examined once, and has
    // one text in each language of the pages and none in another.
    let refused = |sql: &str, constraint| {
        let written = store.conn.execute(sql, [NOW.cast_signed()]);
        assert!(
            matches!(&written, Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.extended_code == constraint),
            "{sql}: {written:?}"
        );
    };
    refused(
        "INSERT INTO feedback_examined (feedback_id, language, examined_at)
         VALUES (1, 'ja', ?1)",
        rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY,
    );
    refused(
        "INSERT INTO feedback_translations (feedback_id, language, body, made_at)
         VALUES (1, 'zh', '您好', ?1)",
        rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
    );
    refused(
        "INSERT INTO feedback_translations (feedback_id, language, body, made_at)
         VALUES (1, 'ja', 'こんにちは', ?1)",
        rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
    );
    // In the other language of the pages it may have one.
    store
        .conn
        .execute(
            "INSERT INTO feedback_translations (feedback_id, language, body, made_at)
             VALUES (1, 'en', 'Hello', ?1)",
            [NOW.cast_signed()],
        )
        .unwrap();
    assert_eq!(rows(&store, "feedback_examined"), 1);
    assert_eq!(rows(&store, "feedback_translations"), 2);
}

#[test]
fn a_broken_database_is_an_error_from_every_operation() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    store
        .send_feedback(&digest(1), &body("Hello"), at(NOW))
        .unwrap();
    let every = Waiting::since(at(NOW));
    let hello = examined(Some("en"), &[(PageLanguage::Chinese, "你好")]);
    let read = |store: &mut Store| {
        store.conversation_translated(
            &account.code,
            FeedbackId::START,
            PageLanguage::Chinese,
            TranslationId::START,
        )
    };
    // Without the translations, and then without what says a message was
    // examined.
    store
        .conn
        .execute_batch("DROP TABLE feedback_translations;")
        .unwrap();
    assert!(matches!(read(&mut store), Err(StoreError::Sqlite(_))));
    assert!(matches!(
        store.record_examined(&waiting(1, "Hello"), &hello, at(NOW)),
        Err(StoreError::Sqlite(_))
    ));
    // The translation that could not be written took nothing with it.
    assert_eq!(rows(&store, "feedback_examined"), 0);
    assert_eq!(
        store.untranslated(&every, 10).unwrap(),
        [waiting(1, "Hello")]
    );
    store
        .conn
        .execute_batch("DROP TABLE feedback_examined;")
        .unwrap();
    assert!(matches!(
        store.untranslated(&every, 10),
        Err(StoreError::Sqlite(_))
    ));
    assert!(matches!(
        store.record_examined(&waiting(1, "Hello"), &examined(None, &[]), at(NOW)),
        Err(StoreError::Sqlite(_))
    ));
}

#[test]
fn a_version_8_database_gains_translations_and_keeps_its_conversations() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    {
        let conn = Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute(
            "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
             VALUES ('0000-0000-0001', 'a@qq.com', ?1, ?2, ?2)",
            rusqlite::params![hash("first").as_str(), NOW.cast_signed()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO feedback (account_id, from_operator, body, sent_at)
                 VALUES (1, 0, 'リンクが開きません', 5), (1, 1, '请重新生成链接', 6);",
        )
        .unwrap();
    }
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
    let code = AccountCode::from_bits(1);
    assert_eq!(
        store
            .conversation_with(&code, FeedbackId::START)
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    // What users wrote before waits for the translator like anything
    // written after.
    let handed = store.untranslated(&Waiting::since(at(5)), 10).unwrap();
    assert_eq!(handed, [waiting(1, "リンクが開きません")]);
    let chinese = examined(Some("ja"), &[(PageLanguage::Chinese, "链接打不开")]);
    assert!(
        store
            .record_examined(&handed[0], &chinese, at(NOW))
            .unwrap()
    );
    assert_eq!(
        translations(&mut store, &code, PageLanguage::Chinese, 0),
        [translation(1, 1, "链接打不开")]
    );
}

/// What `docs/deploy.md` says to run on the database before going back to
/// a program that knows only version 8.
pub(super) const ROLLBACK_TO_8: &str = "DROP TABLE feedback_translations; DROP TABLE feedback_examined; \
                             PRAGMA user_version = 8;";

#[test]
fn the_documented_rollback_leaves_version_8_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let version_8 = new_file(&dir);
    {
        let conn = Connection::open(&version_8).unwrap();
        for sql in &MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
    }
    let rolled_back = dir.path().join("rolled-back.db");
    {
        let mut store = Store::open(&rolled_back, OpenMode::CreateIfMissing).unwrap();
        signed_in(&mut store, "a@qq.com", 1, 1);
        store
            .send_feedback(&digest(1), &body("こんにちは"), at(NOW))
            .unwrap();
        let hello = examined(Some("ja"), &[(PageLanguage::English, "Hello")]);
        assert!(
            store
                .record_examined(&waiting(1, "こんにちは"), &hello, at(NOW))
                .unwrap()
        );
    }
    let conn = Connection::open(&rolled_back).unwrap();
    conn.execute_batch(ROLLBACK_TO_8).unwrap();
    assert_eq!(schema(&rolled_back), schema(&version_8));
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 8);
    // The last number is forgotten with the table that counted it.
    let counted: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_sequence WHERE name = 'feedback_translations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(counted, 0);
    drop(conn);
    // The program of today brings it forward again, keeping the
    // conversation, whose message waits for the translator once more.
    let store = Store::open(&rolled_back, OpenMode::Existing).unwrap();
    assert_eq!(
        store.untranslated(&Waiting::since(at(NOW)), 10).unwrap(),
        [waiting(1, "こんにちは")]
    );
}
