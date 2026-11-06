//! The public pages and what every response carries.

use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, SET_COOKIE};

use super::support::{Browser, TestApp};
use crate::brand::brand;
use crate::i18n::Lang;
use crate::release::MAC;

const PAGES: [&str; 6] = ["/", "/download", "/plans", "/help", "/login", "/register"];

#[tokio::test]
async fn public_pages_render_in_chinese_by_default() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    for path in PAGES {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert!(reply.body.contains("<html lang=\"zh-CN\">"), "{path}");
        assert!(reply.body.contains(brand(Lang::Zh).name), "{path}");
        assert!(reply.body.contains("name=\"csrf\""), "{path}");
        assert!(
            reply
                .body
                .contains("<meta name=\"robots\" content=\"noindex\">")
        );
    }
    let home = browser.get(&app, "/").await;
    assert!(home.body.contains(brand(Lang::Zh).slogan));
    assert!(home.body.contains("登录 / 注册"));
}

#[tokio::test]
async fn pages_load_nothing_from_other_hosts_and_run_no_script() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    for path in PAGES {
        let body = browser.get(&app, path).await.body;
        assert!(!body.contains("<script"), "{path}");
        assert!(
            !body.contains("style="),
            "{path}: inline styles break the CSP"
        );
        for attribute in ["src=\"", "href=\""] {
            for (index, _) in body.match_indices(attribute) {
                let value = &body[index + attribute.len()..];
                let external = value.starts_with("http") || value.starts_with("//");
                let allowed = value.starts_with("https://github.com/billlza/cfw-rs/releases/");
                assert!(
                    !external || allowed,
                    "{path}: {}",
                    &value[..value.len().min(60)]
                );
            }
        }
    }
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    for path in ["/", "/no-such-page", "/robots.txt"] {
        let reply = browser.get(&app, path).await;
        let csp = reply.headers[CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(csp.contains("default-src 'none'") && csp.contains("frame-ancestors 'none'"));
        for (name, value) in [
            ("x-content-type-options", "nosniff"),
            ("x-frame-options", "DENY"),
            ("referrer-policy", "same-origin"),
            ("strict-transport-security", "max-age=31536000"),
            ("cross-origin-opener-policy", "same-origin"),
        ] {
            assert_eq!(reply.headers[name], value, "{path} {name}");
        }
    }
    let page = browser.get(&app, "/").await;
    assert_eq!(page.headers[CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn the_download_page_offers_only_the_published_release() {
    let app = TestApp::new();
    let body = Browser::new([203, 0, 113, 1])
        .get(&app, "/download")
        .await
        .body;
    assert!(body.contains(MAC.dmg_url) && body.contains(MAC.dmg_sha256));
    assert!(body.contains("CFM 0.5.0"));
    assert!(body.contains("即将推出"), "Windows is not released");
    assert!(body.contains("中国大陆直连"));
    assert!(!body.contains(".exe") && !body.contains(".msi"));
}

#[tokio::test]
async fn no_page_calls_the_service_free_and_the_plans_say_how_it_is_paid() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    browser.get(&app, "/").await;
    for (lang, free, paid, expired) in [
        ("zh", ["免费", "不接受任何付款"], "只接受银行卡", "满三个月"),
        (
            "en",
            ["free", "accepts no payments"],
            "only bank cards",
            "three months",
        ),
    ] {
        browser
            .post(&app, "/lang", &[("to", lang), ("back", "home")])
            .await;
        for path in PAGES {
            let body = browser.get(&app, path).await.body;
            for word in free {
                assert!(!body.contains(word), "{lang} {path}: {word}");
            }
        }
        let plans = browser.get(&app, "/plans").await.body;
        assert!(plans.contains(paid) && plans.contains(expired), "{lang}");
    }
}

#[tokio::test]
async fn a_new_browser_gets_a_csrf_cookie_once() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    let first = browser.get(&app, "/").await;
    let cookie = first.headers[SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("__Host-csrf="), "{cookie}");
    assert!(
        cookie.contains("Secure")
            && cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
    );
    let token = browser.cookies["__Host-csrf"].clone();
    assert!(first.body.contains(&format!("value=\"{token}\"")));
    let second = browser.get(&app, "/download").await;
    assert!(second.headers.get(SET_COOKIE).is_none());
    assert!(second.body.contains(&format!("value=\"{token}\"")));
}

#[tokio::test]
async fn protected_pages_send_anonymous_visitors_to_log_in() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    for (path, next) in [
        ("/account", "account"),
        ("/account/password", "password"),
        ("/subscription", "subscription"),
    ] {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(reply.location(), format!("/login?next={next}"));
    }
    let login = browser.get(&app, "/login?next=subscription").await;
    assert!(login.body.contains("name=\"next\" value=\"subscription\""));
    let bogus = browser.get(&app, "/login?next=https://evil.example").await;
    assert_eq!(
        bogus.status,
        StatusCode::BAD_REQUEST,
        "only named pages are accepted"
    );
}
