//! Translations: what the host's translator is handed, what it records
//! of a message, and what the operator's page reads back. What the files
//! hold of them is in `translations_schema`.

use portal_core::feedback::MessageBody;
use portal_core::translation::{Language, PageLanguage, TranslatedText};

use super::*;
use crate::{
    Examined, FeedbackId, FeedbackMessage, MAX_KEPT, Translation, TranslationId, Untranslated,
    Waiting,
};

pub(super) fn body(text: &str) -> MessageBody {
    MessageBody::parse(text).unwrap()
}

/// An account signed in with session `byte`, which lasts two days.
pub(super) fn signed_in(store: &mut Store, address: &str, bits: u64, byte: u8) -> Account {
    let account = register(store, address, bits);
    store
        .start_session(&session(&account, byte, NOW, 2 * 86_400))
        .unwrap();
    account
}

/// The user's message number `id`, as the translator is handed it.
pub(super) fn waiting(id: u64, text: &str) -> Untranslated {
    Untranslated {
        id: FeedbackId::new(id),
        body: body(text),
    }
}

/// What the translator made of a message it found written in `language`.
pub(super) fn examined(language: Option<&str>, translations: &[(PageLanguage, &str)]) -> Examined {
    Examined {
        language: language.map(|code| Language::parse(code).unwrap()),
        translations: translations
            .iter()
            .map(|(language, text)| (*language, TranslatedText::parse(text).unwrap()))
            .collect(),
    }
}

/// Translation number `id` of message number `message`.
pub(super) fn translation(id: u64, message: u64, text: &str) -> Translation {
    Translation {
        id: TranslationId::new(id),
        message: FeedbackId::new(message),
        text: TranslatedText::parse(text).unwrap(),
    }
}

/// The translations into `language` of the messages of the account with
/// `code`, those numbered after `after`.
pub(super) fn translations(
    store: &mut Store,
    code: &AccountCode,
    language: PageLanguage,
    after: u64,
) -> Vec<Translation> {
    store
        .conversation_translated(code, FeedbackId::START, language, TranslationId::new(after))
        .unwrap()
        .unwrap()
        .translations
}

/// How many rows `table` holds.
pub(super) fn rows(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn the_translator_is_handed_each_users_message_once_newest_first() {
    let mut store = store();
    let first = signed_in(&mut store, "a@qq.com", 1, 1);
    signed_in(&mut store, "b@qq.com", 2, 2);
    store
        .send_feedback(&digest(1), &body("リンクが開きません"), at(NOW + 1))
        .unwrap();
    reply(
        &mut store,
        &first.code,
        &body("请重新生成链接"),
        at(NOW + 2),
    )
    .unwrap();
    store
        .send_feedback(&digest(2), &body("The link is broken"), at(NOW + 3))
        .unwrap();
    store
        .send_feedback(&digest(1), &body("谢谢"), at(NOW + 4))
        .unwrap();
    // The operator's own message is no one's to translate.
    let all = [
        waiting(4, "谢谢"),
        waiting(3, "The link is broken"),
        waiting(1, "リンクが開きません"),
    ];
    let every = Waiting::since(at(NOW));
    assert_eq!(store.untranslated(&every, 10).unwrap(), all);
    assert_eq!(store.untranslated(&every, 2).unwrap(), all[..2]);
    // Those below a number, those above one, and those between two.
    let between = |newer_than: u64, older_than: Option<u64>| Waiting {
        newer_than: FeedbackId::new(newer_than),
        older_than: older_than.map(FeedbackId::new),
        ..every
    };
    let handed = |which: &Waiting| store.untranslated(which, 10).unwrap();
    assert_eq!(handed(&between(0, Some(4))), all[1..]);
    assert_eq!(handed(&between(0, Some(1))), []);
    assert_eq!(handed(&between(0, Some(u64::MAX))), all);
    assert_eq!(handed(&between(1, None)), all[..2]);
    assert_eq!(handed(&between(4, None)), []);
    assert_eq!(handed(&between(u64::MAX, None)), []);
    assert_eq!(handed(&between(1, Some(4))), all[1..2]);
    // Those sent at a time or after it.
    assert_eq!(handed(&Waiting::since(at(NOW + 3))), all[..2]);
    assert_eq!(handed(&Waiting::since(at(NOW + 4))), all[..1]);
    assert_eq!(handed(&Waiting::since(at(NOW + 5))), []);
    assert_eq!(
        Waiting::since(at(NOW)),
        Waiting {
            newer_than: FeedbackId::START,
            older_than: None,
            since: at(NOW),
        }
    );

    let japanese = examined(
        Some("ja"),
        &[
            (PageLanguage::English, "The link does not open"),
            (PageLanguage::Chinese, "链接打不开"),
        ],
    );
    assert!(
        store
            .record_examined(&all[2], &japanese, at(NOW + 9))
            .unwrap()
    );
    assert_eq!(store.untranslated(&every, 10).unwrap(), all[..2]);
    // A message whose language no one could tell is examined all the same,
    // and has no translation.
    assert!(
        store
            .record_examined(&all[0], &examined(None, &[]), at(NOW + 10))
            .unwrap()
    );
    assert_eq!(store.untranslated(&every, 10).unwrap(), all[1..2]);
    let found: Vec<(i64, Option<String>, i64)> = store
        .conn
        .prepare("SELECT feedback_id, language, examined_at FROM feedback_examined ORDER BY 1")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        found,
        [
            (1, Some("ja".to_owned()), (NOW + 9).cast_signed()),
            (4, None, (NOW + 10).cast_signed()),
        ]
    );
    let made: Vec<i64> = store
        .conn
        .prepare("SELECT made_at FROM feedback_translations")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(made, [(NOW + 9).cast_signed(); 2]);
}

