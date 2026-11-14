//! Getting the CFM profile: the panel, the download, the subscription link
//! and CFM's fetch of it.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_TYPE, ORIGIN, RETRY_AFTER,
    SET_COOKIE,
};
use portal_core::account::AccountCode;
use portal_core::auth::SecretToken;
use portal_core::profile::ProfileBundle;
use portal_core::time::UnixTime;

use super::support::{Browser, NOW, ORIGIN_URL, PASSWORD, Reply, TestApp, cfm_document};
use crate::security::limits::{FETCHES_PER_LINK, LINK_FETCHES_PER_NETWORK};

/// Where CFM fetches from in these tests, unless a test says otherwise.
const CFM: [u8; 4] = [203, 0, 113, 80];

/// Signs `email` up and binds the account to `user`.
async fn bound(app: &TestApp, email: &str, user: &str) -> (Browser, AccountCode) {
    let (browser, code) = app.signed_up(email).await;
    app.bind(code, user).await;
    (browser, code)
}

/// Makes `browser`'s link; the path CFM fetches (`/sub/<token>`).
async fn make_link(app: &TestApp, browser: &mut Browser) -> String {
    let page = browser.post(app, "/subscription/link", &[]).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    // Switching the language would lose the link, so the page offers none.
    assert!(!page.body.contains("action=\"/lang\""));
    let start = page
        .body
        .find(&format!("value=\"{ORIGIN_URL}/sub/"))
        .expect("the page shows the link");
    let url = &page.body[start + "value=\"".len()..];
    let url = &url[..url.find('"').unwrap()];
    url.strip_prefix(ORIGIN_URL).unwrap().to_owned()
}

/// CFM fetching `path`: no cookie, from `address`. Whatever the outcome,
/// the reply sets no cookie, is not cached, and is either the document or
/// plain text: an HTML page would send the address on as the referrer.
async fn fetch(app: &TestApp, path: &str, address: [u8; 4]) -> Reply {
    let reply = Browser::new(address).get(app, path).await;
    assert!(reply.headers.get(SET_COOKIE).is_none(), "{path}");
    assert_eq!(reply.headers[CACHE_CONTROL], "no-store", "{path}");
    if reply.status != StatusCode::OK {
        assert_eq!(
            reply.headers[CONTENT_TYPE], "text/plain; charset=utf-8",
            "{path}"
        );
        assert!(!reply.body.contains('<'), "{path}: {}", reply.body);
    }
    reply
}

/// `reply` is the document of `user`, as a file CFM can import.
fn is_document_of(reply: &Reply, user: &str) {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body, cfm_document(user));
    assert_eq!(reply.headers[CONTENT_TYPE], "application/json");
    assert_eq!(
        reply.headers[CONTENT_DISPOSITION],
        "attachment; filename=\"moorlight-cfm.json\""
    );
    assert_eq!(reply.headers[CACHE_CONTROL], "no-store");
    // CFM 0.5.1 refuses any encoding but identity.
    assert!(reply.headers.get(CONTENT_ENCODING).is_none());
}

