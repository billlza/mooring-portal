//! Every state-changing request is refused without this site's token.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, ORIGIN};

use super::support::{Browser, ORIGIN_URL, TestApp};

/// Every POST route, with a form that would otherwise be accepted.
const FORMS: [(&str, &str); 6] = [
    ("/login", "email=a%40qq.com&password=x"),
    (
        "/register",
        "email=a%40qq.com&password=correct+horse+battery&confirm=correct+horse+battery",
    ),
    ("/logout", ""),
    (
        "/account/password",
        "current=x&password=correct+horse+battery&confirm=correct+horse+battery",
    ),
    ("/lang", "to=en&back=home"),
    ("/lang", "to=zh&back=help"),
];

async fn post(
    app: &TestApp,
    browser: &Browser,
    path: &str,
    body: String,
    headers: &[(&str, &str)],
) -> StatusCode {
    let mut request = browser.request("POST", path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.send(request.body(Body::from(body)).unwrap())
        .await
        .status
}

const FORM: (&str, &str) = ("content-type", "application/x-www-form-urlencoded");

#[tokio::test]
async fn forms_without_the_token_are_refused() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    browser.get(&app, "/").await;
    let token = browser.cookies["__Host-csrf"].clone();
    for (path, fields) in FORMS {
        let same_site = [FORM, ("origin", ORIGIN_URL)];
        assert_eq!(
            post(&app, &browser, path, fields.to_owned(), &same_site).await,
            StatusCode::FORBIDDEN,
            "{path}: no token"
        );
        let wrong = format!("csrf={}&{fields}", "0".repeat(64));
        assert_eq!(
            post(&app, &browser, path, wrong, &same_site).await,
            StatusCode::FORBIDDEN,
            "{path}: wrong token"
        );
        let malformed = format!("csrf=nope&{fields}");
        assert_eq!(
            post(&app, &browser, path, malformed, &same_site).await,
            StatusCode::FORBIDDEN,
            "{path}: malformed token"
        );
        let good = format!("csrf={token}&{fields}");
        for headers in [
            [FORM, ("origin", "https://evil.example")],
            [FORM, ("sec-fetch-site", "cross-site")],
        ] {
            assert_eq!(
                post(&app, &browser, path, good.clone(), &headers).await,
                StatusCode::FORBIDDEN,
                "{path}: {headers:?}"
            );
        }
        let text = [("content-type", "text/plain"), ("origin", ORIGIN_URL)];
        assert_eq!(
            post(&app, &browser, path, good.clone(), &text).await,
            StatusCode::BAD_REQUEST,
            "{path}: not a form"
        );
    }
}

#[tokio::test]
async fn a_token_without_its_cookie_is_refused() {
    let app = TestApp::new();
    let mut victim = Browser::new([203, 0, 113, 1]);
    victim.get(&app, "/").await;
    let token = victim.cookies["__Host-csrf"].clone();
    // Another browser (no cookie, or its own) cannot use the victim's token.
    let mut attacker = Browser::new([203, 0, 113, 66]);
    let body = format!("csrf={token}&to=en&back=home");
    let status = post(&app, &attacker, "/lang", body.clone(), &[FORM]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    attacker.get(&app, "/").await;
    let status = post(&app, &attacker, "/lang", body, &[FORM]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_form_with_the_token_passes_without_origin_headers() {
    // Older browsers send neither Origin nor Sec-Fetch-Site; the token alone
    // then decides.
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    browser.get(&app, "/").await;
    let token = browser.cookies["__Host-csrf"].clone();
    let status = post(
        &app,
        &browser,
        "/lang",
        format!("csrf={token}&to=en&back=home"),
        &[FORM],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn oversized_and_malformed_forms_are_bad_requests() {
    let app = TestApp::new();
    let mut browser = Browser::new([203, 0, 113, 1]);
    browser.get(&app, "/").await;
    let token = browser.cookies["__Host-csrf"].clone();
    let huge = format!("csrf={token}&to=en&back=home&pad={}", "x".repeat(20_000));
    let request = browser
        .request("POST", "/lang")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ORIGIN, ORIGIN_URL)
        .body(Body::from(huge))
        .unwrap();
    assert_eq!(app.send(request).await.status, StatusCode::BAD_REQUEST);
    let status = post(
        &app,
        &browser,
        "/lang",
        format!("csrf={token}&to=fr&back=home"),
        &[FORM],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "an unknown language");
    let status = post(
        &app,
        &browser,
        "/lang",
        format!("csrf={token}&to=en&back=https://evil.example"),
        &[FORM],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "only named pages are accepted"
    );
}
