//! The home page as a sheet: its words, its two buttons, its three steps,
//! and the bar and the footer every page shares with it.

use super::support::{Browser, TestApp};
use crate::assets::VIEW;
use crate::brand::brand;
use crate::i18n::Lang;

/// The part of `body` from `opening` up to the next `closing`.
fn part<'a>(body: &'a str, opening: &str, closing: &str) -> &'a str {
    let from = body.find(opening).unwrap_or_else(|| panic!("no {opening}"));
    let rest = &body[from..];
    &rest[..rest
        .find(closing)
        .unwrap_or_else(|| panic!("{opening} is not closed"))]
}

/// Where each link in `markup` goes, in the order written.
fn links(markup: &str) -> Vec<&str> {
    markup
        .match_indices("href=\"")
        .map(|(at, opening)| {
            let rest = &markup[at + opening.len()..];
            &rest[..rest.find('"').unwrap()]
        })
        .collect()
}

#[tokio::test]
async fn the_home_page_says_what_the_service_is_and_where_to_begin() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    let home = browser.get(&app, "/").await.body;
    let hero = part(&home, "<div class=\"hero-inner\">", "</section>");
    // The service above the headline, the headline in its two lines, and
    // what it comes to under it.
    assert!(hero.contains("<p class=\"eyebrow\">Mooring · 网络订阅服务</p>"));
    assert!(
        hero.contains("<h1 class=\"slogan\"><span>让连接，</span><span>有处停泊。</span></h1>")
    );
    assert!(hero.contains(
        "<p class=\"hero-lead\"><span>从选择套餐到管理订阅，</span>\
         <span>让每一步都清晰、有序。</span></p>"
    ));
    assert_eq!(home.matches("<h1").count(), 1);
    // Two buttons: the plans first, then the client.
    let buttons = part(hero, "<div class=\"cta\">", "</div>");
    assert_eq!(links(buttons), ["/plans", "/download"]);
    assert!(buttons.contains("<a class=\"button\" href=\"/plans\">查看套餐<svg"));
    assert!(buttons.contains("<a class=\"button ghost\" href=\"/download\">下载客户端<svg"));
    // And two facts under them.
    let facts = part(hero, "<ul class=\"hero-facts\"", "</ul>");
    assert!(facts.contains("<li>英国移动网络出口</li><li>国内网站直连</li>"));
    assert_eq!(facts.matches("<li>").count(), 2);
    // The three steps, numbered, each with its own picture.
    let steps = part(
        &home,
        "<section class=\"start\" aria-label=\"如何开始\">",
        "</section>",
    );
    for (number, title, body) in [
        ("01", "选择套餐", "找到适合你的使用方式"),
        ("02", "导入订阅", "在客户端中添加订阅"),
        ("03", "开始使用", "连接之后，安心使用"),
    ] {
        let step = part(
            steps,
            &format!("<span class=\"step-number\">{number}</span>"),
            "</li>",
        );
        assert!(step.contains(&format!("<h2>{title}</h2>")), "{number}");
        assert!(step.contains(&format!("<p>{body}</p>")), "{number}");
    }
    assert_eq!(steps.matches("<li>").count(), 3);
    let pictures: Vec<&str> = steps
        .match_indices("<svg class=\"step-icon\"")
        .map(|(at, _)| part(&steps[at..], "<path", "</svg>"))
        .collect();
    assert_eq!(pictures.len(), 3);
    assert!(pictures[0] != pictures[1] && pictures[1] != pictures[2] && pictures[0] != pictures[2]);
    // The page ends there: what the service is and promises is on the
    // page the first button leads to.
    assert_eq!(home.matches("<section").count(), 2);
    for moved in ["这是一项什么样的服务", "不窥探", "这不是商业 VPN"] {
        assert!(!home.contains(moved), "{moved}");
    }
    let plans = browser.get(&app, "/plans").await.body;
    for kept in [
        "<h2>这是一项什么样的服务</h2>",
        "<h2>小而稳</h2>",
        "<h2>看得清</h2>",
        "<h2>不窥探</h2>",
        "<h2>国内直连</h2>",
        "不记录你访问的网站、域名、IP 地址或 DNS 查询",
        "<p class=\"note\">这不是商业 VPN：本服务只对运营者信任的朋友开放，不保证随时可用。</p>",
        "<h2>开通方式</h2>",
        "<h2>付款</h2>",
    ] {
        assert!(plans.contains(kept), "{kept}");
    }
}

#[tokio::test]
async fn the_home_page_reads_the_same_in_english() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    // The first page gives the browser the token the switch asks for.
    assert!(
        browser
            .get(&app, "/")
            .await
            .body
            .contains("<html lang=\"zh-CN\">")
    );
    browser
        .post(&app, "/lang", &[("to", "en"), ("back", "home")])
        .await;
    let home = browser.get(&app, "/").await.body;
    assert!(home.contains("<html lang=\"en\">"));
    assert!(home.contains("<p class=\"eyebrow\">Mooring · Network subscription</p>"));
    let [first, second] = brand(Lang::En).slogan;
    assert!(home.contains(&format!(
        "<h1 class=\"slogan\"><span>{first}</span><span>{second}</span></h1>"
    )));
    assert!(home.contains(">View plans<svg") && home.contains(">Get the client<svg"));
    assert!(home.contains(
        "<li>Exits through a UK mobile network</li><li>Mainland sites connect directly</li>"
    ));
    assert!(home.contains("<section class=\"start\" aria-label=\"Getting started\">"));
    for title in ["Choose a plan", "Import the subscription", "Start using it"] {
        assert!(home.contains(&format!("<h2>{title}</h2>")), "{title}");
    }
    // The name is written once in the bar where it is its own Latin form.
    assert!(!home.contains("class=\"logo-latin\""));
    let plans = browser.get(&app, "/plans").await.body;
    assert!(plans.contains("<h2>Discreet</h2>") && plans.contains("<h2>What this service is</h2>"));
}

