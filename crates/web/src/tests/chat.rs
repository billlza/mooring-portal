//! The chat as a signed-in user has it: the requests the chat script
//! makes for the user's conversation, in the box and on the feedback page.
//! Each passes the checks its page's do, a page shown before anyone signed
//! in or out in the browser reads nothing more, and a message stored tells
//! the host that one waits.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, ORIGIN};
use portal_core::account::AccountCode;
use portal_core::feedback::MAX_MESSAGE_CHARS;
use portal_store::MAX_SENT_PER_DAY;
use serde_json::{Value, json};

use super::logs::{capture_logs, logged};
use super::operator::operator;
use super::support::{Browser, NOW, ORIGIN_URL, Reply, TestApp};

/// The JSON document `reply` holds, which it says it is.
pub(super) fn document(reply: &Reply) -> Value {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.headers[CONTENT_TYPE], "application/json");
    serde_json::from_str(&reply.body).unwrap()
}

/// The reason a refused message's `reply` gives, with its status.
pub(super) fn refusal(reply: &Reply) -> (StatusCode, String) {
    assert_eq!(reply.headers[CONTENT_TYPE], "application/json");
    let document: Value = serde_json::from_str(&reply.body).unwrap();
    (reply.status, document["error"].as_str().unwrap().to_owned())
}

/// A message as the script is sent it.
pub(super) fn message(id: u64, mine: bool, from_operator: bool, who: &str, body: &str) -> Value {
    json!({
        "id": id,
        "from_operator": from_operator,
        "mine": mine,
        "who": who,
        "sent": "2026-11-05 20:00",
        "body": body,
    })
}

/// The operator's reply to the account with `code`, sent by the script of
/// its page.
pub(super) async fn reply(op: &mut Browser, app: &TestApp, code: AccountCode, text: &str) {
    let sent = op
        .post(
            app,
            &format!("/operator/feedback/{code}/messages"),
            &[("body", text)],
        )
        .await;
    assert_eq!(sent.status, StatusCode::NO_CONTENT, "{}", sent.body);
}

#[tokio::test]
async fn the_box_and_the_feedback_page_follow_the_conversation() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut op, _) = operator(&app).await;
    user.get(&app, "/account").await;
    let sent = user
        .post(&app, "/feedback/messages", &[("body", "链接打不开")])
        .await;
    assert_eq!(sent.status, StatusCode::NO_CONTENT, "{}", sent.body);
    assert!(sent.body.is_empty());
    reply(&mut op, &app, code, "请重新生成链接").await;
    // Asking whether a reply waits marks nothing, however often.
    for _ in 0..2 {
        let unread = user.ask(&app, "/feedback/unread").await;
        assert_eq!(document(&unread), json!({ "unread": true }));
    }
    assert!(
        user.get(&app, "/account")
            .await
            .body
            .contains("<span class=\"dock-unread\">有新回复</span>")
    );
    // The messages after the one a page shows, oldest first, as its owner
    // reads them; asking for them reads every reply.
    let all = user.ask(&app, "/feedback/messages?after=0").await;
    assert_eq!(
        document(&all),
        json!({ "messages": [
            message(1, true, false, "你", "链接打不开"),
            message(2, false, true, "运营者", "请重新生成链接"),
        ] })
    );
    assert_eq!(
        document(&user.ask(&app, "/feedback/unread").await),
        json!({ "unread": false })
    );
    assert_eq!(
        document(&user.ask(&app, "/feedback/messages?after=1").await),
        json!({ "messages": [message(2, false, true, "运营者", "请重新生成链接")] })
    );
    for after in ["2", "18446744073709551615"] {
        assert_eq!(
            document(
                &user
                    .ask(&app, &format!("/feedback/messages?after={after}"))
                    .await
            ),
            json!({ "messages": [] }),
            "{after}"
        );
    }
    // The feedback page tells its script where it stands.
    let page = user.get(&app, "/feedback").await;
    assert!(page.body.contains(
        "<div class=\"chat\" data-chat=\"page\" data-messages=\"/feedback/messages\" \
         data-after=\"2\""
    ));
    // In English, the names are English.
    user.post(&app, "/lang", &[("to", "en"), ("back", "feedback")])
        .await;
    assert_eq!(
        document(&user.ask(&app, "/feedback/messages?after=1").await)["messages"][0]["who"],
        "Operator"
    );
}

#[tokio::test]
async fn the_script_writes_what_it_is_sent_and_is_sent_text() {
    let app = TestApp::new();
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    let text = "<script>alert(1)</script> & \"<b>\"\n第二行";
    let sent = user
        .post(&app, "/feedback/messages", &[("body", text)])
        .await;
    assert_eq!(sent.status, StatusCode::NO_CONTENT);
    let messages = document(&user.ask(&app, "/feedback/messages?after=0").await);
    assert_eq!(messages["messages"][0]["body"], text);
}

