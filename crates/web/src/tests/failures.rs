//! When the database fails, pages say so: an error page, never an empty
//! state or a guess.

use axum::http::StatusCode;

use super::support::{Browser, PASSWORD, TestApp};

/// Breaks the database of `file` behind the portal's back.
fn break_database(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "DROP TABLE subscription_records; DROP TABLE bindings; DROP TABLE sessions; \
         DROP TABLE accounts;",
    )
    .unwrap();
}

#[tokio::test]
async fn signed_in_pages_report_a_broken_database() {
    let (app, file) = TestApp::on_disk();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    break_database(file.path());
    for path in ["/", "/account", "/subscription", "/account/password"] {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
        assert!(reply.body.contains("服务器出错了"), "{path}");
        assert!(!reply.body.contains("暂无订阅"), "{path}");
    }
}

#[tokio::test]
async fn forms_report_a_broken_database() {
    let (app, file) = TestApp::on_disk();
    let mut browser = Browser::new([203, 0, 113, 40]);
    browser.get(&app, "/").await;
    break_database(file.path());
    for (path, fields) in [
        (
            "/login",
            vec![("email", "alice@qq.com"), ("password", PASSWORD)],
        ),
        (
            "/register",
            vec![
                ("email", "alice@qq.com"),
                ("password", PASSWORD),
                ("confirm", PASSWORD),
            ],
        ),
    ] {
        let reply = browser.post(&app, path, &fields).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
    }
}

#[tokio::test]
async fn logging_out_and_changing_passwords_report_a_broken_database() {
    let (app, file) = TestApp::on_disk();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    let (mut other, _) = app.signed_up("bob@qq.com").await;
    // Drop only what each step needs after its session has been read.
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER no_logout BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .unwrap();
    let reply = browser.post(&app, "/logout", &[]).await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    conn.execute_batch(
        "CREATE TRIGGER no_change BEFORE UPDATE ON accounts BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .unwrap();
    let fields = [
        ("current", PASSWORD),
        ("password", "a whole new passphrase"),
        ("confirm", "a whole new passphrase"),
    ];
    let reply = other.post(&app, "/account/password", &fields).await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    // Nothing changed: the old password still logs in.
    conn.execute_batch("DROP TRIGGER no_logout; DROP TRIGGER no_change;")
        .unwrap();
    let mut third = Browser::new([203, 0, 113, 41]);
    assert_eq!(
        third.log_in(&app, "bob@qq.com").await.status,
        StatusCode::SEE_OTHER
    );
}
