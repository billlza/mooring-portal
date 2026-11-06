//! Each account sees only its own data, by its own session only.

use axum::http::StatusCode;
use portal_core::auth::SecretToken;

use super::support::{TestApp, snapshot_json};
use crate::config::SESSION_TTL_SECS;

#[tokio::test]
async fn an_account_never_sees_another_accounts_data() {
    let app = TestApp::new();
    let (mut alice, alice_code) = app.signed_up("alice@qq.com").await;
    let (mut bob, bob_code) = app.signed_up("bob@163.com").await;
    app.bind(alice_code, "pilotusera").await;
    app.bind(bob_code, "pilotuserb").await;
    app.import(snapshot_json("pilotusera", Some(1_111_000_000)))
        .await;
    app.import(snapshot_json("pilotuserb", Some(2_222_000_000)))
        .await;

    let alice_account = alice.get(&app, "/account").await.body;
    assert!(
        alice_account.contains(&alice_code.to_string()) && alice_account.contains("alice@qq.com")
    );
    assert!(
        !alice_account.contains(&bob_code.to_string()) && !alice_account.contains("bob@163.com")
    );
    let alice_subscription = alice.get(&app, "/subscription").await.body;
    assert!(
        alice_subscription.contains("1.11 GB"),
        "{alice_subscription}"
    );
    assert!(!alice_subscription.contains("2.22 GB"));
    let bob_subscription = bob.get(&app, "/subscription").await.body;
    assert!(bob_subscription.contains("2.22 GB"));
    assert!(!bob_subscription.contains("1.11 GB"));
    // No address names an account: query strings are not consulted.
    let probe = alice
        .get(
            &app,
            &format!("/subscription?account={bob_code}&user=pilotuserb"),
        )
        .await;
    assert!(probe.body.contains("1.11 GB") && !probe.body.contains("2.22 GB"));
    // The Mooring user id itself is never shown.
    assert!(!alice_subscription.contains("pilotusera"));
}

#[tokio::test]
async fn forged_or_expired_sessions_are_anonymous() {
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    let mut forger = browser.clone();
    forger.cookies.insert(
        "__Host-session".to_owned(),
        SecretToken::generate().unwrap().encode(),
    );
    assert_eq!(
        forger.get(&app, "/account").await.status,
        StatusCode::SEE_OTHER
    );
    forger
        .cookies
        .insert("__Host-session".to_owned(), "not-a-token".to_owned());
    assert_eq!(
        forger.get(&app, "/subscription").await.status,
        StatusCode::SEE_OTHER
    );
    let home = forger.get(&app, "/").await;
    assert!(home.body.contains("登录 / 注册"));

    assert_eq!(browser.get(&app, "/account").await.status, StatusCode::OK);
    app.clock.advance(SESSION_TTL_SECS - 1).unwrap();
    assert_eq!(browser.get(&app, "/account").await.status, StatusCode::OK);
    app.clock.advance(1).unwrap();
    assert_eq!(
        browser.get(&app, "/account").await.status,
        StatusCode::SEE_OTHER,
        "expired"
    );
}

#[tokio::test]
async fn a_deleted_accounts_sessions_end_with_it() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.store(move |store| store.delete_account(&code))
        .await
        .unwrap();
    assert_eq!(
        browser.get(&app, "/account").await.status,
        StatusCode::SEE_OTHER
    );
    // The email may register again, as a new account with a new code.
    let (_, again) = app.signed_up("alice@qq.com").await;
    assert_ne!(again, code);
}
