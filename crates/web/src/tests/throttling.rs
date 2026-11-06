//! The limits and the hashing queue hold under load, every login costs one
//! hash whether or not its account exists, and every sign-up two, whether
//! its address is new, taken or banned.

use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use tokio::task::JoinSet;

use super::support::{Browser, PASSWORD, TestApp};
use crate::security::limits::PASSWORD_CHECKS_PER_EMAIL;

const NEW_PASSWORD: &str = "a whole new passphrase";

/// Logs in as `email` with a wrong password from `count` browsers on
/// different networks at once; the statuses, in no particular order.
async fn wrong_passwords_at_once(app: &TestApp, email: &'static str, count: u8) -> Vec<StatusCode> {
    let mut attempts = JoinSet::new();
    for index in 0..count {
        let app = app.clone();
        attempts.spawn(async move {
            let mut browser = Browser::new([10, 1, 0, index]);
            browser.get(&app, "/login").await;
            browser
                .post(
                    &app,
                    "/login",
                    &[("email", email), ("password", "wrong password here")],
                )
                .await
                .status
        });
    }
    let mut statuses = Vec::new();
    while let Some(status) = attempts.join_next().await {
        statuses.push(status.unwrap());
    }
    statuses
}

#[tokio::test]
async fn concurrent_guesses_cannot_outrun_the_per_email_limit() {
    let app = TestApp::new();
    app.signed_up("lilei@qq.com").await;
    let statuses = wrong_passwords_at_once(&app, "lilei@qq.com", 30).await;
    let checked = statuses
        .iter()
        .filter(|status| **status == StatusCode::UNAUTHORIZED)
        .count();
    let limited = statuses
        .iter()
        .filter(|status| **status == StatusCode::TOO_MANY_REQUESTS)
        .count();
    assert_eq!(
        checked,
        usize::try_from(PASSWORD_CHECKS_PER_EMAIL.limit).unwrap(),
        "{statuses:?}"
    );
    assert_eq!(checked + limited, 30, "{statuses:?}");
}

#[tokio::test]
async fn every_login_hashes_once_whether_or_not_the_account_exists() {
    let app = TestApp::new();
    app.signed_up("lilei@qq.com").await;
    let mut browser = Browser::new([203, 0, 113, 51]);
    browser.get(&app, "/login").await;
    for email in ["lilei@qq.com", "nobody@qq.com", "not an email"] {
        let before = app.state.hashing().finished();
        let reply = browser
            .post(
                &app,
                "/login",
                &[("email", email), ("password", "wrong password here")],
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{email}");
        assert_eq!(app.state.hashing().finished(), before + 1, "{email}");
    }
}

#[tokio::test]
async fn every_sign_up_hashes_twice_whether_its_address_is_new_taken_or_banned() {
    let app = TestApp::new();
    let (_, code) = app.signed_up("banned@qq.com").await;
    app.signed_up("taken@qq.com").await;
    let now = portal_core::time::UnixTime::from_secs(super::support::NOW).unwrap();
    app.store(move |store| store.ban(&code, now)).await.unwrap();
    for (last, email, status) in [
        (61, "new@qq.com", StatusCode::SEE_OTHER),
        (62, "taken@qq.com", StatusCode::CONFLICT),
        (63, "banned@qq.com", StatusCode::CONFLICT),
    ] {
        let mut browser = Browser::new([203, 0, 113, last]);
        browser.get(&app, "/register").await;
        let before = app.state.hashing().finished();
        let reply = browser
            .post(
                &app,
                "/register",
                &[
                    ("email", email),
                    ("password", PASSWORD),
                    ("confirm", PASSWORD),
                ],
            )
            .await;
        assert_eq!(reply.status, status, "{email}");
        // The password's hash and the address's ban digest, both in the
        // bounded pool, before the database says which case this is.
        assert_eq!(app.state.hashing().finished(), before + 2, "{email}");
    }
}

#[tokio::test]
async fn a_full_hashing_queue_refuses_at_once_and_costs_the_account_nothing() {
    let app = TestApp::new();
    let (mut owner, _) = app.signed_up("lilei@qq.com").await;
    let mut stranger = Browser::new([203, 0, 113, 52]);
    stranger.get(&app, "/").await;
    let held: Vec<_> = std::iter::from_fn(|| app.state.hashing().ticket().unwrap()).collect();
    assert!(!held.is_empty());
    let hashed = app.state.hashing().finished();
    for (browser, path, fields) in [
        (
            &mut stranger,
            "/login",
            vec![("email", "lilei@qq.com"), ("password", PASSWORD)],
        ),
        (
            &mut owner,
            "/account/password",
            vec![
                ("current", PASSWORD),
                ("password", NEW_PASSWORD),
                ("confirm", NEW_PASSWORD),
            ],
        ),
    ] {
        let reply = browser.post(&app, path, &fields).await;
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert!(reply.body.contains("服务器正忙"), "{path}");
        assert!(reply.headers.get(SET_COOKIE).is_none(), "{path}");
    }
    let mut newcomer = Browser::new([203, 0, 113, 53]);
    newcomer.get(&app, "/register").await;
    let reply = newcomer
        .post(
            &app,
            "/register",
            &[
                ("email", "hanmeimei@qq.com"),
                ("password", PASSWORD),
                ("confirm", PASSWORD),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(app.state.hashing().finished(), hashed, "nothing was hashed");
    drop(held);
    // Neither refusal counted against the email: all its checks are left.
    for _ in 1..PASSWORD_CHECKS_PER_EMAIL.limit {
        let reply = stranger
            .post(
                &app,
                "/login",
                &[
                    ("email", "lilei@qq.com"),
                    ("password", "wrong password here"),
                ],
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    }
    let reply = stranger
        .post(
            &app,
            "/login",
            &[("email", "lilei@qq.com"), ("password", PASSWORD)],
        )
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
}
