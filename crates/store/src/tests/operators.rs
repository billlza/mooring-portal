//! Operators: only the host's command makes one, and an operator's session
//! confirms the password by being replaced with a confirmed one, which
//! lasts [`OPERATOR_CONFIRM_SECS`].

use rusqlite::Connection;

use super::*;
use crate::schema::MIGRATIONS;
use crate::{FeedbackId, OPERATOR_CONFIRM_SECS, OpenMode, OperatorError, OperatorSession};

/// Whether an operator's account with `code` is listed as one.
fn listed_operator(store: &Store, code: AccountCode) -> bool {
    store
        .accounts()
        .unwrap()
        .iter()
        .any(|account| account.code == code && account.operator)
}

/// The sessions there are, by digest's first byte, with when each
/// confirmed the password.
fn sessions(store: &Store) -> Vec<(u8, Option<i64>)> {
    store
        .conn
        .prepare("SELECT token_digest, confirmed_at FROM sessions ORDER BY token_digest")
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?[0], row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn granting_and_revoking_say_whether_anything_changed() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    let other = register(&mut store, "b@qq.com", 2);
    let nobody = AccountCode::from_bits(9);
    assert!(matches!(
        store.grant_operator(&nobody, at(NOW)),
        Err(OperatorError::UnknownAccount(_))
    ));
    assert!(matches!(
        store.revoke_operator(&nobody),
        Err(OperatorError::UnknownAccount(_))
    ));
    assert!(!listed_operator(&store, account.code));
    assert!(store.grant_operator(&account.code, at(NOW)).unwrap());
    assert!(!store.grant_operator(&account.code, at(NOW + 1)).unwrap());
    assert!(listed_operator(&store, account.code));
    assert!(!listed_operator(&store, other.code));
    assert!(store.revoke_operator(&account.code).unwrap());
    assert!(!store.revoke_operator(&account.code).unwrap());
    assert!(!listed_operator(&store, account.code));
}

#[test]
fn confirming_replaces_an_operators_live_session_and_nothing_else() {
    let mut store = store();
    let operator = register(&mut store, "a@qq.com", 1);
    let user = register(&mut store, "b@qq.com", 2);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    store
        .start_session(&session(&operator, 1, NOW, 3600))
        .unwrap();
    store
        .start_session(&session(&operator, 3, NOW, 3600))
        .unwrap();
    store.start_session(&session(&user, 2, NOW, 3600)).unwrap();
    let unconfirmed = OperatorSession {
        account: operator.id,
        confirmed: false,
    };
    assert_eq!(
        store.operator_session(&digest(1), at(NOW)).unwrap(),
        Some(unconfirmed)
    );
    // Someone else's session, an unknown one, and an expired one.
    assert_eq!(store.operator_session(&digest(2), at(NOW)).unwrap(), None);
    assert_eq!(store.operator_session(&digest(9), at(NOW)).unwrap(), None);
    assert_eq!(
        store.operator_session(&digest(1), at(NOW + 3600)).unwrap(),
        None
    );
    // None of them confirms, nor does a replacement for another account.
    for (old, replacement) in [
        (2, session(&user, 0x82, NOW + 5, 3600)),
        (9, session(&operator, 0x89, NOW + 5, 3600)),
        (1, session(&operator, 0x81, NOW + 3600, 3600)),
        (1, session(&user, 0x81, NOW + 5, 3600)),
    ] {
        assert!(!store.confirm_session(&digest(old), &replacement).unwrap());
    }
    assert_eq!(sessions(&store), [(1, None), (2, None), (3, None)]);
    let replacement = session(&operator, 0x81, NOW + 5, 3600);
    assert!(store.confirm_session(&digest(1), &replacement).unwrap());
    // The old token opens nothing; the new one is confirmed; the
    // operator's other session and the user's are as they were.
    assert_eq!(
        sessions(&store),
        [(2, None), (3, None), (0x81, Some((NOW + 5).cast_signed()))]
    );
    assert_eq!(
        store.session_account(&digest(1), at(NOW + 6)).unwrap(),
        None
    );
    assert_eq!(
        store.operator_session(&digest(0x81), at(NOW + 6)).unwrap(),
        Some(OperatorSession {
            confirmed: true,
            ..unconfirmed
        })
    );
    assert_eq!(
        store.operator_session(&digest(3), at(NOW + 6)).unwrap(),
        Some(unconfirmed)
    );
}

#[test]
fn a_confirmation_lasts_its_time_and_no_longer() {
    let mut store = store();
    let operator = register(&mut store, "a@qq.com", 1);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    store
        .start_session(&session(&operator, 1, NOW, 86_400))
        .unwrap();
    let confirmed = NOW + 10;
    assert!(
        store
            .confirm_session(&digest(1), &session(&operator, 2, confirmed, 86_400))
            .unwrap()
    );
    let lately = |store: &Store, now| {
        store
            .operator_session(&digest(2), at(now))
            .unwrap()
            .unwrap()
            .confirmed
    };
    assert!(lately(&store, confirmed));
    assert!(lately(&store, confirmed + OPERATOR_CONFIRM_SECS - 1));
    assert!(!lately(&store, confirmed + OPERATOR_CONFIRM_SECS));
    // A clock set back leaves a confirmation in the future, which counts
    // for nothing.
    assert!(!lately(&store, confirmed - 1));
}

