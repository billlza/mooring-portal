//! Feedback: a signed-in user writes to the operator from any page and
//! reads the reply on the feedback page. Only the session names the
//! account, and what a user wrote is shown as text.

use axum::http::StatusCode;
use portal_core::account::AccountCode;
use portal_core::feedback::MAX_MESSAGE_CHARS;
use portal_core::time::UnixTime;
use portal_store::MAX_SENT_PER_DAY;

use super::support::{Browser, NOW, TestApp};
use crate::SESSION_TTL_SECS;

/// The box at the bottom right of the page.
const DOCK: &str = "<details class=\"dock\">";
/// Its form, and the page's.
const FORM: &str = "<form class=\"form\" method=\"post\" action=\"/feedback\">";

/// The operator's reply to the account with `code`, sent from the
/// operator pages.
async fn reply(app: &TestApp, code: AccountCode, text: &str) {
    let (mut op, _) = super::operator::operator(app).await;
    let sent = op
        .post(
            app,
            &format!("/operator/feedback/{code}"),
            &[("body", text)],
        )
        .await;
    assert_eq!(sent.status, StatusCode::SEE_OTHER, "{}", sent.body);
}

/// How many conversations the operator has.
async fn conversations(app: &TestApp) -> usize {
    app.store(|store| store.conversations())
        .await
        .unwrap()
        .len()
}

#[tokio::test]
async fn the_box_is_on_every_page_of_a_signed_in_user_but_the_conversation() {
    let app = TestApp::new();
    let mut visitor = Browser::new([203, 0, 113, 1]);
    for path in ["/", "/plans", "/mac", "/help", "/login"] {
        assert!(!visitor.get(&app, path).await.body.contains(DOCK), "{path}");
    }
    let (mut user, _) = app.signed_up("a@qq.com").await;
    for path in [
        "/",
        "/plans",
        "/download",
        "/mac",
        "/help",
        "/account",
        "/account/password",
        "/subscription",
    ] {
        let page = user.get(&app, path).await;
        assert!(page.body.contains(DOCK), "{path}");
        assert!(page.body.contains(FORM), "{path}");
        assert!(!page.body.contains("dock-unread"), "{path}");
    }
    let page = user.get(&app, "/feedback").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(!page.body.contains(DOCK));
    assert!(page.body.contains(FORM));
    assert!(page.body.contains("还没有消息。"));
    assert!(
        page.body
            .contains("<a href=\"/feedback\" aria-current=\"page\">反馈问题</a>")
    );
}

#[tokio::test]
async fn a_message_reaches_the_operator_and_the_reply_comes_back() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    user.get(&app, "/account").await;
    let sent = user
        .post(
            &app,
            "/feedback",
            &[("body", "  订阅链接打不开\r\n换了节点也不行  ")],
        )
        .await;
    assert_eq!(sent.status, StatusCode::SEE_OTHER, "{}", sent.body);
    assert_eq!(sent.location(), "/feedback?notice=sent");
    let page = user.get(&app, sent.location()).await;
    assert!(page.body.contains("已发送。运营者回复后会显示在这里。"));
    assert!(page.body.contains(
        "<li class=\"message mine\" data-id=\"1\">\n    <p class=\"message-meta\"><span \
         class=\"message-who\">你</span> <span class=\"muted small\">2026-11-05 20:00</span></p>\n    \
         <p class=\"message-body\">订阅链接打不开\n换了节点也不行</p>"
    ));
    let waiting = app.store(|store| store.conversations()).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].account, code);
    assert!(waiting[0].awaiting_reply);

    reply(&app, code, "请在订阅页重新生成链接").await;
    for path in ["/", "/account"] {
        let page = user.get(&app, path).await;
        assert!(
            page.body
                .contains("<span class=\"dock-unread\">有新回复</span>"),
            "{path}"
        );
    }
    let read = user.get(&app, "/feedback").await;
    assert!(read.body.contains(
        "<li class=\"message from-operator\" data-id=\"2\">\n    <p class=\"message-meta\"><span \
         class=\"message-who\">运营者</span>"
    ));
    assert!(read.body.contains("请在订阅页重新生成链接"));
    // Read now: no page tells of it any more.
    assert!(!read.body.contains("有新回复"));
    assert!(
        !user
            .get(&app, "/account")
            .await
            .body
            .contains("<span class=\"dock-unread\">")
    );
}

#[tokio::test]
async fn what_a_user_wrote_is_shown_as_text() {
    let app = TestApp::new();
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    user.post(
        &app,
        "/feedback",
        &[("body", "<script>alert(1)</script> & \"<b>\"")],
    )
    .await;
    let page = user.get(&app, "/feedback").await;
    assert!(!page.body.contains("<script>alert(1)"));
    assert!(
        page.body
            .contains("&#60;script&#62;alert(1)&#60;/script&#62; &#38; &#34;&#60;b&#62;&#34;"),
        "{}",
        page.body
    );
}

