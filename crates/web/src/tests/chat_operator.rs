//! The chat on the operator's page of one conversation: the script follows
//! the conversation and reads nothing for its owner, only a confirmed
//! operator follows it, and a reply held while the operator is revoked is
//! turned away.

use axum::http::StatusCode;
use portal_core::account::AccountCode;
use portal_store::OPERATOR_CONFIRM_SECS;
use serde_json::json;

use super::chat::{document, message, refusal, reply};
use super::operator::operator;
use super::support::{Browser, TestApp};

#[tokio::test]
async fn the_operators_page_follows_one_conversation_and_reads_nothing_for_its_owner() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (mut op, _) = operator(&app).await;
    user.get(&app, "/feedback").await;
    user.post(&app, "/feedback/messages", &[("body", "在吗")])
        .await;
    reply(&mut op, &app, code, "在").await;
    let path = format!("/operator/feedback/{code}/messages");
    let read = op.ask(&app, &format!("{path}?after=0")).await;
    assert_eq!(
        document(&read),
        json!({ "messages": [
            message(1, false, false, &code.to_string(), "在吗"),
            message(2, true, true, "运营者", "在"),
        ] })
    );
    assert_eq!(
        document(&op.ask(&app, &format!("{path}?after=1")).await),
        json!({ "messages": [message(2, true, true, "运营者", "在")] })
    );
    // The owner has not read the reply.
    assert_eq!(
        document(&user.ask(&app, "/feedback/unread").await),
        json!({ "unread": true })
    );
    let page = op.get(&app, &format!("/operator/feedback/{code}")).await;
    assert!(page.body.contains(&format!(
        "<div class=\"chat\" data-chat=\"page\" data-messages=\"{path}\" data-after=\"2\""
    )));
    // A reply that cannot be sent, with the reason.
    let refused = op.post(&app, &path, &[("body", " \n ")]).await;
    assert_eq!(
        refusal(&refused),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "请先写下回复。".to_owned()
        )
    );
    // No account, or no code at all.
    let nobody = AccountCode::from_bits(9);
    for path in [
        format!("/operator/feedback/{nobody}/messages?after=0"),
        "/operator/feedback/not-a-code/messages?after=0".to_owned(),
    ] {
        assert_eq!(op.ask(&app, &path).await.status, StatusCode::NOT_FOUND);
    }
    let unknown = op
        .post(
            &app,
            &format!("/operator/feedback/{nobody}/messages"),
            &[("body", "你好")],
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_a_confirmed_operator_follows_a_conversation() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    user.post(&app, "/feedback/messages", &[("body", "我的私事")])
        .await;
    let path = format!("/operator/feedback/{code}/messages");
    let read = format!("{path}?after=0");
    // Signed out, and signed in but no operator.
    let mut visitor = Browser::new([203, 0, 113, 1]);
    visitor.get(&app, "/").await;
    for (browser, status, location) in [
        (&mut visitor, StatusCode::SEE_OTHER, "/login?next=operator"),
        (&mut user, StatusCode::NOT_FOUND, ""),
    ] {
        for reply in [
            browser.ask(&app, &read).await,
            browser.post(&app, &path, &[("body", "冒充运营者")]).await,
        ] {
            assert_eq!((reply.status, reply.location()), (status, location));
            assert!(!reply.body.contains("我的私事"));
        }
    }
    // An operator whose confirmation is too old is asked for it again.
    let (mut op, _) = operator(&app).await;
    app.clock.advance(OPERATOR_CONFIRM_SECS).unwrap();
    for reply in [
        op.ask(&app, &read).await,
        op.post(&app, &path, &[("body", "过期的回复")]).await,
    ] {
        assert_eq!(reply.status, StatusCode::SEE_OTHER);
        assert_eq!(reply.location(), "/operator/confirm?next=feedback");
        assert!(!reply.body.contains("我的私事"));
    }
    let conversation = app
        .store(move |store| store.conversation_with(&code, portal_store::FeedbackId::START))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conversation.len(), 1);
}

#[tokio::test]
async fn a_held_reply_of_an_operator_since_revoked_is_turned_away() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    user.get(&app, "/feedback").await;
    let (op, op_code) = operator(&app).await;
    let (release, pending) = op
        .held_post(
            &app,
            &format!("/operator/feedback/{code}/messages"),
            &[("body", "晚到的回复")],
        )
        .await;
    app.store(move |store| store.revoke_operator(&op_code))
        .await
        .unwrap();
    release.send(()).unwrap();
    let refused = pending.await.unwrap();
    assert_eq!(refused.status, StatusCode::NOT_FOUND);
    assert_eq!(
        document(&user.ask(&app, "/feedback/unread").await),
        json!({ "unread": false })
    );
}
