//! A link request whose session ends while its body is still arriving:
//! the link must not be made. The session is read when the request's
//! headers arrive; the body can come any time later.

use axum::body::{Body, Bytes};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, ORIGIN};
use tokio::sync::oneshot;

use super::support::{Browser, ORIGIN_URL, PASSWORD, Reply, TestApp};

/// Posts `browser`'s form to `path`, holding its body back: returns once
/// the portal asks for the body, with a sender that releases it and the
/// reply to come.
async fn held_post(
    app: &TestApp,
    browser: &Browser,
    path: &'static str,
) -> (oneshot::Sender<()>, tokio::task::JoinHandle<Reply>) {
    let (asked, asked_for_body) = oneshot::channel::<()>();
    let (release, released) = oneshot::channel::<()>();
    let form = format!("csrf={}", browser.cookies["__Host-csrf"]);
    let body = futures_util::stream::once(async move {
        asked.send(()).unwrap();
        released.await.unwrap();
        Ok::<_, std::io::Error>(Bytes::from(form))
    });
    let request = browser
        .request("POST", path)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ORIGIN, ORIGIN_URL)
        .header("sec-fetch-site", "same-origin")
        .body(Body::from_stream(body))
        .unwrap();
    let pending = tokio::spawn({
        let app = app.clone();
        async move { app.send(request).await }
    });
    asked_for_body.await.unwrap();
    (release, pending)
}

#[tokio::test]
async fn a_password_change_during_the_request_ends_it() {
    let app = TestApp::new();
    let (mut owner, code) = app.signed_up("lilei@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    // Someone else holds a copy of the owner's cookies.
    let thief = owner.clone();
    let (release, pending) = held_post(&app, &thief, "/subscription/link").await;
    let new_password = "a whole new passphrase";
    let changed = owner
        .post(
            &app,
            "/account/password",
            &[
                ("current", PASSWORD),
                ("password", new_password),
                ("confirm", new_password),
            ],
        )
        .await;
    assert_eq!(changed.location(), "/account?notice=password_changed");
    release.send(()).unwrap();
    let reply = pending.await.unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
    assert_eq!(reply.location(), "/login?next=subscription");
    assert!(!reply.body.contains("/sub/"));
    let page = owner.get(&app, "/subscription").await;
    assert!(page.body.contains("还没有订阅链接"));
}

#[tokio::test]
async fn a_deleted_accounts_request_cannot_reach_the_next_account_with_its_id() {
    let app = TestApp::new();
    let (_first, first_code) = app.signed_up("first@qq.com").await;
    let (attacker, attacker_code) = app.signed_up("attacker@qq.com").await;
    let (release, pending) = held_post(&app, &attacker, "/subscription/link").await;
    // The operator deletes the attacker's account, the newest one; the
    // next account to register gets its row id back.
    app.store(move |store| store.delete_account(&attacker_code))
        .await
        .unwrap();
    let (mut victim, victim_code) = app.signed_up("victim@qq.com").await;
    app.bind(victim_code, "pilotuserb").await;
    app.bind(first_code, "pilotusera").await;
    app.import_profiles(&["pilotusera", "pilotuserb"]).await;
    release.send(()).unwrap();
    let reply = pending.await.unwrap();
    assert_eq!(
        reply.location(),
        "/login?next=subscription",
        "{}",
        reply.body
    );
    assert!(!reply.body.contains("/sub/"));
    let page = victim.get(&app, "/subscription").await;
    assert!(page.body.contains("还没有订阅链接"));
}

#[tokio::test]
async fn a_request_whose_session_ended_cannot_turn_off_the_owners_new_link() {
    let app = TestApp::new();
    let (mut owner, code) = app.signed_up("lilei@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let thief = owner.clone();
    let (release, pending) = held_post(&app, &thief, "/subscription/link/revoke").await;
    let new_password = "a whole new passphrase";
    owner
        .post(
            &app,
            "/account/password",
            &[
                ("current", PASSWORD),
                ("password", new_password),
                ("confirm", new_password),
            ],
        )
        .await;
    let page = owner.post(&app, "/subscription/link", &[]).await;
    assert_eq!(page.status, StatusCode::OK);
    release.send(()).unwrap();
    let reply = pending.await.unwrap();
    assert_eq!(
        reply.location(),
        "/login?next=subscription",
        "{}",
        reply.body
    );
    let page = owner.get(&app, "/subscription").await;
    assert!(
        page.body.contains("当前链接生成于"),
        "the new link is still on"
    );
}