#[tokio::test]
async fn a_message_the_script_cannot_send_comes_back_with_the_reason() {
    let app = TestApp::new();
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    let long = "字".repeat(MAX_MESSAGE_CHARS + 1);
    for (text, reason) in [
        ("  \r\n\t ", "请先写下你的问题。"),
        (long.as_str(), "消息太长了，最多 1000 字。"),
        ("颜色\u{1b}[31m", "消息里有无法显示的字符，请删掉后再发。"),
    ] {
        let refused = user
            .post(&app, "/feedback/messages", &[("body", text)])
            .await;
        assert_eq!(
            refusal(&refused),
            (StatusCode::UNPROCESSABLE_ENTITY, reason.to_owned())
        );
    }
    for n in 0..MAX_SENT_PER_DAY {
        let sent = user
            .post(
                &app,
                "/feedback/messages",
                &[("body", &format!("第 {n} 条"))],
            )
            .await;
        assert_eq!(sent.status, StatusCode::NO_CONTENT, "{n}");
    }
    let refused = user
        .post(&app, "/feedback/messages", &[("body", "再一条")])
        .await;
    assert_eq!(
        refusal(&refused),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "24 小时内最多发 20 条消息，请稍后再发。".to_owned()
        )
    );
    let kept = document(&user.ask(&app, "/feedback/messages?after=0").await);
    assert_eq!(
        kept["messages"].as_array().unwrap().len(),
        usize::try_from(MAX_SENT_PER_DAY).unwrap()
    );
}

#[tokio::test]
async fn a_signed_out_script_is_told_so_and_reads_nothing() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    user.post(&app, "/feedback/messages", &[("body", "我的私事")])
        .await;
    let (mut op, _) = operator(&app).await;
    reply(&mut op, &app, code, "运营者的答复").await;
    let mut visitor = Browser::new([203, 0, 113, 1]);
    visitor.get(&app, "/").await;
    // None at all, and a session that has ended.
    app.clock.advance(crate::SESSION_TTL_SECS).unwrap();
    for browser in [&mut visitor, &mut user] {
        for path in ["/feedback/messages?after=0", "/feedback/unread"] {
            let refused = browser.ask(&app, path).await;
            assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{path}");
            assert!(refused.body.is_empty(), "{path}");
        }
        let refused = browser
            .post(&app, "/feedback/messages", &[("body", "你好")])
            .await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
        assert!(refused.body.is_empty());
    }
    // A blank message is refused for being blank, which tells nothing of
    // anyone's conversation, before anything is looked up.
    let refused = visitor
        .post(&app, "/feedback/messages", &[("body", " ")])
        .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    let refused = user
        .post(&app, "/feedback/messages", &[("body", " ")])
        .await;
    assert_eq!(refusal(&refused).0, StatusCode::UNPROCESSABLE_ENTITY);
    let conversation = app
        .store(move |store| store.conversation_with(&code, portal_store::FeedbackId::START))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conversation.len(), 2);
}

#[tokio::test]
async fn the_scripts_requests_pass_the_checks_a_form_does() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    // A form from elsewhere, and one without the page's token.
    let token = user.cookies["__Host-csrf"].clone();
    for (origin, form) in [
        (
            "https://evil.example",
            format!("csrf={token}&body=%E4%BC%AA%E9%80%A0%E7%9A%84"),
        ),
        (ORIGIN_URL, "body=%E4%BC%AA%E9%80%A0%E7%9A%84".to_owned()),
    ] {
        let request = user
            .request("POST", "/feedback/messages")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ORIGIN, origin)
            .body(Body::from(form))
            .unwrap();
        let forged = app.send(request).await;
        assert_eq!(forged.status, StatusCode::FORBIDDEN, "{origin}");
    }
    // A question that is not one.
    for path in [
        "/feedback/messages",
        "/feedback/messages?after=",
        "/feedback/messages?after=-1",
        "/feedback/messages?after=one",
        &format!("/operator/feedback/{code}/messages?after=x"),
    ] {
        let refused = user.ask(&app, path).await;
        assert_ne!(refused.status, StatusCode::OK, "{path}");
        assert!(!refused.body.contains("伪造的"), "{path}");
    }
    let messages = document(&user.ask(&app, "/feedback/messages?after=0").await);
    assert_eq!(messages, json!({ "messages": [] }));
}

/// `path` asked with `token` in the page-token header, or with none.
async fn ask_with(app: &TestApp, browser: &Browser, path: &str, token: Option<&str>) -> Reply {
    let mut request = browser.request("GET", path);
    if let Some(token) = token {
        request = request.header("x-csrf-token", token);
    }
    app.send(request.body(Body::empty()).unwrap()).await
}

