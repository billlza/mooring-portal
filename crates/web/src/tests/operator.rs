//! The operator pages: only an account the host made an operator opens
//! them, and only from a session that confirmed the password lately.

use axum::http::StatusCode;
use portal_core::account::AccountCode;
use portal_core::time::UnixTime;
use portal_store::OPERATOR_CONFIRM_SECS;

use super::support::{Browser, NOW, PASSWORD, Reply, TestApp};

/// Every operator page a browser fetches, and the section each asks the
/// confirmation to return to.
const PAGES: [(&str, &str); 5] = [
    ("/operator", "overview"),
    ("/operator/feedback", "feedback"),
    ("/operator/accounts", "accounts"),
    ("/operator/bookings", "bookings"),
    ("/operator/payments", "payments"),
];

/// Every form of the operator pages but the confirmation, with fields a
/// page would send.
fn forms(target: &str) -> [(String, Vec<(&'static str, String)>); 6] {
    [
        (
            "/operator/accounts/bind".to_owned(),
            vec![
                ("account", target.to_owned()),
                ("user", "pilotuserz".to_owned()),
            ],
        ),
        (
            "/operator/accounts/unbind".to_owned(),
            vec![("account", target.to_owned())],
        ),
        (
            "/operator/accounts/ban".to_owned(),
            vec![("account", target.to_owned())],
        ),
        (
            "/operator/bookings/close".to_owned(),
            vec![("id", "1".to_owned())],
        ),
        (
            format!("/operator/feedback/{target}"),
            vec![("body", "你好".to_owned())],
        ),
        (
            "/operator/payments/approve".to_owned(),
            vec![
                ("order", "0123456789abcdefghjk".to_owned()),
                ("shown", "checking".to_owned()),
            ],
        ),
    ]
}

/// Makes the account with `code` an operator, as `grant-operator` does.
pub(super) async fn grant(app: &TestApp, code: AccountCode) {
    app.store(move |store| store.grant_operator(&code, UnixTime::from_secs(NOW).unwrap()))
        .await
        .unwrap();
}

/// Confirms `password` from `browser`, to go on to `next`.
pub(super) async fn confirm(
    browser: &mut Browser,
    app: &TestApp,
    password: &str,
    next: &str,
) -> Reply {
    browser
        .get(app, &format!("/operator/confirm?next={next}"))
        .await;
    browser
        .post(
            app,
            "/operator/confirm",
            &[("password", password), ("next", next)],
        )
        .await
}

/// An operator, signed in from a browser that confirmed the password.
pub(super) async fn operator(app: &TestApp) -> (Browser, AccountCode) {
    let (mut browser, code) = app.signed_up("op@qq.com").await;
    grant(app, code).await;
    let confirmed = confirm(&mut browser, app, PASSWORD, "overview").await;
    assert_eq!(
        confirmed.status,
        StatusCode::SEE_OTHER,
        "{}",
        confirmed.body
    );
    (browser, code)
}

async fn post_form(
    browser: &mut Browser,
    app: &TestApp,
    path: &str,
    fields: &[(&str, String)],
) -> Reply {
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    browser.post(app, path, &fields).await
}

/// Whether the account with `code` is still there.
async fn exists(app: &TestApp, code: AccountCode) -> bool {
    app.store(move |store| store.accounts())
        .await
        .unwrap()
        .iter()
        .any(|account| account.code == code)
}

#[tokio::test]
async fn a_signed_out_visitor_is_sent_to_log_in_first() {
    let app = TestApp::new();
    let (_, target) = app.signed_up("a@qq.com").await;
    let mut visitor = Browser::new([203, 0, 113, 1]);
    let conversation = format!("/operator/feedback/{target}");
    for path in PAGES
        .iter()
        .map(|(path, _)| *path)
        .chain([conversation.as_str(), "/operator/confirm"])
    {
        let reply = visitor.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(reply.location(), "/login?next=operator", "{path}");
    }
    for (path, fields) in forms(&target.to_string()).iter().chain([&(
        "/operator/confirm".to_owned(),
        vec![
            ("password", PASSWORD.to_owned()),
            ("next", "accounts".to_owned()),
        ],
    )]) {
        let reply = post_form(&mut visitor, &app, path, fields).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(reply.location(), "/login?next=operator", "{path}");
    }
    assert!(exists(&app, target).await);
    // Once logged in, an operator goes on to the pages.
    let login = visitor.get(&app, "/login?next=operator").await;
    assert!(
        login
            .body
            .contains("<input type=\"hidden\" name=\"next\" value=\"operator\">")
    );
}

#[tokio::test]
async fn an_account_that_is_no_operator_finds_no_operator_pages() {
    let app = TestApp::new();
    let (mut user, code) = app.signed_up("a@qq.com").await;
    let (_, target) = app.signed_up("b@qq.com").await;
    let conversation = format!("/operator/feedback/{target}");
    for path in PAGES
        .iter()
        .map(|(path, _)| *path)
        .chain([conversation.as_str(), "/operator/confirm"])
    {
        assert_eq!(
            user.get(&app, path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    for (path, fields) in forms(&target.to_string()) {
        let reply = post_form(&mut user, &app, &path, &fields).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
    }
    let confirmed = confirm(&mut user, &app, PASSWORD, "accounts").await;
    assert_eq!(confirmed.status, StatusCode::NOT_FOUND);
    assert!(exists(&app, target).await);
    assert!(exists(&app, code).await);
    let home = user.get(&app, "/").await;
    assert!(!home.body.contains("href=\"/operator\""));
}

#[tokio::test]
async fn the_pages_open_once_the_password_is_confirmed_and_for_half_an_hour() {
    let app = TestApp::new();
    let (mut op, code) = app.signed_up("op@qq.com").await;
    grant(&app, code).await;
    // The navigation offers the pages, with no feedback awaiting.
    let home = op.get(&app, "/").await;
    assert!(
        home.body
            .contains("<a href=\"/operator\" aria-current=\"false\">运营</a>")
    );
    // The operator answers feedback rather than sends it.
    assert!(!home.body.contains("<details class=\"dock\">"));
    let conversation = format!("/operator/feedback/{code}");
    for (path, next) in PAGES
        .into_iter()
        .chain([(conversation.as_str(), "feedback")])
    {
        let reply = op.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(reply.location(), format!("/operator/confirm?next={next}"));
    }
    let form = op.get(&app, "/operator/confirm?next=accounts").await;
    assert_eq!(form.status, StatusCode::OK);
    assert!(
        form.body
            .contains("为了安全，每 30 分钟要重新输入一次你的密码。")
    );
    assert!(
        form.body
            .contains("<input type=\"hidden\" name=\"next\" value=\"accounts\">")
    );
    let wrong = confirm(&mut op, &app, "not the password", "accounts").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.body.contains("密码不正确。"));
    assert_eq!(
        op.get(&app, "/operator/accounts").await.status,
        StatusCode::SEE_OTHER
    );
    let right = confirm(&mut op, &app, PASSWORD, "accounts").await;
    assert_eq!(right.status, StatusCode::SEE_OTHER, "{}", right.body);
    assert_eq!(right.location(), "/operator/accounts");
    for (path, _) in PAGES.into_iter().chain([(conversation.as_str(), "")]) {
        let page = op.get(&app, path).await;
        assert_eq!(page.status, StatusCode::OK, "{path}");
        assert!(
            page.body
                .contains("<nav class=\"tabs\" aria-label=\"运营\">")
        );
        assert!(
            page.body
                .contains("<a href=\"/operator\" aria-current=\"page\">运营</a>"),
            "{path}"
        );
    }
    app.clock.advance(OPERATOR_CONFIRM_SECS - 1).unwrap();
    assert_eq!(op.get(&app, "/operator").await.status, StatusCode::OK);
    app.clock.advance(1).unwrap();
    let expired = op.get(&app, "/operator/bookings").await;
    assert_eq!(expired.status, StatusCode::SEE_OTHER);
    assert_eq!(expired.location(), "/operator/confirm?next=bookings");
}

#[tokio::test]
async fn each_session_confirms_for_itself() {
    let app = TestApp::new();
    let (_, code) = operator(&app).await;
    let mut elsewhere = Browser::new([203, 0, 113, 9]);
    elsewhere.log_in(&app, "op@qq.com").await;
    let reply = elsewhere.get(&app, "/operator").await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), "/operator/confirm?next=overview");
    assert!(exists(&app, code).await);
}

#[tokio::test]
async fn wrong_passwords_are_limited_as_password_changes_are() {
    let app = TestApp::new();
    let (mut op, code) = app.signed_up("op@qq.com").await;
    grant(&app, code).await;
    for attempt in 0..10 {
        let wrong = confirm(&mut op, &app, "not the password", "overview").await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED, "{attempt}");
    }
    let limited = confirm(&mut op, &app, PASSWORD, "overview").await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.body.contains("尝试次数过多，请 15 分钟后再试。"));
    assert_eq!(
        op.get(&app, "/operator").await.status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn the_confirmation_returns_to_a_section_and_nowhere_else() {
    let app = TestApp::new();
    let (mut op, code) = app.signed_up("op@qq.com").await;
    grant(&app, code).await;
    let plain = op.get(&app, "/operator/confirm").await;
    assert!(
        plain
            .body
            .contains("<input type=\"hidden\" name=\"next\" value=\"overview\">")
    );
    for next in ["//elsewhere.example", "https://elsewhere.example/", "login"] {
        let page = op
            .get(&app, &format!("/operator/confirm?next={next}"))
            .await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST, "{next}");
        let posted = op
            .post(
                &app,
                "/operator/confirm",
                &[("password", PASSWORD), ("next", next)],
            )
            .await;
        assert_eq!(posted.status, StatusCode::BAD_REQUEST, "{next}");
    }
    for (path, next) in PAGES {
        let confirmed = confirm(&mut op, &app, PASSWORD, next).await;
        assert_eq!(confirmed.location(), path);
    }
}

#[tokio::test]
async fn an_operator_no_more_finds_no_operator_pages() {
    let app = TestApp::new();
    let (mut op, code) = operator(&app).await;
    assert_eq!(op.get(&app, "/operator").await.status, StatusCode::OK);
    app.store(move |store| store.revoke_operator(&code))
        .await
        .unwrap();
    assert_eq!(
        op.get(&app, "/operator").await.status,
        StatusCode::NOT_FOUND
    );
    assert!(!op.get(&app, "/").await.body.contains("href=\"/operator\""));
    // Made one again, it confirms again.
    grant(&app, code).await;
    let again = op.get(&app, "/operator").await;
    assert_eq!(again.status, StatusCode::SEE_OTHER);
    assert_eq!(again.location(), "/operator/confirm?next=overview");
}

/// The overview's tile for the release on the download page.
fn release_tile(body: &str) -> &str {
    let start = body
        .find("<a class=\"panel fact tile\" href=\"/download\">")
        .unwrap_or_else(|| panic!("no release tile: {body}"));
    let end = start + body[start..].find("</a>").unwrap();
    &body[start..end]
}

#[tokio::test]
async fn the_overview_says_when_the_download_page_was_last_checked() {
    const SHA: &str = "49d92ffcee6d6d794d4c4a7bc55faab3957601969b2a9a2cbc795a5d39847ba3";
    const HINT: &str = "<p class=\"small caution\">24 小时内没有核对成功。在主机上运行 \
                        journalctl -u mooring-portal-release.service 查看原因。</p>";
    let app = TestApp::new();
    let (mut op, _) = operator(&app).await;
    // Nothing imported yet: said, marked, and where to look.
    let body = op.get(&app, "/operator").await.body;
    let tile = release_tile(&body);
    assert!(
        tile.contains("<p class=\"label\">下载页上的 CFM</p>"),
        "{tile}"
    );
    assert!(
        tile.contains("<p class=\"value tone-warn\">还没有</p>"),
        "{tile}"
    );
    assert!(tile.contains(HINT), "{tile}");
    // Checked just now: the version and the time, unmarked.
    app.offer_release("0.5.1-build.51006", "2026-11-12T19:03:23Z", SHA)
        .await;
    let body = op.get(&app, "/operator").await.body;
    let tile = release_tile(&body);
    assert!(
        tile.contains("<p class=\"value\">0.5.1（build 51006）</p>"),
        "{tile}"
    );
    assert!(tile.contains("发布信息核对于 "), "{tile}");
    assert!(!tile.contains("caution"), "{tile}");
    // A day and a second without a check: marked, and where to look.
    app.clock.advance(24 * 3600 + 1).unwrap();
    let confirmed = confirm(&mut op, &app, PASSWORD, "overview").await;
    assert_eq!(confirmed.status, StatusCode::SEE_OTHER);
    let body = op.get(&app, "/operator").await.body;
    let tile = release_tile(&body);
    assert!(
        tile.contains("<p class=\"value tone-warn\">0.5.1（build 51006）</p>"),
        "{tile}"
    );
    assert!(tile.contains(HINT), "{tile}");
}