#[tokio::test]
async fn only_a_bound_account_has_a_cfm_panel_and_it_waits_for_a_profile() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("lilei@qq.com").await;
    let page = browser.get(&app, "/subscription").await;
    assert!(!page.body.contains("在 CFM 中使用"));
    app.bind(code, "pilotusera").await;
    let page = browser.get(&app, "/subscription").await;
    assert!(page.body.contains("在 CFM 中使用"));
    assert!(page.body.contains("目前没有你的 CFM 配置。"));
    assert!(!page.body.contains("action=\"/subscription/link\""));
    assert!(!page.body.contains("/subscription/profile.json"));
    // Nothing to fetch, so neither a link nor a download.
    let refused = browser.post(&app, "/subscription/link", &[]).await;
    assert_eq!(refused.status, StatusCode::SEE_OTHER);
    assert_eq!(refused.location(), "/subscription?notice=link_unavailable");
    let page = browser.get(&app, refused.location()).await;
    assert!(
        page.body
            .contains("目前没有你的 CFM 配置，暂时不能生成订阅链接。")
    );
    let download = browser.get(&app, "/subscription/profile.json").await;
    assert_eq!(download.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn each_account_sees_when_its_own_profile_was_made() {
    let app = TestApp::new();
    let (mut lilei, lilei_code) = bound(&app, "lilei@qq.com", "pilotusera").await;
    let (mut hanmeimei, _) = bound(&app, "hanmeimei@163.com", "pilotuserb").await;
    app.import_profiles(&["pilotusera", "pilotuserb"]).await;
    // An hour later the operator imports a bundle for Li Lei alone.
    let later = NOW + 3600;
    let bundle = format!(
        "{{\"format\": \"mooring-portal-cfm-profiles\", \"version\": 2, \"generated_at\": \
         {later}, \"profiles\": [{{\"user\": \"pilotusera\", \"document\": {}}}], \
         \"withheld\": []}}",
        cfm_document("pilotusera")
    );
    let bundle = ProfileBundle::parse(bundle.as_bytes()).unwrap();
    let now = UnixTime::from_secs(later).unwrap();
    app.store(move |store| store.import_profile_for(&lilei_code, &bundle, now))
        .await
        .unwrap();
    let page = lilei.get(&app, "/subscription").await;
    assert!(
        page.body.contains("配置生成于 2026-11-05 21:00"),
        "{}",
        page.body
    );
    let page = hanmeimei.get(&app, "/subscription").await;
    assert!(
        page.body.contains("配置生成于 2026-11-05 20:00"),
        "{}",
        page.body
    );
}

#[tokio::test]
async fn the_profile_downloads_exactly_as_mooring_wrote_it() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let page = browser.get(&app, "/subscription").await;
    assert!(page.body.contains("配置生成于 2026-11-05 20:00"));
    assert!(page.body.contains("href=\"/subscription/profile.json\""));
    assert!(page.body.contains("生成订阅链接"));
    let download = browser.get(&app, "/subscription/profile.json").await;
    is_document_of(&download, "pilotusera");
}

#[tokio::test]
async fn a_link_is_shown_once_and_fetches_the_profile() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let path = make_link(&app, &mut browser).await;
    let token = path.strip_prefix("/sub/").unwrap();
    assert!(SecretToken::parse(token).is_ok(), "{token}");
    is_document_of(&fetch(&app, &path, CFM).await, "pilotusera");
    // The subscription page says when it was made, but never shows it.
    let page = browser.get(&app, "/subscription").await;
    assert!(page.body.contains("action=\"/lang\""));
    assert!(page.body.contains("当前链接生成于 2026-11-05 20:00"));
    assert!(page.body.contains("重新生成"));
    assert!(page.body.contains("停用链接"));
    assert!(!page.body.contains(token));
}