#[tokio::test]
async fn a_question_without_its_pages_token_is_refused_and_marks_nothing() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    user.post(&app, "/feedback/messages", &[("body", "我的私事")])
        .await;
    let (mut op, _) = operator(&app).await;
    reply(&mut op, &app, code, "运营者的答复").await;
    // Another browser's token, one that is no token, and none: a link
    // followed to the address, or a page from another site, sends none.
    let mut other = Browser::new([203, 0, 113, 1]);
    other.get(&app, "/").await;
    let foreign = other.cookies["__Host-csrf"].clone();
    let operators = format!("/operator/feedback/{code}/messages?after=0");
    for token in [Some(foreign.as_str()), Some("not-a-token"), None] {
        for (browser, path) in [
            (&user, "/feedback/messages?after=0"),
            (&user, "/feedback/unread"),
            (&op, operators.as_str()),
        ] {
            let refused = ask_with(&app, browser, path, token).await;
            assert_eq!(refused.status, StatusCode::FORBIDDEN, "{path} {token:?}");
            assert!(!refused.body.contains("我的私事"), "{path} {token:?}");
            assert!(!refused.body.contains("运营者的答复"), "{path} {token:?}");
        }
    }
    // The reply is still unread.
    assert_eq!(
        document(&user.ask(&app, "/feedback/unread").await),
        json!({ "unread": true })
    );
}

#[tokio::test]
async fn a_page_shown_before_another_account_signed_in_reads_nothing_of_it() {
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("a@qq.com").await;
    browser.get(&app, "/feedback").await;
    let shown = browser.cookies["__Host-csrf"].clone();
    // Another account, with a reply it has not read.
    let (mut other, other_code) = app.signed_up("b@qq.com").await;
    other.get(&app, "/feedback").await;
    other
        .post(&app, "/feedback/messages", &[("body", "B 的私事")])
        .await;
    let (mut op, _) = operator(&app).await;
    reply(&mut op, &app, other_code, "给 B 的答复").await;
    // In the first browser, A signs out and B signs in, while the page
    // shown to A stays open.
    browser.post(&app, "/logout", &[]).await;
    browser.log_in(&app, "b@qq.com").await;
    for path in ["/feedback/messages?after=0", "/feedback/unread"] {
        let stale = ask_with(&app, &browser, path, Some(&shown)).await;
        assert_eq!(stale.status, StatusCode::FORBIDDEN, "{path}");
        assert!(!stale.body.contains("B 的私事"), "{path}");
        assert!(!stale.body.contains("给 B 的答复"), "{path}");
    }
    // B's reply is still unread, and a page shown to B now reads it.
    assert_eq!(
        document(&other.ask(&app, "/feedback/unread").await),
        json!({ "unread": true })
    );
    let read = document(&browser.ask(&app, "/feedback/messages?after=0").await);
    assert_eq!(read["messages"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn each_message_a_user_sends_tells_the_host_and_nothing_else_does() {
    let dir = tempfile::tempdir().unwrap();
    let signal = dir.path().join("new-message");
    let app = TestApp::signalling(&signal);
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut op, _) = operator(&app).await;
    user.get(&app, "/feedback").await;
    // Nothing yet: refused messages tell nothing.
    user.post(&app, "/feedback/messages", &[("body", " ")])
        .await;
    user.post(&app, "/feedback", &[("body", " ")]).await;
    assert!(!signal.exists());
    // A message sent from the box's form, and one from the script.
    let sent = user.post(&app, "/feedback", &[("body", "第一条")]).await;
    assert_eq!(sent.status, StatusCode::SEE_OTHER);
    assert_eq!(
        std::fs::read_to_string(&signal).unwrap(),
        format!("{NOW}\n")
    );
    app.clock.advance(60).unwrap();
    user.post(&app, "/feedback/messages", &[("body", "第二条")])
        .await;
    let written = std::fs::read_to_string(&signal).unwrap();
    assert_eq!(written, format!("{}\n", NOW + 60));
    // The operator's replies tell the host nothing.
    std::fs::remove_file(&signal).unwrap();
    reply(&mut op, &app, code, "收到").await;
    let replied = op
        .post(
            &app,
            &format!("/operator/feedback/{code}"),
            &[("body", "好")],
        )
        .await;
    assert_eq!(replied.status, StatusCode::SEE_OTHER);
    assert!(!signal.exists());
    // Nor does a message refused as one too many.
    for n in 2..MAX_SENT_PER_DAY {
        let sent = user
            .post(
                &app,
                "/feedback/messages",
                &[("body", &format!("第 {n} 条"))],
            )
            .await;
        assert_eq!(sent.status, StatusCode::NO_CONTENT, "{n}");
    }
    std::fs::remove_file(&signal).unwrap();
    let refused = user
        .post(&app, "/feedback/messages", &[("body", "又一条")])
        .await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(!signal.exists());
}

#[tokio::test]
async fn a_signal_that_cannot_be_written_is_logged_and_the_message_still_sent() {
    capture_logs();
    let dir = tempfile::tempdir().unwrap();
    let signal = dir.path().join("gone").join("new-message");
    let app = TestApp::signalling(&signal);
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    let sent = user
        .post(&app, "/feedback/messages", &[("body", "在吗")])
        .await;
    assert_eq!(sent.status, StatusCode::NO_CONTENT);
    let log = logged();
    assert!(
        log.contains("telling the host of a new message failed"),
        "{log}"
    );
    assert!(log.contains("ERROR"), "{log}");
    // What was written is not in the log.
    assert!(!log.contains("在吗"), "{log}");
    let kept = document(&user.ask(&app, "/feedback/messages?after=0").await);
    assert_eq!(kept["messages"][0]["body"], "在吗");
}
