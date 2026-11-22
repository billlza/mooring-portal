//! Translations on the operator's page of one conversation: under a
//! message a user wrote stands the message in the page's language, where
//! the host's translator has written it; the page's script is told of
//! those written since; and the message's owner is shown none.

use axum::http::StatusCode;
use portal_core::account::AccountCode;
use portal_core::time::UnixTime;
use portal_core::translation::{Language, PageLanguage, TranslatedText};
use portal_store::{Examined, FeedbackId, Waiting};
use serde_json::json;

use super::chat::{document, message, reply};
use super::operator::operator;
use super::support::{Browser, NOW, TestApp};

/// A user's message in English, with markup.
const ENGLISH: &str = "Where is my <b>link</b>?\nIt is gone.";

/// [`ENGLISH`] as the translator wrote it in Chinese.
const CHINESE: &str = "我的<b>链接</b>在哪里？\n它不见了。";

/// What marks a translation on a Chinese page, and on an English one.
const MARK_ZH: &str = "机器翻译，可能有误";
const MARK_EN: &str = "Machine translation; it may be wrong";

/// Records what the host's translator made of the user's message number
/// `id`: that it is written in `language`, and `translations` of it.
async fn examine(app: &TestApp, id: u64, language: &str, translations: &[(PageLanguage, &str)]) {
    let examined = Examined {
        language: Some(Language::parse(language).unwrap()),
        translations: translations
            .iter()
            .map(|(language, text)| (*language, TranslatedText::parse(text).unwrap()))
            .collect(),
    };
    let only = Waiting {
        newer_than: FeedbackId::new(id - 1),
        older_than: Some(FeedbackId::new(id + 1)),
        since: UnixTime::EPOCH,
    };
    let recorded = app
        .store(move |store| {
            let waiting = store.untranslated(&only, 1).unwrap();
            store.record_examined(&waiting[0], &examined, UnixTime::from_secs(NOW).unwrap())
        })
        .await
        .unwrap();
    assert!(recorded);
}

/// The user's message `text`, sent by the script of the feedback page.
async fn write(user: &mut Browser, app: &TestApp, text: &str) {
    let sent = user
        .post(app, "/feedback/messages", &[("body", text)])
        .await;
    assert_eq!(sent.status, StatusCode::NO_CONTENT, "{}", sent.body);
}

/// A conversation of three messages: the user's [`ENGLISH`], the user's
/// thanks in Chinese, and the operator's reply. The translator has
/// written the first in Chinese (translation 1) and the second in English
/// (translation 2). The user's browser, the operator's, and the account.
async fn conversation(app: &TestApp) -> (Browser, Browser, AccountCode) {
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut op, _) = operator(app).await;
    user.get(app, "/feedback").await;
    write(&mut user, app, ENGLISH).await;
    write(&mut user, app, "谢谢").await;
    reply(&mut op, app, code, "不客气").await;
    examine(app, 1, "en", &[(PageLanguage::Chinese, CHINESE)]).await;
    examine(app, 2, "zh", &[(PageLanguage::English, "Much obliged")]).await;
    (user, op, code)
}

/// The line of message number `id` in the thread of `page`.
fn line(page: &str, id: u64) -> &str {
    let start = page.find(&format!(" data-id=\"{id}\">")).unwrap();
    let end = start + page[start..].find("</li>").unwrap();
    &page[start..end]
}

/// A translation as a page shows it under its message, marked `mark`.
fn shown(mark: &str, text: &str) -> String {
    format!(
        "<div class=\"message-translation\">\n      <p class=\"message-translated muted \
         small\">{mark}</p>\n      <p class=\"message-body\">{text}</p>\n    </div>"
    )
}

#[tokio::test]
async fn the_operators_page_shows_a_users_message_in_the_pages_language_too() {
    let app = TestApp::new();
    let (_, mut op, code) = conversation(&app).await;
    let path = format!("/operator/feedback/{code}");
    let page = op.get(&app, &path).await;
    assert_eq!(page.status, StatusCode::OK);
    // Under the message in English, the message in Chinese: as text, like
    // the message.
    let first = line(&page.body, 1);
    let written = first
        .find("<p class=\"message-body\">Where is my &#60;b&#62;link&#60;/b&#62;?\nIt is gone.</p>")
        .unwrap();
    let translated = first
        .find(&shown(
            MARK_ZH,
            "我的&#60;b&#62;链接&#60;/b&#62;在哪里？\n它不见了。",
        ))
        .unwrap();
    assert!(written < translated);
    // The message in Chinese is not written again, and the operator's own
    // never is.
    for id in [2, 3] {
        assert!(
            !line(&page.body, id).contains("message-translation"),
            "{id}"
        );
    }
    assert!(!page.body.contains("Much obliged"));
    // The page tells its script where it stands among the translations.
    assert!(page.body.contains(&format!(
        "data-messages=\"{path}/messages\" data-after=\"3\" data-translated=\"1\" \
         data-translation=\"{MARK_ZH}\""
    )));

    // A reply sent back with its error shows them as well.
    let refused = op.post(&app, &path, &[("body", " ")]).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(line(&refused.body, 1).contains("它不见了。"));

    // In English, the other message is the one written again.
    op.post(&app, "/lang", &[("to", "en"), ("back", "operator")])
        .await;
    let page = op.get(&app, &path).await;
    assert!(!line(&page.body, 1).contains("message-translation"));
    assert!(line(&page.body, 2).contains(&shown(MARK_EN, "Much obliged")));
    assert!(!page.body.contains("它不见了"));
    assert!(page.body.contains(&format!(
        "data-after=\"3\" data-translated=\"2\" data-translation=\"{MARK_EN}\""
    )));
}