#[tokio::test]
async fn every_page_opens_on_the_home_pages_picture_drawn_and_still() {
    let app = TestApp::new();
    let hero = app.state.assets().paths().hero.clone();
    // The picture the home page paints its scene over, as an ornament that
    // no reader is told of: the same file, still, with the stars that come
    // and go over it.
    let backdrop = format!(
        "<div class=\"backdrop\" aria-hidden=\"true\">\n  \
         <img class=\"hero-art\" src=\"{hero}\" alt=\"\" width=\"{}\" height=\"{}\" \
         decoding=\"async\">\n  <div class=\"hero-twinkle\"></div>\n</div>",
        VIEW.0, VIEW.1
    );
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    let mut stranger = Browser::new([203, 0, 113, 9]);
    for (signed_in, path) in [
        (true, "/plans"),
        (true, "/download"),
        (true, "/help"),
        (true, "/account"),
        (true, "/account/password"),
        (true, "/subscription"),
        (false, "/login"),
        (false, "/register"),
    ] {
        let visitor = if signed_in {
            &mut browser
        } else {
            &mut stranger
        };
        let body = visitor.get(&app, path).await.body;
        assert_eq!(body.matches(&backdrop).count(), 1, "{path}");
        // Under the bar, which comes after it.
        assert!(
            body.find(&backdrop) < body.find("<header class=\"topbar\">"),
            "{path}"
        );
        // Still: these pages run no script, so nothing is painted over it.
        assert!(
            !body.contains("<canvas") && !body.contains("<script"),
            "{path}"
        );
    }
    // The home page holds the picture once, under its live scene.
    let home = browser.get(&app, "/").await.body;
    assert!(!home.contains("class=\"backdrop\""));
    assert_eq!(home.matches(&format!("src=\"{hero}\"")).count(), 1);
}

#[tokio::test]
async fn every_page_has_the_same_bar_and_footer() {
    let app = TestApp::new();
    let (mut browser, _) = app.signed_up("alice@qq.com").await;
    for (path, current) in [
        ("/", "/"),
        ("/plans", "/plans"),
        ("/download", "/download"),
        ("/mac", "/mac"),
        ("/help", "/help"),
        ("/account", ""),
        ("/subscription", ""),
    ] {
        let body = browser.get(&app, path).await.body;
        // The links, in the bar and again in the menu a narrow screen
        // folds them into: home, plans, download, renting a Mac, help.
        for nav in [
            part(&body, "<nav class=\"nav\"", "</nav>"),
            part(&body, "<details class=\"menu\">", "</details>"),
        ] {
            assert_eq!(
                links(nav),
                ["/", "/plans", "/download", "/mac", "/help"],
                "{path}"
            );
            for name in ["首页", "套餐", "下载", "租 Mac", "帮助"] {
                assert!(nav.contains(&format!(">{name}</a>")), "{path} {name}");
            }
            // The page's own link says so, and only that one.
            let marked: Vec<&str> = nav
                .split("<a ")
                .skip(1)
                .filter(|link| link.contains("aria-current=\"page\""))
                .map(|link| links(link)[0])
                .collect();
            let expected: Vec<&str> = [current].into_iter().filter(|c| !c.is_empty()).collect();
            assert_eq!(marked, expected, "{path}");
        }
        // The bar: the mark and the name, and the way to the account.
        let bar = part(&body, "<header class=\"topbar\">", "</header>");
        assert!(bar.contains(
            "<span class=\"logo-name\">泊光</span><span class=\"logo-latin\">Moorlight</span>"
        ));
        assert!(
            bar.contains("<a class=\"pill\" href=\"/account\">我的账户<svg"),
            "{path}"
        );
        // The footer: the name again, and the way to help.
        let footer = part(&body, "<footer class=\"footer\">", "</footer>");
        assert_eq!(links(footer), ["/", "/help"], "{path}");
        assert!(footer.contains(
            "<span class=\"logo-name\">泊光</span><span class=\"logo-latin\">Moorlight</span></a>"
        ));
        assert!(
            footer.contains("<a class=\"footer-help\" href=\"/help\">帮助中心<svg"),
            "{path}"
        );
        // Help is reached from the bar and the footer. The one thing that
        // floats over a signed-in user's page is the box that writes to the
        // operator (2026-11-14), after the footer.
        assert!(!body.contains("help-float"), "{path}");
        let after = body.split("</footer>\n").nth(1).unwrap();
        assert!(after.starts_with("<details class=\"dock\">"), "{path}");
        assert_eq!(
            body.matches("<details class=\"dock\">").count(),
            1,
            "{path}"
        );
        assert!(
            after.trim_end().ends_with("</details>\n</body>\n</html>"),
            "{path}"
        );
    }
    // A visitor who is not signed in is shown the way in instead, and
    // nothing floats over the page.
    let mut stranger = Browser::new([203, 0, 113, 9]);
    let home = stranger.get(&app, "/").await.body;
    assert!(home.contains("<a class=\"pill\" href=\"/login\">登录 / 注册<svg"));
    assert!(!home.contains("我的账户"));
    assert!(home.trim_end().ends_with("</footer>\n</body>\n</html>"));
}
