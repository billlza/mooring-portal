//! `translate`: the host's translator is asked about each message of a
//! user's that waits, the newest first, and what it answers is recorded.
//! A stand-in answers on the socket, as the test says. What becomes of a
//! run when the translator fails is in `translate_failures`.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use portal_core::feedback::MessageBody;
use portal_core::time::UnixTime;
use portal_core::translation::{MAX_TRANSLATION_CHARS, PageLanguage};
use portal_store::{AsOperator, FeedbackId, NewSession, OpenMode, Store, TranslationId, Waiting};

use super::{Fixture, NOW};
use crate::translate::RECENT_SECS;

/// What the stand-in does with a question.
pub(super) enum Reply {
    /// Answers these bytes.
    Bytes(Vec<u8>),
    /// Takes the question and says nothing.
    Silence,
}

impl From<&str> for Reply {
    fn from(answer: &str) -> Self {
        Self::Bytes(answer.as_bytes().to_vec())
    }
}

/// The answer `ok` with `said`, as a translator writes it.
pub(super) fn ok(said: &str) -> Reply {
    Reply::from(format!("ok {}\n{said}", said.len()).as_str())
}

/// The answer that the translator has nothing to say.
pub(super) fn none() -> Reply {
    Reply::from("none\n")
}

/// A translator that answers as the test says, and keeps what it was
/// asked: each question's line and its text.
pub(super) struct StandIn {
    socket: PathBuf,
    asked: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl StandIn {
    pub(super) fn new(dir: &Path, answer: impl Fn(&str, &str) -> Reply + Send + 'static) -> Self {
        let socket = dir.join("translator.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (heard, stopped) = (Arc::clone(&asked), Arc::clone(&stop));
        let thread = thread::spawn(move || {
            // The connections it leaves unanswered, kept open.
            let mut silent = Vec::new();
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                // The question's line ends with the number of bytes of
                // the text after it; the asker goes on holding its end.
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let (line, length) = line.trim_end_matches('\n').rsplit_once(' ').unwrap();
                let mut text = vec![0; length.parse().unwrap()];
                reader.read_exact(&mut text).unwrap();
                let text = String::from_utf8(text).unwrap();
                heard.lock().unwrap().push((line.to_owned(), text.clone()));
                match answer(line, &text) {
                    // An answer too long is not read to its end: the rest
                    // of it has nowhere to go.
                    Reply::Bytes(bytes) => drop(stream.write_all(&bytes)),
                    Reply::Silence => silent.push(stream),
                }
            }
        });
        Self {
            socket,
            asked,
            stop,
            thread: Some(thread),
        }
    }

    pub(super) fn socket(&self) -> &str {
        self.socket.to_str().unwrap()
    }

    /// The questions asked so far, as `line|text`.
    pub(super) fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .map(|(line, text)| format!("{line}|{text}"))
            .collect()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the thread from its wait for a connection.
        drop(UnixStream::connect(&self.socket));
        if let Some(thread) = self.thread.take() {
            let joined = thread.join();
            // A test that fails already says why. Failing again here,
            // while it unwinds, would abort every test of the run.
            if !thread::panicking() {
                joined.unwrap();
            }
        }
    }
}

/// The language a detector would name for `text`, by its first character:
/// enough to tell the tests' messages apart.
fn language_of(text: &str) -> Option<&'static str> {
    match text.chars().next()? {
        'ぁ'..='ヿ' => Some("ja"),
        '一'..='鿿' => Some("zh"),
        'А'..='я' => Some("ru"),
        'A'..='z' => Some("en"),
        _ => None,
    }
}

/// A translator that knows every language but Russian, and writes a
/// translation as `[from>into] text`.
pub(super) fn fluent(line: &str, text: &str) -> Reply {
    let words: Vec<&str> = line.split(' ').collect();
    match words[..] {
        ["detect"] => language_of(text).map_or_else(none, ok),
        ["translate", "ru", _] => none(),
        ["translate", from, into] => ok(&format!("[{from}>{into}] {text}\n")),
        _ => Reply::from("what?\n"),
    }
}

