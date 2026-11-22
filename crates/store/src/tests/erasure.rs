//! What deleting an account leaves in the database files. Unless secure
//! deletion zeroes it, SQLite leaves a deleted row's bytes in the page's
//! free space; and its write-ahead log keeps the earlier versions of every
//! page it changed until the log is emptied, while the server, which holds
//! the database open from its start, keeps the log in place.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use super::*;
use crate::{OpenMode, StoreError};

/// Two bound accounts, each with everything the portal keeps of one: a
/// session, a binding, a snapshot record, a CFM profile, a link, and a
/// conversation with the operator, its question translated.
fn two_accounts(store: &mut Store) -> (Account, Account) {
    let gone = register(store, "gone.away@qq.com", 1);
    let kept = register(store, "still.here@qq.com", 2);
    for (account, byte, id) in [(&gone, 0xA1, "pilotusera"), (&kept, 0xB2, "pilotuserb")] {
        store
            .start_session(&session(account, byte, NOW, 3600))
            .unwrap();
        store.bind(&account.code, &user(id), at(NOW)).unwrap();
        let [question, answer] = conversation(byte);
        store
            .send_feedback(&digest(byte), &message(&question), at(NOW))
            .unwrap();
        let handed = store
            .untranslated(&crate::Waiting::since(at(NOW)), 1)
            .unwrap();
        assert!(
            store
                .record_examined(&handed[0], &translated(byte), at(NOW))
                .unwrap()
        );
        reply(store, &account.code, &message(&answer), at(NOW)).unwrap();
    }
    store
        .import(
            &snapshot(NOW, &[("pilotusera", Some(7)), ("pilotuserb", Some(8))]),
            at(NOW),
        )
        .unwrap();
    store
        .import_profiles(&bundle(NOW, &["pilotusera", "pilotuserb"]), at(NOW))
        .unwrap();
    for (session, link) in [(0xA1, 0xC3), (0xB2, 0xD4)] {
        store
            .replace_subscription_link(&digest(session), &digest(link), at(NOW))
            .unwrap();
    }
    (gone, kept)
}

/// What the account signed in with session `byte` wrote to the operator,
/// and the answer.
fn conversation(byte: u8) -> [String; 2] {
    [format!("question {byte:02x}"), format!("answer {byte:02x}")]
}

fn message(text: &str) -> portal_core::feedback::MessageBody {
    portal_core::feedback::MessageBody::parse(text).unwrap()
}

/// What the translator wrote of that question, in Chinese.
fn translation(byte: u8) -> String {
    format!("translation {byte:02x}")
}

/// What the translator made of that question.
fn translated(byte: u8) -> crate::Examined {
    use portal_core::translation::{Language, PageLanguage, TranslatedText};
    crate::Examined {
        language: Some(Language::parse("ja").unwrap()),
        translations: [(
            PageLanguage::Chinese,
            TranslatedText::parse(&translation(byte)).unwrap(),
        )]
        .into(),
    }
}

/// What would show that `account` was here: its address and code, its
/// Mooring user (which its profile names too), its session's and link's
/// token digests, its conversation, and the translation of what it wrote.
fn traces(account: &Account, user: &str, session: u8, link: u8) -> [Vec<u8>; 8] {
    let [question, answer] = conversation(session);
    [
        account.email.as_str().as_bytes().to_vec(),
        account.code.to_string().into_bytes(),
        user.as_bytes().to_vec(),
        vec![session; 32],
        vec![link; 32],
        question.into_bytes(),
        answer.into_bytes(),
        translation(session).into_bytes(),
    ]
}

/// Which of the files that can hold rows hold `trace`: the database and
/// its write-ahead log. The `-shm` index holds page numbers only.
fn holders(path: &Path, trace: &[u8]) -> Vec<PathBuf> {
    let log = path.with_file_name("portal.db-wal");
    [path.to_owned(), log]
        .into_iter()
        .filter(|file| match fs::read(file) {
            Ok(bytes) => bytes.windows(trace.len()).any(|window| window == trace),
            // The last connection to close removes the log.
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => panic!("cannot read {}: {error}", file.display()),
        })
        .collect()
}

#[test]
fn a_deleted_or_banned_account_leaves_nothing_in_the_files() {
    let delete = |store: &mut Store, code: &AccountCode| store.delete_account(code).unwrap();
    let ban = |store: &mut Store, code: &AccountCode| {
        store.ban(code, &[], at(NOW + 5)).unwrap();
    };
    for remove in [&delete as &dyn Fn(&mut Store, &AccountCode), &ban] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portal.db");
        let mut server = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
        let (gone, kept) = two_accounts(&mut server);
        let mut operator = Store::open(&path, OpenMode::Existing).unwrap();
        remove(&mut operator, &gone.code);
        operator.erase_deleted().unwrap();
        for trace in traces(&gone, "pilotusera", 0xA1, 0xC3) {
            assert_eq!(
                holders(&path, &trace),
                Vec::<PathBuf>::new(),
                "{}",
                String::from_utf8_lossy(&trace)
            );
        }
        // The search finds what is still there.
        for trace in traces(&kept, "pilotuserb", 0xB2, 0xD4) {
            assert!(
                !holders(&path, &trace).is_empty(),
                "{}",
                String::from_utf8_lossy(&trace)
            );
        }
        drop(server);
    }
}