#[tokio::test]
async fn a_message_that_cannot_be_sent_comes_back_with_the_reason() {
    let app = TestApp::new();
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    let long = "字".repeat(MAX_MESSAGE_CHARS + 1);
    for (text, reason) in [
        ("  \r\n\t ", "请先写下你的问题。"),
        (long.as_str(), "消息太长了，最多 1000 字。"),
        ("颜色\u{1b}[31m", "消息里有无法显示的字符，请删掉后再发。"),
    ] {
        let refused = user.post(&app, "/feedback", &[("body", text)]).await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{reason}");
        assert!(
            refused
                .body
                .contains(&format!("<p class=\"alert\" role=\"alert\">{reason}</p>")),
            "{reason}"
        );
    }
    // What was written stays in the form.
    let refused = user.post(&app, "/feedback", &[("body", &long)]).await;
    assert!(
        refused
            .body
            .contains(&format!("required>{long}</textarea>"))
    );
    assert_eq!(conversations(&app).await, 0);
}

#[tokio::test]
async fn a_user_sends_a_bounded_number_of_messages_a_day() {
    let app = TestApp::new();
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    for n in 0..MAX_SENT_PER_DAY {
        let sent = user
            .post(&app, "/feedback", &[("body", &format!("第 {n} 条"))])
            .await;
        assert_eq!(sent.status, StatusCode::SEE_OTHER, "{n}");
    }
    let refused = user.post(&app, "/feedback", &[("body", "再一条")]).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        refused
            .body
            .contains("24 小时内最多发 20 条消息，请稍后再发。")
    );
    assert!(refused.body.contains("required>再一条</textarea>"));
    app.clock.advance(24 * 60 * 60).unwrap();
    let sent = user.post(&app, "/feedback", &[("body", "一天后")]).await;
    assert_eq!(sent.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_signed_out_visitor_is_sent_to_log_in_first() {
    let app = TestApp::new();
    let mut visitor = Browser::new([203, 0, 113, 1]);
    let page = visitor.get(&app, "/feedback").await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert_eq!(page.location(), "/login?next=feedback");
    let sent = visitor.post(&app, "/feedback", &[("body", "你好")]).await;
    assert_eq!(sent.status, StatusCode::SEE_OTHER);
    assert_eq!(sent.location(), "/login?next=feedback");
    // A session that ended while the message was being written.
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    app.clock.advance(SESSION_TTL_SECS).unwrap();
    for text in ["你好", " "] {
        let sent = user.post(&app, "/feedback", &[("body", text)]).await;
        assert_eq!(sent.status, StatusCode::SEE_OTHER, "{text:?}");
        assert_eq!(sent.location(), "/login?next=feedback");
    }
    assert_eq!(conversations(&app).await, 0);
}

#[tokio::test]
async fn the_longest_message_in_the_widest_characters_is_sent() {
    let app = TestApp::new();
    let (mut user, _) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    let longest = "😀".repeat(MAX_MESSAGE_CHARS);
    let sent = user.post(&app, "/feedback", &[("body", &longest)]).await;
    assert_eq!(sent.status, StatusCode::SEE_OTHER, "{}", sent.body);
    assert!(user.get(&app, "/feedback").await.body.contains(&longest));
}

#[tokio::test]
async fn a_held_message_of_an_account_since_banned_reads_no_one_elses_conversation() {
    let app = TestApp::new();
    app.signed_up("first@qq.com").await;
    let (attacker, attacker_code) = app.signed_up("attacker@qq.com").await;
    // A blank message: the page sends it back, with the conversation.
    let (release, pending) = attacker
        .held_post(&app, "/feedback", &[("body", " ")])
        .await;
    app.store(move |store| store.ban(&attacker_code, &[], UnixTime::from_secs(NOW).unwrap()))
        .await
        .unwrap();
    // The next account to register gets the banned one's row id back.
    let (mut victim, victim_code) = app.signed_up("victim@qq.com").await;
    victim.get(&app, "/feedback").await;
    victim
        .post(&app, "/feedback", &[("body", "我的订阅链接是私事")])
        .await;
    reply(&app, victim_code, "运营者给的答复").await;
    release.send(()).unwrap();
    let refused = pending.await.unwrap();
    assert_eq!(
        refused.location(),
        "/login?next=feedback",
        "{}",
        refused.body
    );
    for private in ["我的订阅链接是私事", "运营者给的答复"] {
        assert!(!refused.body.contains(private), "{private}");
    }
    // Nor did it read the victim's reply for them.
    assert!(
        victim
            .get(&app, "/account")
            .await
            .body
            .contains("<span class=\"dock-unread\">有新回复</span>")
    );
}
