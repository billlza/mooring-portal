//! The public pages and what every response carries.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, SET_COOKIE};

use super::support::{Browser, STRICT_POLICY, TestApp};
use crate::assets::{VIEW, module};
use crate::brand::brand;
use crate::i18n::Lang;

const PAGES: [&str; 6] = ["/", "/download", "/plans", "/help", "/login", "/register"];

/// Attributes such as `onclick=`, which no policy here lets run. Only
/// tags are read: the pages escape `<` in text.
fn handlers(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    for tag in body.split('<').skip(1) {
        let tag = tag.split('>').next().unwrap();
        // With the quoted values gone, what is left is the names.
        let names: String = tag.split('"').step_by(2).collect();
        for attribute in names.split_whitespace().skip(1) {
            if let Some((name, _)) = attribute.split_once('=')
                && name.len() > 2
                && name.starts_with("on")
            {
                found.push(name.to_owned());
            }
        }
    }
    found
}

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
    for line in brand(Lang::Zh).slogan {
        assert!(home.body.contains(line));
    }
    assert!(home.body.contains("登录 / 注册"));
}

#[tokio::test]
async fn pages_load_nothing_from_other_hosts_and_only_the_home_page_runs_a_script() {
    let app = TestApp::new();
    let script = app.state.assets().paths().script.clone();
    let mut browser = Browser::new([203, 0, 113, 1]);
    for path in PAGES {
        let body = browser.get(&app, path).await.body;
        if path == "/" {
            // One script: a module of this site's own, with nothing inline.
            assert_eq!(body.matches("<script").count(), 1);
            assert!(body.contains(&format!(
                "<script type=\"module\" src=\"{script}\"></script>"
            )));
        } else {
            assert!(!body.contains("<script"), "{path}");
        }
        assert!(
            !body.contains("style="),
            "{path}: inline styles break the CSP"
        );
        assert!(!body.contains("javascript:"), "{path}");
        assert_eq!(handlers(&body), Vec::<String>::new(), "{path}");
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
    // Nor do the pages behind the login, or an error page.
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    for path in [
        "/account",
        "/account/password",
        "/subscription",
        "/no-such-page",
    ] {
        let body = browser.get(&app, path).await.body;
        assert!(!body.contains("<script"), "{path}");
        assert_eq!(handlers(&body), Vec::<String>::new(), "{path}");
    }
}

#[test]
fn handlers_are_found_wherever_they_are_written() {
    assert_eq!(
        handlers("<a href=\"/\" onclick=\"x()\"><img src=\"a\" onerror=boom>"),
        ["onclick", "onerror"]
    );
    // Words are not attributes, and neither is what a value holds.
    assert!(
        handlers("<p>log on = off, one=1</p><form action=\"/on\" title=\"a onb=c\" open>")
            .is_empty()
    );
}

#[tokio::test]
async fn only_the_home_page_may_run_this_sites_scripts() {
    let app = TestApp::new();
    let paths = app.state.assets().paths().clone();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    for path in ["/", "/?from=elsewhere"] {
        let reply = browser.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(
            reply.headers[CONTENT_SECURITY_POLICY],
            format!("{STRICT_POLICY}; script-src 'self'; worker-src 'none'"),
            "{path}"
        );
    }
    // A browser asking only for the page's headers is told the same.
    let request = browser.request("HEAD", "/").body(Body::empty()).unwrap();
    let headers_only = app.send(request).await;
    assert_eq!(headers_only.status, StatusCode::OK);
    assert_eq!(
        headers_only.headers[CONTENT_SECURITY_POLICY],
        format!("{STRICT_POLICY}; script-src 'self'; worker-src 'none'")
    );
    // Only the page itself: another method at its address gets no script.
    // (The page that fails is in `failures`.)
    let request = browser.request("POST", "/").body(Body::empty()).unwrap();
    let refused = app.send(request).await;
    assert_eq!(refused.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(refused.headers[CONTENT_SECURITY_POLICY], STRICT_POLICY);
    for path in [
        "/download",
        "/plans",
        "/help",
        "/login",
        "/register",
        "/account",
        "/account/password",
        "/subscription",
        "/subscription/profile.json",
        "/sub/not-a-token",
        "/login?next=nowhere",
        "/no-such-page",
        "/robots.txt",
        "/assets/site.css",
        "/index.html",
        "//",
        &paths.script,
        &paths.hero,
    ] {
        let reply = browser.get(&app, path).await;
        assert_eq!(
            reply.headers[CONTENT_SECURITY_POLICY], STRICT_POLICY,
            "{path}"
        );
        // Nothing but the script's own modules is served as script.
        let served_as = reply
            .headers
            .get(CONTENT_TYPE)
            .map_or("", |value| value.to_str().unwrap());
        assert_eq!(
            served_as.contains("javascript"),
            path.ends_with(".mjs"),
            "{path}: {served_as}"
        );
    }
}

#[tokio::test]
async fn the_home_page_shows_nothing_a_request_chose() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    let plain = browser.get(&app, "/").await.body;
    let token = browser.cookies["__Host-csrf"].clone();
    // The same browser, now sending what a hostile link or page could:
    // a cookie of this site's with a value it never set, and a stray one.
    for name in ["__Host-lang", "zq9marker"] {
        browser
            .cookies
            .insert(name.to_owned(), "zq9<script>marker()</script>".to_owned());
    }
    let marker = "zq9\"><script>marker()</script>";
    let request = browser
        .request("GET", "/?next=%22%3E%3Cscript%3Ezq9marker&zq9marker=1")
        .header("user-agent", marker)
        .header("referer", "https://zq9marker.example/")
        .header("accept-language", "zq9marker")
        .header("x-forwarded-for", "zq9marker")
        .body(Body::empty())
        .unwrap();
    let hostile = app.send(request).await;
    assert_eq!(hostile.status, StatusCode::OK);
    assert_eq!(hostile.body, plain);
    assert!(!hostile.body.contains("zq9"));
    // The one thing of the request's in the page is its own CSRF token,
    // which is accepted only as 64 hexadecimal digits.
    assert!(plain.contains(&format!("value=\"{token}\"")));
    assert!(token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[tokio::test]
async fn the_home_page_carries_the_live_scene_and_still_shows_the_drawn_image() {
    let app = TestApp::new();
    let paths = app.state.assets().paths().clone();
    let mut browser = Browser::new([203, 0, 113, 1]);
    let home = browser.get(&app, "/").await.body;
    // Without script: the drawn image. The scene has no button of its own:
    // the one button on the page is the language switch's.
    assert!(home.contains(&format!("<img class=\"hero-art\" src=\"{}\"", paths.hero)));
    assert_eq!(home.matches("<button").count(), 1);
    assert!(home.contains("<button type=\"submit\" title=\"Switch to English\""));
    // What the script is told: the size of the picture it draws, written
    // as the script reads it, in plain decimals. Nothing else: the scene
    // loads no picture.
    assert!(home.contains(&format!(
        "<canvas class=\"hero-scene\" aria-hidden=\"true\" data-view=\"{} {}\"></canvas>",
        VIEW.0, VIEW.1
    )));
    assert!(home.contains("data-view=\"3160 790\""));
    assert_eq!(home.matches(" data-").count(), 1);
    assert_eq!(
        home.matches(".svg").count(),
        3,
        "the mark twice and the hero"
    );
    // The page fetches the entry's imports ahead of it.
    assert!(!paths.imported.is_empty());
    for module in &paths.imported {
        assert!(home.contains(&format!("<link rel=\"modulepreload\" href=\"{module}\">")));
    }
    assert_eq!(
        home.matches("rel=\"modulepreload\"").count(),
        paths.imported.len()
    );
    // And it holds everything the script looks for: each element by its
    // class, and each fact by its attribute.
    let stage = module("stage.mjs");
    let looked_for = |call: &str| -> Vec<&str> {
        stage
            .match_indices(call)
            .filter_map(|(at, _)| {
                let arguments = &stage[at + call.len()..];
                let arguments = &arguments[..arguments.find(')')?];
                let name = arguments.strip_suffix('"')?;
                Some(&name[name.rfind('"')? + 1..])
            })
            .collect()
    };
    let selectors = looked_for("need(");
    assert_eq!(selectors, [".hero", ".hero-art", "canvas.hero-scene"]);
    for selector in selectors {
        let (tag, class) = selector.split_once('.').unwrap();
        let written = if tag.is_empty() {
            format!(" class=\"{class}\"")
        } else {
            format!("<{tag} class=\"{class}\"")
        };
        assert!(home.contains(&written), "{selector}");
    }
    assert_eq!(stage.matches("canvas.dataset.").count(), 1);
    assert!(stage.contains("canvas.dataset.view;"));
    // No other page has the scene, or any script.
    for path in ["/download", "/plans", "/help", "/login", "/register"] {
        let body = browser.get(&app, path).await.body;
        assert!(
            !body.contains("<canvas") && !body.contains("<script"),
            "{path}"
        );
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
async fn the_download_page_offers_the_release_the_host_last_imported() {
    const FIRST: &str = "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f";
    const SECOND: &str = "fc9a1e62700a1b5f66ec42fcda0e5eb90efc052251321927f52b97c9223b7b93";
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    // Before any import the page says so and points at the releases. It
    // offers no file, and no release written into the program.
    let body = browser.get(&app, "/download").await.body;
    assert!(body.contains("本站还没有取得 CFM 的发布信息"));
    assert!(body.contains("href=\"https://github.com/billlza/cfw-rs/releases/latest\""));
    assert!(!body.contains(".dmg") && !body.contains("SHA-256") && !body.contains("0.5.0"));

    // Published at two in the morning of the 5th, Beijing time.
    app.offer_release("0.5.1", "2026-11-04T18:00:00Z", FIRST)
        .await;
    let body = browser.get(&app, "/download").await.body;
    for shown in [
        "<dd>CFM 0.5.1</dd>",
        "<dd>2026-11-05</dd>",
        "<code class=\"hash\">c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f</code>",
        "<code class=\"hash\">Clash.for.Mac_0.5.1_arm64.dmg</code>",
        "href=\"https://github.com/billlza/cfw-rs/releases/download/v0.5.1/Clash.for.Mac_0.5.1_arm64.dmg\"",
        "href=\"https://github.com/billlza/cfw-rs/releases/tag/v0.5.1\"",
        "发布信息核对于 2026-11-05 20:00 <span class=\"muted small\">北京时间</span>",
    ] {
        assert!(body.contains(shown), "{shown}");
    }
    assert!(!body.contains("本站还没有取得") && !body.contains("超过 24 小时"));
    assert!(!body.contains("releases/latest"));

    // The host imports the next release, and the page follows.
    app.clock.advance(300).unwrap();
    app.offer_release("0.5.2", "2026-11-05T11:00:00Z", SECOND)
        .await;
    let body = browser.get(&app, "/download").await.body;
    assert!(body.contains("<dd>CFM 0.5.2</dd>") && body.contains(SECOND));
    assert!(
        body.contains("Clash.for.Mac_0.5.2_arm64.dmg") && body.contains("核对于 2026-11-05 20:05")
    );
    assert!(!body.contains("0.5.1") && !body.contains(FIRST));

    // A day without a successful check passes unremarked; a second more
    // and the page says its details may be out of date.
    app.clock.advance(24 * 3600).unwrap();
    assert!(
        !browser
            .get(&app, "/download")
            .await
            .body
            .contains("24 小时内没有核对")
    );
    app.clock.advance(1).unwrap();
    let stale = browser.get(&app, "/download").await.body;
    assert!(
        stale.contains(
            "<p class=\"alert\" role=\"status\">最近 24 小时内没有核对这份发布信息的记录"
        )
    );
    assert!(stale.contains("<dd>CFM 0.5.2</dd>"), "it is still offered");

    // The rest of the page is as it was.
    assert!(stale.contains("即将推出"), "Windows is not released");
    assert!(stale.contains("中国大陆直连"));
    assert!(!stale.contains(".exe") && !stale.contains(".msi"));

    browser
        .post(&app, "/lang", &[("to", "en"), ("back", "download")])
        .await;
    let english = browser.get(&app, "/download").await.body;
    assert!(english.contains("Release details checked at 2026-11-05 20:05"));
    assert!(english.contains("no record of these release details being checked in the last 24"));
    let fresh = TestApp::new();
    browser.cookies.remove("__Host-session");
    let english = browser.get(&fresh, "/download").await.body;
    assert!(
        english.contains("has not fetched CFM&#39;s release details yet"),
        "{english}"
    );
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
async fn the_plans_say_when_a_paused_service_resumes_and_how_long_a_pack_lasts() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    browser.get(&app, "/").await;
    // Mooring's nightly job resumes a user a pack now covers, and one who
    // has renewed; a pack counts only in the cycle it is granted in (the
    // operator's decision of 2026-11-12).
    for (lang, said, unsaid) in [
        (
            "zh",
            [
                "买一个够补上超出部分的流量包，会在下一次北京时间凌晨 3 点到 6 点的更新时恢复。",
                "流量包只在开通它的那个计费周期内有效，没用完的不带到下个周期。",
                "服务到期后暂停，直到你续费；续费后，会在下一次北京时间凌晨 3 点到 6 点的更新时恢复。",
            ],
            "直到运营者恢复",
        ),
        (
            "en",
            [
                "A traffic pack that covers what you used over the quota resumes it at the next \
                 update, between 3 and 6 a.m. Beijing time.",
                "A traffic pack counts only in the billing cycle it is granted in; what is left \
                 unused does not carry over.",
                "When your service expires, it pauses until you renew; once you have, it resumes \
                 at the next update, between 3 and 6 a.m. Beijing time.",
            ],
            "until the operator resumes it",
        ),
    ] {
        browser
            .post(&app, "/lang", &[("to", lang), ("back", "plans")])
            .await;
        let plans = browser.get(&app, "/plans").await.body;
        for sentence in said {
            assert!(plans.contains(sentence), "{lang}: {sentence}");
        }
        assert!(!plans.contains(unsaid), "{lang}");
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
    // The refusal is this site's own page, and quotes nothing back.
    assert!(bogus.body.contains("请求无效。") && !bogus.body.contains("evil"));
    let (mut member, _) = app.signed_up("alice@qq.com").await;
    for path in ["/account?notice=evil", "/subscription?notice=evil"] {
        let reply = member.get(&app, path).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{path}");
        assert!(reply.body.contains("请求无效。") && !reply.body.contains("evil"));
    }
}
