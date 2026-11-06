//! What the portal logs: never a password, an email address, a token or a
//! cookie, and nothing a request chose.

use std::cell::RefCell;
use std::io;
use std::sync::Once;

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, ORIGIN};

use super::support::{Browser, ORIGIN_URL, PASSWORD, TestApp};

thread_local! {
    static LOGGED: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Appends to the calling thread's log.
struct ThreadLog;

impl io::Write for ThreadLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        LOGGED.with_borrow_mut(|log| log.extend_from_slice(bytes));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Sends every log of this test binary to the logging thread's own buffer,
/// and empties this thread's. A subscriber scoped to one test would miss
/// events: with a single subscriber registered, tracing decides once per
/// log statement whether anyone listens, by asking the subscriber of
/// whichever thread reaches it first, and that may be another test's.
fn capture_logs() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        tracing_subscriber::fmt()
            .with_writer(|| ThreadLog)
            .with_max_level(tracing::Level::TRACE)
            .try_init()
            .unwrap();
    });
    LOGGED.with_borrow_mut(Vec::clear);
}

/// What this thread has logged since [`capture_logs`].
fn logged() -> String {
    LOGGED.with_borrow(|log| String::from_utf8(log.clone()).unwrap())
}

/// A value a request chose, to look for in the log.
const CHOSEN: &str = "chosen-by-the-request";

#[tokio::test]
async fn logs_hold_no_secret_and_nothing_a_request_chose() {
    // The test runtime runs every handler on this thread.
    capture_logs();
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("lilei@qq.com").await;
    let session = browser.cookies["__Host-session"].clone();
    let csrf = browser.cookies["__Host-csrf"].clone();
    let new_password = "a whole new passphrase";
    browser
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
    browser.post(&app, "/logout", &[]).await;
    let mut stranger = Browser::new([203, 0, 113, 60]);
    stranger.get(&app, "/login").await;
    for password in ["wrong password here", new_password] {
        stranger
            .post(
                &app,
                "/login",
                &[("email", "lilei@qq.com"), ("password", password)],
            )
            .await;
    }
    // A malformed form, a forged one and a refused origin are logged too.
    let lang = stranger
        .post(&app, "/lang", &[("to", "zh"), ("back", CHOSEN)])
        .await;
    assert_eq!(lang.status, StatusCode::BAD_REQUEST);
    let forged = stranger
        .request("POST", "/logout")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ORIGIN, ORIGIN_URL)
        .body(Body::from(format!("csrf={CHOSEN}")))
        .unwrap();
    assert_eq!(app.send(forged).await.status, StatusCode::FORBIDDEN);
    let elsewhere = stranger
        .request("POST", "/logout")
        .header(ORIGIN, format!("https://{CHOSEN}.example"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.send(elsewhere).await.status, StatusCode::FORBIDDEN);
    let log = logged();
    for expected in [
        "registered an account",
        "refused a malformed request",
        "refused a state-changing request",
    ] {
        assert!(log.contains(expected), "{expected} is logged: {log}");
    }
    for secret in [
        PASSWORD,
        new_password,
        "wrong password here",
        "lilei@qq.com",
        "lilei",
        session.as_str(),
        csrf.as_str(),
        stranger.cookies["__Host-csrf"].as_str(),
        CHOSEN,
    ] {
        assert!(!log.contains(secret), "{secret} leaked: {log}");
    }
}