pub(super) fn body(text: &str) -> MessageBody {
    MessageBody::parse(text).unwrap()
}

/// Sends `text` to the operator from the account signed in with `digest`.
pub(super) fn write(fixture: &Fixture, digest: &portal_core::auth::TokenDigest, text: &str) {
    write_at(fixture, digest, text, NOW);
}

/// [`write`], at the time `secs`.
fn write_at(fixture: &Fixture, digest: &portal_core::auth::TokenDigest, text: &str, secs: u64) {
    fixture
        .store()
        .send_feedback(digest, &body(text), UnixTime::from_secs(secs).unwrap())
        .unwrap();
}

/// The operator's `text` to the account `to`. The account signed in with
/// `session` becomes an operator for it, with a session that has just
/// confirmed its password in place of that one.
fn reply(
    fixture: &Fixture,
    session: &portal_core::auth::TokenDigest,
    to: &portal_core::account::AccountCode,
    text: &str,
) {
    let mut store = fixture.store();
    let now = UnixTime::from_secs(NOW).unwrap();
    let operator = store.session_account(session, now).unwrap().unwrap();
    store.grant_operator(&operator.code, now).unwrap();
    let confirmed = portal_core::auth::TokenDigest::from_bytes([0xEE; 32]);
    let replacement = NewSession {
        digest: confirmed,
        account: operator.id,
        created_at: now,
        expires_at: now.checked_add_secs(3600).unwrap(),
    };
    assert!(store.confirm_session(session, &replacement).unwrap());
    let sent = store
        .as_operator(&confirmed, now, |pages| pages.reply(to, &body(text)))
        .unwrap();
    assert!(matches!(sent, AsOperator::Done(())));
}

/// The translations into `language` of the account's messages, as
/// `message number: text`.
pub(super) fn translations(
    fixture: &Fixture,
    account: &portal_core::account::AccountCode,
    language: PageLanguage,
) -> Vec<String> {
    fixture
        .store()
        .conversation_translated(account, FeedbackId::START, language, TranslationId::START)
        .unwrap()
        .unwrap()
        .translations
        .into_iter()
        .map(|made| format!("{}: {}", made.message.get(), made.text))
        .collect()
}

/// The numbers of the messages the translator has not examined, however
/// long ago they were sent, the oldest first.
pub(super) fn waiting(fixture: &Fixture) -> Vec<u64> {
    fixture
        .store()
        .untranslated(&Waiting::since(UnixTime::EPOCH), 100)
        .unwrap()
        .into_iter()
        .rev()
        .map(|message| message.id.get())
        .collect()
}

#[test]
fn each_message_is_put_into_the_languages_of_the_pages_it_is_not_written_in() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), fluent);
    let (alice, alice_session) = fixture.signed_in("alice@qq.com", 1);
    let (bob, bob_session) = fixture.signed_in("bob@163.com", 2);
    write(&fixture, &alice_session, "リンクが開きません\n二行目");
    write(&fixture, &alice_session, "订阅链接打不开");
    write(&fixture, &bob_session, "The link is broken");
    reply(&fixture, &alice_session, &bob, "Please make a new link");
    let args = ["translate", "--translator", translator.socket()];
    let done = fixture.run(&args);
    assert_eq!(done.code, ExitCode::SUCCESS, "{}", done.err);
    assert_eq!(
        done.out,
        "examined 3 message(s) and wrote 4 translation(s)\n"
    );
    assert!(done.err.is_empty());
    // The newest message is asked about first. Japanese goes into English
    // and that English into Chinese; Chinese and English go straight into
    // each other. The text is handed over whole, its line break included.
    // The operator's own message is no one's to translate.
    assert_eq!(
        translator.asked(),
        [
            "detect|The link is broken",
            "translate en zh|The link is broken",
            "detect|订阅链接打不开",
            "translate zh en|订阅链接打不开",
            "detect|リンクが開きません\n二行目",
            "translate ja en|リンクが開きません\n二行目",
            "translate en zh|[ja>en] リンクが開きません\n二行目",
        ]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::Chinese),
        ["1: [en>zh] [ja>en] リンクが開きません\n二行目"]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::English),
        [
            "2: [zh>en] 订阅链接打不开",
            "1: [ja>en] リンクが開きません\n二行目",
        ]
    );
    assert_eq!(
        translations(&fixture, &bob, PageLanguage::Chinese),
        ["3: [en>zh] The link is broken"]
    );
    assert_eq!(
        translations(&fixture, &bob, PageLanguage::English),
        Vec::<String>::new()
    );
    // Nothing is asked twice.
    let again = fixture.run(&args);
    assert_eq!(again.code, ExitCode::SUCCESS, "{}", again.err);
    assert_eq!(again.out, "no message waits for the translator\n");
    assert_eq!(translator.asked().len(), 7);
}

