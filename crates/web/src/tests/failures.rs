//! When the database fails, pages say so: an error page, never an empty
//! state or a guess.

use axum::http::StatusCode;

use super::support::{Browser, ORIGIN_URL, PASSWORD, TestApp};

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

#[tokio::test]
async fn getting_the_cfm_profile_reports_a_broken_database() {
    let (app, file) = TestApp::on_disk();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    app.bind(code, "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let page = browser.post(&app, "/subscription/link", &[]).await;
    let marker = format!("{ORIGIN_URL}/sub/");
    let start = page.body.find(&marker).unwrap() + marker.len();
    let link = format!("/sub/{}", &page.body[start..start + 64]);
    // The session still reads; the profiles and the links do not.
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "DROP TABLE cfm_profiles; \
         CREATE TRIGGER no_revoke BEFORE DELETE ON subscription_links \
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .unwrap();
    for path in ["/subscription", "/subscription/profile.json"] {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
    }
    let fetched = Browser::new([203, 0, 113, 42]).get(&app, &link).await;
    assert_eq!(fetched.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(fetched.body.starts_with("服务器出错了"), "{}", fetched.body);
    assert!(!fetched.body.contains("secret-of"));
    for path in ["/subscription/link", "/subscription/link/revoke"] {
        let reply = browser.post(&app, path, &[]).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
    }
}