#[tokio::test]
async fn a_new_link_ends_the_old_one_and_revoking_ends_both() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let first = make_link(&app, &mut browser).await;
    let second = make_link(&app, &mut browser).await;
    assert_ne!(first, second);
    assert_eq!(fetch(&app, &first, CFM).await.status, StatusCode::NOT_FOUND);
    is_document_of(&fetch(&app, &second, CFM).await, "pilotusera");
    let revoked = browser.post(&app, "/subscription/link/revoke", &[]).await;
    assert_eq!(revoked.location(), "/subscription?notice=link_revoked");
    let page = browser.get(&app, revoked.location()).await;
    assert!(page.body.contains("订阅链接已停用。"));
    assert!(page.body.contains("生成订阅链接"));
    assert_eq!(
        fetch(&app, &second, CFM).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn each_account_reaches_only_its_own_profile() {
    let app = TestApp::new();
    let (mut lilei, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    let (mut hanmeimei, _) = bound(&app, "hanmeimei@qq.com", "pilotuserb").await;
    app.import_profiles(&["pilotusera", "pilotuserb"]).await;
    let lilei_link = make_link(&app, &mut lilei).await;
    let hanmeimei_link = make_link(&app, &mut hanmeimei).await;
    is_document_of(&fetch(&app, &lilei_link, CFM).await, "pilotusera");
    is_document_of(&fetch(&app, &hanmeimei_link, CFM).await, "pilotuserb");
    is_document_of(
        &hanmeimei.get(&app, "/subscription/profile.json").await,
        "pilotuserb",
    );
    // Revoking or replacing acts on the signed-in account's link alone.
    hanmeimei.post(&app, "/subscription/link/revoke", &[]).await;
    is_document_of(&fetch(&app, &lilei_link, CFM).await, "pilotusera");
    make_link(&app, &mut hanmeimei).await;
    is_document_of(&fetch(&app, &lilei_link, CFM).await, "pilotusera");
}

#[tokio::test]
async fn changing_the_password_turns_the_link_off() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let link = make_link(&app, &mut browser).await;
    let change = |current: &'static str, new: &'static str| {
        [("current", current), ("password", new), ("confirm", new)]
    };
    let changed = browser
        .post(
            &app,
            "/account/password",
            &change(PASSWORD, "a whole new passphrase"),
        )
        .await;
    assert_eq!(
        changed.location(),
        "/account?notice=password_changed_link_revoked"
    );
    let page = browser.get(&app, changed.location()).await;
    assert!(page.body.contains("你的订阅链接也已停用"));
    assert_eq!(fetch(&app, &link, CFM).await.status, StatusCode::NOT_FOUND);
    // Without a link, the usual notice.
    let changed = browser
        .post(
            &app,
            "/account/password",
            &change("a whole new passphrase", "yet another passphrase"),
        )
        .await;
    assert_eq!(changed.location(), "/account?notice=password_changed");
}

#[tokio::test]
async fn a_paused_link_fetches_nothing_and_works_again_once_delivered() {
    let app = TestApp::new();
    let (mut lilei, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let link = make_link(&app, &mut lilei).await;
    for reason in ["suspended", "agreement_not_accepted"] {
        app.import_bundle(&[], &[("pilotusera", reason)]).await;
        assert_eq!(
            fetch(&app, &link, CFM).await.status,
            StatusCode::NOT_FOUND,
            "{reason}"
        );
        let page = lilei.get(&app, "/subscription").await;
        assert!(page.body.contains("现在暂停，取不到配置"), "{reason}");
        assert!(
            page.body.contains("action=\"/subscription/link/revoke\""),
            "{reason}"
        );
        assert!(
            !page.body.contains("action=\"/subscription/link\""),
            "{reason}"
        );
        app.import_profiles(&["pilotusera"]).await;
        is_document_of(&fetch(&app, &link, CFM).await, "pilotusera");
    }
    // Revoked by its owner while paused, it stays gone once delivered again.
    app.import_bundle(&[], &[("pilotusera", "suspended")]).await;
    let revoked = lilei.post(&app, "/subscription/link/revoke", &[]).await;
    assert_eq!(revoked.location(), "/subscription?notice=link_revoked");
    app.import_profiles(&["pilotusera"]).await;
    assert_eq!(fetch(&app, &link, CFM).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unbinding_or_revoking_the_user_ends_the_link_for_good() {
    let app = TestApp::new();
    let (mut lilei, lilei_code) = bound(&app, "lilei@qq.com", "pilotusera").await;
    let (mut hanmeimei, _) = bound(&app, "hanmeimei@qq.com", "pilotuserb").await;
    app.import_profiles(&["pilotusera", "pilotuserb"]).await;
    let lilei_link = make_link(&app, &mut lilei).await;
    let hanmeimei_link = make_link(&app, &mut hanmeimei).await;
    app.store(move |store| store.unbind(&lilei_code))
        .await
        .unwrap();
    assert_eq!(
        fetch(&app, &lilei_link, CFM).await.status,
        StatusCode::NOT_FOUND
    );
    // A bundle that withholds pilotuserb as revoked ends its link with its
    // profile.
    app.import_bundle(&[], &[("pilotuserb", "revoked")]).await;
    assert_eq!(
        fetch(&app, &hanmeimei_link, CFM).await.status,
        StatusCode::NOT_FOUND
    );
    let page = hanmeimei.get(&app, "/subscription").await;
    assert!(page.body.contains("目前没有你的 CFM 配置。"));
    assert!(!page.body.contains("停用链接"));
    assert!(!page.body.contains("action=\"/subscription/link\""));
    // Delivered to again, the user has a profile but not the old link.
    app.import_profiles(&["pilotuserb"]).await;
    assert_eq!(
        fetch(&app, &hanmeimei_link, CFM).await.status,
        StatusCode::NOT_FOUND
    );
    let page = hanmeimei.get(&app, "/subscription").await;
    assert!(page.body.contains("还没有订阅链接"));
    assert!(page.body.contains("action=\"/subscription/link\""));
}

#[tokio::test]
async fn unknown_and_malformed_links_are_not_found() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let live = make_link(&app, &mut browser).await;
    let unknown = format!("/sub/{}", SecretToken::generate().unwrap().encode());
    let trailing = format!("{live}/");
    let deeper = format!("{live}/more");
    for path in [
        unknown.as_str(),
        trailing.as_str(),
        deeper.as_str(),
        "/sub/abc",
        "/sub/ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789",
        "/sub/%ff%fe",
    ] {
        let reply = fetch(&app, path, CFM).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(reply.body.contains("找不到"), "{path}");
    }
    // A HEAD request is a fetch without the document.
    let head = app
        .send(
            Browser::new(CFM)
                .request("HEAD", &live)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.headers[CONTENT_TYPE], "application/json");
    assert!(head.body.is_empty());
}

#[tokio::test]
async fn fetches_are_limited_per_network_before_the_link_is_read() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let link = make_link(&app, &mut browser).await;
    for _ in 0..LINK_FETCHES_PER_NETWORK.limit {
        assert_eq!(
            fetch(&app, "/sub/abc", CFM).await.status,
            StatusCode::NOT_FOUND
        );
    }
    let limited = fetch(&app, &link, CFM).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers[RETRY_AFTER], "3600");
    assert!(!limited.body.contains("secret-of"));
    is_document_of(&fetch(&app, &link, [203, 0, 113, 81]).await, "pilotusera");
}

#[tokio::test]
async fn link_management_needs_a_session_and_a_same_site_form() {
    let app = TestApp::new();
    let (mut browser, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    app.import_profiles(&["pilotusera"]).await;
    let mut stranger = Browser::new([203, 0, 113, 90]);
    stranger.get(&app, "/").await;
    for path in ["/subscription/link", "/subscription/link/revoke"] {
        let reply = stranger.post(&app, path, &[]).await;
        assert_eq!(reply.location(), "/login?next=subscription", "{path}");
    }
    let download = stranger.get(&app, "/subscription/profile.json").await;
    assert_eq!(download.location(), "/login?next=subscription");
    // A form from another site, with the account's cookies, is refused.
    let forged = browser
        .request("POST", "/subscription/link")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ORIGIN, "https://elsewhere.example")
        .body(Body::from(format!(
            "csrf={}",
            browser.cookies["__Host-csrf"]
        )))
        .unwrap();
    assert_eq!(app.send(forged).await.status, StatusCode::FORBIDDEN);
    let page = browser.get(&app, "/subscription").await;
    assert!(page.body.contains("还没有订阅链接"));
}

#[tokio::test]
async fn one_link_cannot_use_up_the_fetches_of_another_from_the_same_exit() {
    let app = TestApp::new();
    let (mut lilei, _) = bound(&app, "lilei@qq.com", "pilotusera").await;
    let (mut hanmeimei, _) = bound(&app, "hanmeimei@qq.com", "pilotuserb").await;
    app.import_profiles(&["pilotusera", "pilotuserb"]).await;
    let lilei_link = make_link(&app, &mut lilei).await;
    let hanmeimei_link = make_link(&app, &mut hanmeimei).await;
    // Both fetch through their tunnels, from the operator's one exit.
    for _ in 0..FETCHES_PER_LINK.limit {
        is_document_of(&fetch(&app, &lilei_link, CFM).await, "pilotusera");
    }
    let limited = fetch(&app, &lilei_link, CFM).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    is_document_of(&fetch(&app, &hanmeimei_link, CFM).await, "pilotuserb");
}