#[test]
fn a_message_no_one_can_place_or_translate_stays_as_written() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), fluent);
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    write(&fixture, &session, "???");
    write(&fixture, &session, "Привет");
    let args = ["translate", "--translator", translator.socket()];
    let done = fixture.run(&args);
    assert_eq!(done.code, ExitCode::SUCCESS, "{}", done.err);
    assert_eq!(
        done.out,
        "examined 2 message(s) and wrote 0 translation(s)\n"
    );
    // With no English of the Russian, there is nothing to put into
    // Chinese.
    assert_eq!(
        translator.asked(),
        ["detect|Привет", "translate ru en|Привет", "detect|???"]
    );
    for language in PageLanguage::ALL {
        assert_eq!(
            translations(&fixture, &alice, language),
            Vec::<String>::new()
        );
    }
    // They were examined all the same: neither is asked about again.
    assert_eq!(waiting(&fixture), Vec::<u64>::new());
    assert_eq!(
        fixture.run(&args).out,
        "no message waits for the translator\n"
    );
    assert_eq!(translator.asked().len(), 3);
}

#[test]
fn a_translation_that_cannot_be_shown_is_not_kept() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), |line, text| match (line, text) {
        ("translate ja en", "あ") => ok(" \n"),
        ("translate ja en", "い") => ok("photo\u{202e}gpj.exe\n"),
        ("translate ja en", "う") => ok(&"w".repeat(MAX_TRANSLATION_CHARS + 1)),
        // Longer than any answer that is read to its end.
        ("translate ja en", "え") => ok(&"w".repeat(9 * MAX_TRANSLATION_CHARS)),
        ("translate en zh", "[ja>en] お") => ok(""),
        _ => fluent(line, text),
    });
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    for text in ["あ", "い", "う", "え", "お"] {
        write(&fixture, &session, text);
    }
    let done = fixture.run(&["translate", "--translator", translator.socket()]);
    assert_eq!(done.code, ExitCode::SUCCESS, "{}", done.err);
    assert_eq!(
        done.out,
        "examined 5 message(s) and wrote 1 translation(s); 5 translation(s) could not be shown \
         and were not kept\n"
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::English),
        ["5: [ja>en] お"]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::Chinese),
        Vec::<String>::new()
    );
    // Without English that can be shown, no Chinese is asked for.
    assert_eq!(
        translator
            .asked()
            .iter()
            .filter(|asked| asked.starts_with("translate en zh"))
            .count(),
        1
    );
    assert_eq!(waiting(&fixture), Vec::<u64>::new());
}