#[tokio::test]
async fn the_operators_script_is_told_of_the_translations_made_since() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut op, _) = operator(&app).await;
    user.get(&app, "/feedback").await;
    write(&mut user, &app, ENGLISH).await;
    // A page shown before the translator has come by.
    let path = format!("/operator/feedback/{code}");
    let page = op.get(&app, &path).await;
    assert!(page.body.contains("data-after=\"1\" data-translated=\"0\""));
    assert!(!page.body.contains("message-translation"));
    let messages = format!("{path}/messages");
    let ask = async |op: &mut Browser, query: &str| {
        document(&op.ask(&app, &format!("{messages}?{query}")).await)
    };
    assert_eq!(
        ask(&mut op, "after=1&translated=0").await,
        json!({ "messages": [], "translations": [] })
    );
    // The translation of a message the page shows comes by itself.
    examine(&app, 1, "en", &[(PageLanguage::Chinese, CHINESE)]).await;
    assert_eq!(
        ask(&mut op, "after=1&translated=0").await,
        json!({ "messages": [], "translations": [
            { "id": 1, "message": 1, "body": CHINESE },
        ] })
    );
    // Messages written since come with theirs; a translation into another
    // language is not this page's.
    write(&mut user, &app, "谢谢").await;
    examine(&app, 2, "zh", &[(PageLanguage::English, "Much obliged")]).await;
    write(&mut user, &app, "Здравствуйте").await;
    examine(
        &app,
        3,
        "ru",
        &[
            (PageLanguage::Chinese, "您好"),
            (PageLanguage::English, "Hello"),
        ],
    )
    .await;
    let who = code.to_string();
    assert_eq!(
        ask(&mut op, "after=1&translated=1").await,
        json!({
            "messages": [
                message(2, false, false, &who, "谢谢"),
                message(3, false, false, &who, "Здравствуйте"),
            ],
            "translations": [{ "id": 3, "message": 3, "body": "您好" }],
        })
    );
    // After the last, and after any number, there is none.
    for translated in ["3", "18446744073709551615"] {
        assert_eq!(
            ask(&mut op, &format!("after=3&translated={translated}")).await,
            json!({ "messages": [], "translations": [] }),
            "{translated}"
        );
    }
    // A page shown before the site showed translations asks for none, and
    // is told of none.
    assert_eq!(
        ask(&mut op, "after=2").await,
        json!({ "messages": [message(3, false, false, &who, "Здравствуйте")] })
    );
    // On an English page, the English ones.
    op.post(&app, "/lang", &[("to", "en"), ("back", "operator")])
        .await;
    assert_eq!(
        ask(&mut op, "after=3&translated=0").await,
        json!({ "messages": [], "translations": [
            { "id": 2, "message": 2, "body": "Much obliged" },
            { "id": 4, "message": 3, "body": "Hello" },
        ] })
    );
    // A question that is not one, and an account that is none.
    for query in [
        "after=0&translated=",
        "after=0&translated=-1",
        "after=0&translated=one",
        "translated=0",
    ] {
        let refused = op.ask(&app, &format!("{messages}?{query}")).await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{query}");
        assert!(!refused.body.contains("Much obliged"), "{query}");
    }
    let nobody = AccountCode::from_bits(9);
    let unknown = op
        .ask(
            &app,
            &format!("/operator/feedback/{nobody}/messages?after=0&translated=0"),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_messages_owner_is_shown_no_translation_of_it() {
    let app = TestApp::new();
    let (mut user, _, code) = conversation(&app).await;
    for lang in ["zh", "en"] {
        user.post(&app, "/lang", &[("to", lang), ("back", "feedback")])
            .await;
        let page = user.get(&app, "/feedback").await;
        assert_eq!(page.status, StatusCode::OK);
        assert!(page.body.contains("It is gone."), "{lang}");
        for trace in [
            "message-translation",
            "data-translated",
            "data-translation",
            "它不见了",
            "Much obliged",
            MARK_ZH,
            MARK_EN,
        ] {
            assert!(!page.body.contains(trace), "{lang} {trace}");
        }
        // Nor is its script told of one, whatever it asks.
        let read = document(
            &user
                .ask(&app, "/feedback/messages?after=0&translated=0")
                .await,
        );
        let told: Vec<&String> = read.as_object().unwrap().keys().collect();
        assert_eq!(told, ["messages"], "{lang}");
        assert_eq!(read["messages"].as_array().unwrap().len(), 3, "{lang}");
    }
    // Only an operator's page reads them.
    let asked = user
        .ask(
            &app,
            &format!("/operator/feedback/{code}/messages?after=0&translated=0"),
        )
        .await;
    assert_eq!(asked.status, StatusCode::NOT_FOUND);
    assert!(!asked.body.contains("它不见了"));
    assert!(!asked.body.contains("Much obliged"));
}