/// A connection to `path` on its own thread that begins a read, or with
/// `write` an immediate write transaction, and keeps it open until told to
/// end it. Returns once it has begun, with the sender that ends it.
fn hold(path: &Path, write: bool) -> (mpsc::Sender<()>, thread::JoinHandle<()>) {
    let path = path.to_owned();
    let (began, begun) = mpsc::channel();
    let (end, ended) = mpsc::channel::<()>();
    let holder = thread::spawn(move || {
        let mut conn = Connection::open(&path).unwrap();
        let behavior = if write {
            TransactionBehavior::Immediate
        } else {
            TransactionBehavior::Deferred
        };
        let tx = conn.transaction_with_behavior(behavior).unwrap();
        let _: i64 = tx
            .query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))
            .unwrap();
        began.send(()).unwrap();
        ended.recv().unwrap();
        tx.commit().unwrap();
    });
    begun.recv().unwrap();
    (end, holder)
}

#[test]
fn erasing_waits_for_a_reader_that_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    let mut store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
    // The read begins while the log holds pages, so it reads from the log,
    // and the log cannot be emptied until it ends.
    register(&mut store, "a@qq.com", 1);
    let (end, reader) = hold(&path, false);
    let ender = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        end.send(()).unwrap();
    });
    store.erase_deleted().unwrap();
    ender.join().unwrap();
    reader.join().unwrap();
    let log = fs::metadata(path.with_file_name("portal.db-wal")).unwrap();
    assert_eq!(log.len(), 0, "the log is truncated");
}

#[test]
fn erasing_reports_a_reader_or_writer_that_outlasts_the_wait() {
    // A reader keeps the log from being emptied; a writer keeps the
    // database from being rebuilt.
    for write in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portal.db");
        let mut store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
        register(&mut store, "a@qq.com", 1);
        let (end, holder) = hold(&path, write);
        assert!(
            matches!(store.erase_deleted(), Err(StoreError::EraseBusy)),
            "write: {write}"
        );
        end.send(()).unwrap();
        holder.join().unwrap();
        store.erase_deleted().unwrap();
    }
}

/// Changes the accounts table's only page in the file at `path`, which no
/// connection may have open.
fn edit_accounts_page(path: &Path, edit: impl FnOnce(&mut [u8])) {
    let mut file = fs::read(path).unwrap();
    let page_size = match u16::from_be_bytes([file[16], file[17]]) {
        1 => 65_536,
        size => usize::from(size),
    };
    let root: i64 = Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT rootpage FROM sqlite_schema WHERE name = 'accounts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let start = (usize::try_from(root).unwrap() - 1) * page_size;
    let page = &mut file[start..start + page_size];
    // A table leaf is type 13.
    assert_eq!(page[0], 13, "the accounts table fits in its root page");
    edit(page);
    fs::write(path, file).unwrap();
}

/// Writes `bytes` into the accounts page's unused space, between its cell
/// pointers and its cells, which SQLite never reads.
fn plant_in_unused_space(path: &Path, bytes: &[u8]) {
    edit_accounts_page(path, |page| {
        // The cell count is at 3 and the start of the cell content at 5,
        // after an eight-byte header.
        let cells = usize::from(u16::from_be_bytes([page[3], page[4]]));
        let content = usize::from(u16::from_be_bytes([page[5], page[6]]));
        let unused = 8 + 2 * cells..content;
        assert!(unused.len() > bytes.len() + 16, "{unused:?}");
        let at = unused.start + 8;
        page[at..at + bytes.len()].copy_from_slice(bytes);
    });
}

#[test]
fn erasing_removes_a_stale_copy_sqlite_left_in_unused_space() {
    // When SQLite rebuilds a page while rebalancing the tree, it leaves the
    // bytes of the cells that moved in the space the page no longer uses,
    // and deleting such a row later zeroes only its live cell. A review
    // found a whole deleted account in such space after a long, varied
    // history; this test plants the same thing directly.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    let mut store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
    register(&mut store, "kept@qq.com", 1);
    // The last connection to close folds the log into the database file.
    drop(store);
    let stale = b"gone.but.copied@qq.com";
    plant_in_unused_space(&path, stale);
    let mut server = Store::open(&path, OpenMode::Existing).unwrap();
    let mut operator = Store::open(&path, OpenMode::Existing).unwrap();
    assert_eq!(server.accounts().unwrap().len(), 1);
    assert_eq!(holders(&path, stale), vec![path.clone()]);
    operator.erase_deleted().unwrap();
    assert_eq!(holders(&path, stale), Vec::<PathBuf>::new());
    assert!(!holders(&path, b"kept@qq.com").is_empty());
    register(&mut server, "after@qq.com", 2);
    assert_eq!(server.accounts().unwrap().len(), 2);
}

#[test]
fn erasing_a_corrupt_database_reports_the_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("portal.db");
    let mut store = Store::open(&path, OpenMode::CreateIfMissing).unwrap();
    register(&mut store, "kept@qq.com", 1);
    drop(store);
    // No page has type 0: the rebuild cannot read the table.
    edit_accounts_page(&path, |page| page[0] = 0);
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let error = store.erase_deleted().unwrap_err();
    assert!(
        matches!(&error, StoreError::Sqlite(error) if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseCorrupt)),
        "{error}"
    );
}