#[test]
fn what_is_written_during_a_run_is_examined_in_it() {
    let fixture = Fixture::new();
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    write(&fixture, &session, "first");
    write(&fixture, &session, "second");
    let db = fixture.db();
    // While the translator answers about the oldest message, and again
    // about the one written then, another is written.
    let translator = StandIn::new(fixture.dir.path(), move |line, text| {
        let then = match (line, text) {
            ("detect", "first") => Some("third"),
            ("detect", "third") => Some("fourth"),
            _ => None,
        };
        if let Some(then) = then {
            Store::open(&db, OpenMode::Existing)
                .unwrap()
                .send_feedback(&session, &body(then), UnixTime::from_secs(NOW).unwrap())
                .unwrap();
        }
        fluent(line, text)
    });
    let done = fixture.run(&["translate", "--translator", translator.socket()]);
    assert_eq!(done.code, ExitCode::SUCCESS, "{}", done.err);
    assert_eq!(
        done.out,
        "examined 4 message(s) and wrote 4 translation(s)\n"
    );
    // Each once, and none of those examined before is asked about again.
    assert_eq!(
        translator
            .asked()
            .iter()
            .filter_map(|asked| asked.strip_prefix("detect|"))
            .collect::<Vec<_>>(),
        ["second", "first", "third", "fourth"]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::Chinese).len(),
        4
    );
    assert_eq!(waiting(&fixture), Vec::<u64>::new());
}

#[test]
fn a_message_that_has_waited_too_long_stays_as_written() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), fluent);
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    // Written when there was no translator: one a second too long ago,
    // and one just in time.
    write_at(&fixture, &session, "long ago", NOW);
    write_at(&fixture, &session, "just in time", NOW + 1);
    fixture.clock.advance(RECENT_SECS + 1).unwrap();
    let done = fixture.run(&["translate", "--translator", translator.socket()]);
    assert_eq!(done.code, ExitCode::SUCCESS, "{}", done.err);
    assert_eq!(
        done.out,
        "examined 1 message(s) and wrote 1 translation(s)\n"
    );
    assert_eq!(
        translator.asked(),
        ["detect|just in time", "translate en zh|just in time"]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::Chinese),
        ["2: [en>zh] just in time"]
    );
    // It is not examined, and no later run asks about it.
    assert_eq!(waiting(&fixture), [1]);
    let again = fixture.run(&["translate", "--translator", translator.socket()]);
    assert_eq!(again.out, "no message waits for the translator\n");
    assert_eq!(translator.asked().len(), 2);
}

#[test]
fn a_message_deleted_while_it_is_examined_gets_no_translation() {
    let fixture = Fixture::new();
    let (alice, alice_session) = fixture.signed_in("alice@qq.com", 1);
    let (bob, bob_session) = fixture.signed_in("bob@163.com", 2);
    write(&fixture, &bob_session, "Hello");
    write(&fixture, &alice_session, "Goodbye");
    let db = fixture.db();
    // While the translator writes its answer about alice's message, that
    // account is deleted and bob writes: bob's message takes its number.
    let translator = StandIn::new(fixture.dir.path(), move |line, text| {
        if line == "translate en zh" && text == "Goodbye" {
            let mut store = Store::open(&db, OpenMode::Existing).unwrap();
            store.delete_account(&alice).unwrap();
            store
                .send_feedback(
                    &bob_session,
                    &body("Anyone?"),
                    UnixTime::from_secs(NOW).unwrap(),
                )
                .unwrap();
        }
        fluent(line, text)
    });
    let done = fixture.run(&["translate", "--translator", translator.socket()]);
    assert_eq!(done.code, ExitCode::SUCCESS, "{}", done.err);
    assert_eq!(
        done.out,
        "examined 1 message(s) and wrote 1 translation(s); 1 message(s) had been deleted or \
         examined meanwhile\n"
    );
    // Bob's new message has the number of alice's and none of its words.
    assert_eq!(
        translations(&fixture, &bob, PageLanguage::Chinese),
        ["1: [en>zh] Hello"]
    );
    assert_eq!(waiting(&fixture), [2]);
}
