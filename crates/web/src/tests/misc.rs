//! Language switching, assets, errors and the reverse-proxy mode.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, SET_COOKIE};

use super::support::{Browser, TestApp};
use crate::brand::brand;
use crate::config::ClientIpSource;
use crate::i18n::Lang;
use crate::security::limits::LOGIN_PER_NETWORK;

#[tokio::test]
async fn the_language_switch_remembers_english_and_returns() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 20]);
    browser.get(&app, "/help").await;
    let reply = browser
        .post(&app, "/lang", &[("to", "en"), ("back", "help")])
        .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), "/help");
    let cookie = reply.headers[SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("__Host-lang=en;") && cookie.contains("Max-Age=31536000"));
    let page = browser.get(&app, "/help").await.body;
    assert!(page.contains("<html lang=\"en\">"));
    assert!(page.contains(brand(Lang::En).name));
    assert!(page.contains("Contacting the operator"));
    assert!(
        page.contains("name=\"to\" value=\"zh\""),
        "the switch now offers Chinese"
    );
    let back = browser
        .post(&app, "/lang", &[("to", "zh"), ("back", "home")])
        .await;
    assert_eq!(back.location(), "/");
    assert!(
        browser
            .get(&app, "/")
            .await
            .body
            .contains("<html lang=\"zh-CN\">")
    );
}

#[tokio::test]
async fn every_page_renders_in_english() {
    let app = TestApp::new();
    let (mut browser, code) = app.signed_up("alice@qq.com").await;
    browser
        .post(&app, "/lang", &[("to", "en"), ("back", "home")])
        .await;
    for path in [
        "/",
        "/download",
        "/plans",
        "/help",
        "/account",
        "/account/password",
        "/subscription",
    ] {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert!(reply.body.contains("<html lang=\"en\">"), "{path}");
    }
    assert!(
        browser
            .get(&app, "/subscription")
            .await
            .body
            .contains("No subscription")
    );
    app.bind(code, "pilotusera").await;
    assert!(
        browser
            .get(&app, "/subscription")
            .await
            .body
            .contains("Waiting for data")
    );
}

#[tokio::test]
async fn assets_are_served_by_content_name_and_cached() {
    let app = TestApp::new();
    let paths = app.state.assets().paths().clone();
    let mut browser = Browser::new([203, 0, 113, 21]);
    for (path, content_type) in [
        (&paths.css, "text/css; charset=utf-8"),
        (&paths.mark, "image/svg+xml"),
        (&paths.hero, "image/svg+xml"),
        (&paths.land, "image/svg+xml"),
        (&paths.ring, "image/svg+xml"),
        (&paths.script, "text/javascript; charset=utf-8"),
    ] {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(reply.headers[CONTENT_TYPE], content_type);
        assert_eq!(
            reply.headers[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(
            reply.headers.get(SET_COOKIE).is_none(),
            "assets set no cookies"
        );
    }
    let stable = browser.get(&app, "/assets/site.css").await;
    assert_eq!(stable.headers[CACHE_CONTROL], "public, max-age=300");
    let missing = browser.get(&app, "/assets/site.0000.css").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    // A path that is not even text is answered the same way, by this
    // site's own page.
    let unreadable = browser.get(&app, "/assets/%ff").await;
    assert_eq!(unreadable.status, StatusCode::NOT_FOUND);
    assert_eq!(unreadable.body, missing.body);
    assert!(unreadable.body.contains("<html lang=\"zh-CN\">"));
    // The script's modules import each other from its own directory, and
    // from no other.
    let (directory, entry) = paths.script.rsplit_once('/').unwrap();
    assert_eq!(entry, "hero.mjs");
    assert_eq!(
        paths.imported,
        ["measure.mjs", "scene.mjs", "shader.mjs", "stage.mjs"]
            .map(|name| format!("{directory}/{name}"))
    );
    for module in &paths.imported {
        let reply = browser.get(&app, module).await;
        assert_eq!(reply.status, StatusCode::OK, "{module}");
        assert_eq!(
            reply.headers[CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            reply.headers[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
    }
    for missing in [
        format!("{directory}/measure.test.mjs"),
        format!("{directory}/hero.mjs/hero.mjs"),
        format!("{directory}/"),
        "/assets/js.0000000000000000/hero.mjs".to_owned(),
        "/assets/hero.mjs".to_owned(),
        "/assets/".to_owned(),
    ] {
        assert_eq!(
            browser.get(&app, &missing).await.status,
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
    let home = browser.get(&app, "/").await.body;
    assert!(home.contains(&paths.css) && home.contains(&paths.hero));
    let robots = browser.get(&app, "/robots.txt").await;
    assert_eq!(robots.body, "User-agent: *\nDisallow: /\n");
}

#[tokio::test]
async fn unknown_pages_and_methods_are_refused_plainly() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 22]);
    let missing = browser.get(&app, "/admin").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(
        missing.body.contains("找不到这个页面。")
            && missing.body.contains("This page does not exist.")
    );
    assert!(missing.body.contains("/assets/site.css"));
    assert_eq!(
        browser.get(&app, "/logout").await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let request = browser
        .request("DELETE", "/account")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.send(request).await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn behind_a_proxy_the_forwarded_address_is_limited() {
    let app = TestApp::with(ClientIpSource::ForwardedFor, 100);
    let mut local = Browser::new([127, 0, 0, 1]);
    local.get(&app, "/login").await;
    let token = local.cookies["__Host-csrf"].clone();
    // A malformed email is counted against the network only.
    let form = format!("csrf={token}&email=not+an+email&password=nope");
    let send = |forwarded: Option<&'static str>, peer: &Browser| {
        let mut request = peer
            .request("POST", "/login")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(address) = forwarded {
            request = request.header("x-forwarded-for", address);
        }
        request.body(Body::from(form.clone())).unwrap()
    };
    for _ in 0..LOGIN_PER_NETWORK.limit {
        let reply = app.send(send(Some("203.0.113.30"), &local)).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    }
    let limited = app.send(send(Some("203.0.113.30"), &local)).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    let neighbour = app.send(send(Some("198.51.100.40"), &local)).await;
    assert_eq!(
        neighbour.status,
        StatusCode::UNAUTHORIZED,
        "another client of the same proxy has its own allowance"
    );
    let missing = app.send(send(None, &local)).await;
    assert_eq!(
        missing.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a proxy must forward the address"
    );
    let mut direct = Browser::new([203, 0, 113, 31]);
    direct.cookies = local.cookies.clone();
    let bypass = app.send(send(Some("203.0.113.30"), &direct)).await;
    assert_eq!(
        bypass.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "only the local proxy may connect"
    );
}