#[test]
fn revoking_forgets_every_confirmation() {
    let mut store = store();
    let operator = register(&mut store, "a@qq.com", 1);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    for byte in [1, 2] {
        store
            .start_session(&session(&operator, byte, NOW, 3600))
            .unwrap();
        let replacement = session(&operator, byte | 0x80, NOW + 1, 3600);
        assert!(store.confirm_session(&digest(byte), &replacement).unwrap());
    }
    store.revoke_operator(&operator.code).unwrap();
    assert_eq!(
        store.operator_session(&digest(0x81), at(NOW + 2)).unwrap(),
        None
    );
    store.grant_operator(&operator.code, at(NOW + 3)).unwrap();
    for byte in [0x81, 0x82] {
        assert!(
            !store
                .operator_session(&digest(byte), at(NOW + 4))
                .unwrap()
                .unwrap()
                .confirmed
        );
    }
}

#[test]
fn a_new_password_leaves_no_session_confirmed() {
    let mut store = store();
    let operator = register(&mut store, "a@qq.com", 1);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    store
        .start_session(&session(&operator, 1, NOW, 3600))
        .unwrap();
    store
        .confirm_session(&digest(1), &session(&operator, 3, NOW + 1, 3600))
        .unwrap();
    store
        .change_password(&hash("second"), &session(&operator, 2, NOW + 2, 3600))
        .unwrap();
    assert_eq!(
        store.operator_session(&digest(3), at(NOW + 3)).unwrap(),
        None
    );
    assert_eq!(
        store.operator_session(&digest(2), at(NOW + 3)).unwrap(),
        Some(OperatorSession {
            account: operator.id,
            confirmed: false,
        })
    );
}

#[test]
fn an_account_takes_its_operator_rights_with_it() {
    let mut store = store();
    let deleted = register(&mut store, "a@qq.com", 1);
    let banned = register(&mut store, "b@qq.com", 2);
    for account in [&deleted, &banned] {
        store.grant_operator(&account.code, at(NOW)).unwrap();
    }
    store.delete_account(&deleted.code).unwrap();
    store.ban(&banned.code, &[], at(NOW + 1)).unwrap();
    let left: i64 = store
        .conn
        .query_row("SELECT count(*) FROM operators", [], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_version_7_database_gains_operators_and_feedback_and_keeps_its_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    {
        let conn = Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
        conn.execute(
            "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
             VALUES ('0000-0000-0001', 'a@qq.com', ?1, ?2, ?2)",
            rusqlite::params![hash("first").as_str(), NOW.cast_signed()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (token_digest, account_id, created_at, expires_at)
             VALUES (?1, 1, ?2, ?3)",
            rusqlite::params![
                digest(1).as_bytes(),
                NOW.cast_signed(),
                (NOW + 3600).cast_signed()
            ],
        )
        .unwrap();
    }
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
    let account = store.session_account(&digest(1), at(NOW)).unwrap().unwrap();
    assert_eq!(store.operator_session(&digest(1), at(NOW)).unwrap(), None);
    store.grant_operator(&account.code, at(NOW)).unwrap();
    assert_eq!(
        store.operator_session(&digest(1), at(NOW)).unwrap(),
        Some(OperatorSession {
            account: account.id,
            confirmed: false,
        })
    );
    assert!(
        store
            .conversation_with(&account.code, FeedbackId::START)
            .unwrap()
            .unwrap()
            .is_empty()
    );
}

/// What `docs/deploy.md` says to run on the database before going back to
/// a program that knows only version 7.
const ROLLBACK_TO_7: &str = "DROP TABLE feedback; DROP TABLE operators; \
                             ALTER TABLE sessions DROP COLUMN confirmed_at; \
                             PRAGMA user_version = 7;";

#[test]
fn the_documented_rollback_leaves_version_7_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let version_7 = new_file(&dir);
    {
        let conn = Connection::open(&version_7).unwrap();
        for sql in &MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    let rolled_back = dir.path().join("rolled-back.db");
    {
        let mut store = Store::open(&rolled_back, OpenMode::CreateIfMissing).unwrap();
        let account = register(&mut store, "a@qq.com", 1);
        store.grant_operator(&account.code, at(NOW)).unwrap();
        store
            .start_session(&session(&account, 2, NOW, 3600))
            .unwrap();
        store
            .confirm_session(&digest(2), &session(&account, 1, NOW, 3600))
            .unwrap();
        reply(
            &mut store,
            &account.code,
            &portal_core::feedback::MessageBody::parse("你好").unwrap(),
            at(NOW),
        )
        .unwrap();
    }
    // Back a version at a time, as `docs/deploy.md` says: what version 9
    // added goes before what version 8 did.
    let conn = Connection::open(&rolled_back).unwrap();
    conn.execute_batch(translations_schema::ROLLBACK_TO_8)
        .unwrap();
    conn.execute_batch(ROLLBACK_TO_7).unwrap();
    drop(conn);
    assert_eq!(schema(&rolled_back), schema(&version_7));
    let version: i64 = Connection::open(&rolled_back)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 7);
    // The program of today brings it forward again, keeping its sessions.
    let store = Store::open(&rolled_back, OpenMode::Existing).unwrap();
    assert!(
        store
            .session_account(&digest(1), at(NOW))
            .unwrap()
            .is_some()
    );
}
