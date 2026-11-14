//! When the database fails, pages say so: an error page, never an empty
//! state or a guess.

use axum::http::StatusCode;
use axum::http::header::CONTENT_SECURITY_POLICY;

use super::support::{Browser, ORIGIN_URL, PASSWORD, STRICT_POLICY, TestApp};

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
        // An error page runs no script, at the home page's address too.
        assert_eq!(
            reply.headers[CONTENT_SECURITY_POLICY], STRICT_POLICY,
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_download_page_reports_a_release_it_cannot_read() {
    // A release that cannot be read is an error, not "none imported yet".
    let (app, file) = TestApp::on_disk();
    app.offer_release(
        "0.5.1",
        "2026-11-04T18:00:00Z",
        "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f",
    )
    .await;
    let mut browser = Browser::new([203, 0, 113, 41]);
    assert_eq!(browser.get(&app, "/download").await.status, StatusCode::OK);
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute("UPDATE cfm_release SET version = 'latest'", [])
        .unwrap();
    let reply = browser.get(&app, "/download").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(reply.body.contains("服务器出错了"));
    assert!(!reply.body.contains("还没有取得") && !reply.body.contains(".dmg"));
    conn.execute_batch("DROP TABLE cfm_release").unwrap();
    assert_eq!(
        browser.get(&app, "/download").await.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
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

#[tokio::test]
async fn an_operators_action_whose_page_cannot_be_read_is_not_made() {
    let (app, file) = TestApp::on_disk();
    let (mut op, _) = super::operator::operator(&app).await;
    let (_, alice) = app.signed_up("alice@qq.com").await;
    app.bind(alice, "pilotusera").await;
    let (_, carol) = app.signed_up("carol@qq.com").await;
    let (_, broken) = app.signed_up("broken@qq.com").await;
    // One account's row reads as no account, and with it the list of
    // accounts every one of these actions' pages shows.
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    let break_row = |created_at: i64| {
        conn.execute(
            "UPDATE accounts SET created_at = ?1 WHERE code = ?2",
            rusqlite::params![created_at, broken.to_string()],
        )
        .unwrap();
    };
    let created_at: i64 = conn
        .query_row(
            "SELECT created_at FROM accounts WHERE code = ?1",
            [broken.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    break_row(-1);
    let (alice_code, carol_code) = (alice.to_string(), carol.to_string());
    for (path, fields) in [
        (
            "/operator/accounts/bind",
            vec![("account", carol_code.as_str()), ("user", "pilotuserc")],
        ),
        (
            "/operator/accounts/unbind",
            vec![("account", alice_code.as_str())],
        ),
        (
            "/operator/accounts/ban",
            vec![("account", alice_code.as_str())],
        ),
    ] {
        let failed = op.post(&app, path, &fields).await;
        assert_eq!(failed.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
    }
    // Nothing was done: what was done would have gone unsaid.
    break_row(created_at);
    assert_eq!(
        super::operating::binding(&app, alice).await,
        Some(Some("pilotusera".to_owned()))
    );
    assert_eq!(super::operating::binding(&app, carol).await, Some(None));
}

#[tokio::test]
async fn the_overview_says_of_a_release_it_cannot_read_and_shows_the_rest() {
    // The download page and the imports fail on such a row. The operator's
    // overview, where every confirmation lands, says so instead of failing
    // as well, and still shows what waits for the operator.
    let (app, file) = TestApp::on_disk();
    app.offer_release(
        "0.5.1",
        "2026-11-04T18:00:00Z",
        "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f",
    )
    .await;
    let (mut op, _) = super::operator::operator(&app).await;
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute("UPDATE cfm_release SET version = 'latest'", [])
        .unwrap();
    let reply = op.get(&app, "/operator").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let tile = super::operator::release_tile(&reply.body);
    assert!(
        tile.contains("<p class=\"value tone-warn\">读不出来</p>"),
        "{tile}"
    );
    assert!(tile.contains("docs/releases.md"), "{tile}");
    assert!(
        !tile.contains("还没有") && !tile.contains("0.5.1"),
        "{tile}"
    );
    assert!(reply.body.contains("等你回复的反馈") && reply.body.contains("等你处理的付款"));
    // A release table that is gone is no stored release that misreads: the
    // page fails, as every page does when the database does.
    conn.execute_batch("DROP TABLE cfm_release").unwrap();
    assert_eq!(
        op.get(&app, "/operator").await.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