#[test]
fn the_operators_page_reads_the_translations_made_after_a_number() {
    let mut store = store();
    let first = signed_in(&mut store, "a@qq.com", 1, 1);
    let second = signed_in(&mut store, "b@qq.com", 2, 2);
    for (session, text) in [(1, "リンクが開きません"), (1, "谢谢"), (2, "Привет")]
    {
        store
            .send_feedback(&digest(session), &body(text), at(NOW + 1))
            .unwrap();
    }
    // Made in this order: the first account's first message, the second
    // account's, then the first account's second.
    for (message, text, language, translations) in [
        (
            1,
            "リンクが開きません",
            "ja",
            &[
                (PageLanguage::Chinese, "链接打不开"),
                (PageLanguage::English, "The link does not open"),
            ][..],
        ),
        (3, "Привет", "ru", &[(PageLanguage::Chinese, "你好")][..]),
        (2, "谢谢", "zh", &[(PageLanguage::English, "Thanks")][..]),
    ] {
        assert!(
            store
                .record_examined(
                    &waiting(message, text),
                    &examined(Some(language), translations),
                    at(NOW + 5)
                )
                .unwrap()
        );
    }
    let mut read =
        |code: &AccountCode, language, after| translations(&mut store, code, language, after);
    assert_eq!(
        read(&first.code, PageLanguage::Chinese, 0),
        [translation(1, 1, "链接打不开")]
    );
    assert_eq!(
        read(&first.code, PageLanguage::English, 0),
        [
            translation(2, 1, "The link does not open"),
            translation(4, 2, "Thanks"),
        ]
    );
    assert_eq!(
        read(&first.code, PageLanguage::English, 2),
        [translation(4, 2, "Thanks")]
    );
    assert_eq!(read(&first.code, PageLanguage::English, 4), []);
    assert_eq!(read(&first.code, PageLanguage::English, u64::MAX), []);
    // Each conversation has its own.
    assert_eq!(
        read(&second.code, PageLanguage::Chinese, 0),
        [translation(3, 3, "你好")]
    );
    assert_eq!(read(&second.code, PageLanguage::English, 0), []);
    // The page follows the messages by one number and the translations by
    // another: a translation made since is given with the messages
    // written since, whichever message it is of.
    let numbers = |messages: &[FeedbackMessage]| -> Vec<u64> {
        messages.iter().map(|message| message.id.get()).collect()
    };
    let since = store
        .conversation_translated(
            &first.code,
            FeedbackId::new(1),
            PageLanguage::English,
            TranslationId::new(2),
        )
        .unwrap()
        .unwrap();
    assert_eq!(numbers(&since.messages), [2]);
    assert_eq!(since.translations, [translation(4, 2, "Thanks")]);
    let late = store
        .conversation_translated(
            &first.code,
            FeedbackId::new(2),
            PageLanguage::English,
            TranslationId::START,
        )
        .unwrap()
        .unwrap();
    assert_eq!(numbers(&late.messages), [] as [u64; 0]);
    assert_eq!(late.translations.len(), 2);
    assert_eq!(
        store
            .conversation_translated(
                &AccountCode::from_bits(99),
                FeedbackId::START,
                PageLanguage::Chinese,
                TranslationId::START
            )
            .unwrap(),
        None
    );
    assert_eq!(TranslationId::START.get(), 0);
    assert_eq!(TranslationId::new(7).get(), 7);
}

#[test]
fn nothing_is_recorded_of_a_message_that_is_gone_examined_or_another() {
    let mut store = store();
    let first = signed_in(&mut store, "a@qq.com", 1, 1);
    signed_in(&mut store, "b@qq.com", 2, 2);
    let english = examined(Some("ja"), &[(PageLanguage::English, "Hello")]);
    reply(&mut store, &first.code, &body("你好"), at(NOW + 1)).unwrap();
    store
        .send_feedback(&digest(1), &body("こんにちは"), at(NOW + 2))
        .unwrap();
    // The operator's message, a message of another text, and a number no
    // row has.
    for other in [
        waiting(1, "你好"),
        waiting(2, "こんばんは"),
        waiting(3, "こんにちは"),
        waiting(u64::MAX, "こんにちは"),
    ] {
        assert!(
            !store
                .record_examined(&other, &english, at(NOW + 3))
                .unwrap(),
            "{other:?}"
        );
    }
    assert_eq!(rows(&store, "feedback_examined"), 0);
    assert_eq!(rows(&store, "feedback_translations"), 0);

    // Examined once: a second word on it changes nothing.
    let message = waiting(2, "こんにちは");
    assert!(
        store
            .record_examined(&message, &english, at(NOW + 3))
            .unwrap()
    );
    let chinese = examined(Some("ko"), &[(PageLanguage::Chinese, "你好")]);
    assert!(
        !store
            .record_examined(&message, &chinese, at(NOW + 4))
            .unwrap()
    );
    assert_eq!(
        translations(&mut store, &first.code, PageLanguage::English, 0),
        [translation(1, 2, "Hello")]
    );
    assert_eq!(rows(&store, "feedback_translations"), 1);

    // The latest message of all goes with its account while its
    // translation is being made, and the next message written takes its
    // number: the translation is not that message's.
    store
        .send_feedback(&digest(1), &body("さようなら"), at(NOW + 5))
        .unwrap();
    let every = Waiting::since(at(NOW));
    let handed = store.untranslated(&every, 10).unwrap();
    assert_eq!(handed, [waiting(3, "さようなら")]);
    store.delete_account(&first.code).unwrap();
    store
        .send_feedback(&digest(2), &body("Good morning"), at(NOW + 6))
        .unwrap();
    let taken = store.untranslated(&every, 10).unwrap();
    assert_eq!(taken, [waiting(1, "Good morning")]);
    store
        .send_feedback(&digest(2), &body("Still there?"), at(NOW + 7))
        .unwrap();
    store
        .send_feedback(&digest(2), &body("Hello?"), at(NOW + 8))
        .unwrap();
    assert_eq!(
        store.untranslated(&every, 1).unwrap(),
        [waiting(3, "Hello?")]
    );
    let goodbye = examined(Some("ja"), &[(PageLanguage::English, "Goodbye")]);
    assert!(
        !store
            .record_examined(&handed[0], &goodbye, at(NOW + 9))
            .unwrap()
    );
    assert_eq!(rows(&store, "feedback_examined"), 0);
    assert_eq!(rows(&store, "feedback_translations"), 0);
    assert_eq!(store.untranslated(&every, 10).unwrap().len(), 3);
}

#[test]
fn what_was_made_of_a_message_goes_with_it_and_its_number_is_not_used_again() {
    let mut store = store();
    let first = signed_in(&mut store, "a@qq.com", 1, 1);
    let second = signed_in(&mut store, "b@qq.com", 2, 2);
    for (session, text) in [(2, "Hello"), (1, "こんにちは")] {
        store
            .send_feedback(&digest(session), &body(text), at(NOW + 1))
            .unwrap();
    }
    let hello = examined(Some("en"), &[(PageLanguage::Chinese, "你好")]);
    let both = examined(
        Some("ja"),
        &[
            (PageLanguage::Chinese, "你好"),
            (PageLanguage::English, "Hello"),
        ],
    );
    assert!(
        store
            .record_examined(&waiting(1, "Hello"), &hello, at(NOW + 2))
            .unwrap()
    );
    assert!(
        store
            .record_examined(&waiting(2, "こんにちは"), &both, at(NOW + 2))
            .unwrap()
    );
    assert_eq!(rows(&store, "feedback_examined"), 2);
    assert_eq!(rows(&store, "feedback_translations"), 3);

    // With the account, the latest translations go; the next one made
    // does not take their numbers, which a page may have passed.
    store.delete_account(&first.code).unwrap();
    assert_eq!(rows(&store, "feedback_examined"), 1);
    assert_eq!(rows(&store, "feedback_translations"), 1);
    store
        .send_feedback(&digest(2), &body("Thanks"), at(NOW + 3))
        .unwrap();
    let thanks = examined(Some("en"), &[(PageLanguage::Chinese, "谢谢")]);
    assert!(
        store
            .record_examined(&waiting(2, "Thanks"), &thanks, at(NOW + 4))
            .unwrap()
    );
    assert_eq!(
        translations(&mut store, &second.code, PageLanguage::Chinese, 0),
        [translation(1, 1, "你好"), translation(4, 2, "谢谢")]
    );

    // A conversation keeps its latest messages only: the translations of
    // the one it drops go with it.
    for sent in 0..u64::from(MAX_KEPT) - 1 {
        reply(&mut store, &second.code, &body("好的"), at(NOW + 5 + sent)).unwrap();
    }
    assert_eq!(rows(&store, "feedback_examined"), 1);
    assert_eq!(
        translations(&mut store, &second.code, PageLanguage::Chinese, 0),
        [translation(4, 2, "谢谢")]
    );
}
