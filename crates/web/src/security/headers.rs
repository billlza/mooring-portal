//! Security headers on every response.
//!
//! The content security policy allows only this origin's stylesheets and
//! images, and on every page but one no script at all: the pages that hold
//! a form, an account or a subscription link work without JavaScript, so a
//! script that got into one would not run. Pages are not cached (they carry
//! CSRF tokens and account data); hashed assets set their own policy.
//!
//! The one exception is the page the router names, the home page, which
//! may run this origin's own script files: they draw its live scene
//! (`assets`). Inline script, and script from anywhere else, stay refused
//! there too. Under `default-src 'none'` a script there can open no
//! connection of its own, and `worker-src 'none'` keeps it from starting
//! a worker; it can still do what the page may, which is to load this
//! origin's images, submit this origin's forms, and send the browser to
//! another address. The home page shows nothing a visitor typed, and no
//! response of this server that a visitor shapes is served as script, so
//! the exception gives a stranger nothing to run (`docs/security.md`,
//! section 6).
//!
//! The referrer policy is `same-origin`, not `no-referrer`: under
//! `no-referrer`, browsers send `Origin: null` with this site's own form
//! posts (Fetch standard, "append a request Origin header"), which the
//! CSRF origin check must refuse. Links to other sites still carry no
//! referrer.

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY,
    X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderName, HeaderValue, Method, Response, StatusCode};
use axum::middleware::Next;

/// What every page's policy says.
macro_rules! base_policy {
    () => {
        "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; \
         frame-ancestors 'none'; base-uri 'none'"
    };
}

/// The policy of every response but the scripted page's: no script.
const POLICY: &str = base_policy!();
/// The scripted page's policy: this origin's script files, and nothing
/// else that the base policy refuses. Workers are refused by name, because
/// a policy that allows scripts would otherwise allow them too.
const SCRIPTED_POLICY: &str = concat!(base_policy!(), "; script-src 'self'; worker-src 'none'");

/// The other headers set on every response.
const HEADERS: [(HeaderName, &str); 7] = [
    (X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (X_FRAME_OPTIONS, "DENY"),
    (REFERRER_POLICY, "same-origin"),
    (STRICT_TRANSPORT_SECURITY, "max-age=31536000"),
    (
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    ),
    (
        HeaderName::from_static("cross-origin-resource-policy"),
        "same-origin",
    ),
    (
        HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    ),
];

/// Middleware: adds the content security policy and [`HEADERS`], and
/// `Cache-Control: no-store` unless the handler chose a cache policy.
/// `scripted` is the path of the one page that may run this origin's
/// scripts: the page itself, as a browser fetches it and as it is served
/// when all is well. Nothing else answered at its address may, an error
/// page least of all. The policy is set here, whatever a handler set.
pub(crate) async fn add(scripted: &'static str, request: Request, next: Next) -> Response<Body> {
    let fetched = request.method() == Method::GET || request.method() == Method::HEAD;
    let asked_for_page = fetched && request.uri().path() == scripted;
    let mut response = next.run(request).await;
    let policy = if asked_for_page && response.status() == StatusCode::OK {
        SCRIPTED_POLICY
    } else {
        POLICY
    };
    let headers = response.headers_mut();
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(policy));
    for (name, value) in HEADERS {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if !headers.contains_key(CACHE_CONTROL) {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::middleware;
    use axum::routing::get;
    use tower::ServiceExt as _;

    use super::*;

    /// The status and policy a small site answers with when `scripted` is
    /// its scripted page. Its page at `/` answers a form post as it answers
    /// a fetch, which no page of the portal does: the rule is the
    /// middleware's own, and does not lean on the router.
    async fn answer(scripted: &'static str, method: &str, path: &str) -> (StatusCode, String) {
        async fn page() -> &'static str {
            "the page"
        }
        async fn failed() -> StatusCode {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        let site = Router::new()
            .route("/", get(page).post(page))
            .route("/failed", get(failed))
            .layer(middleware::from_fn(move |request, next| {
                add(scripted, request, next)
            }));
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .unwrap();
        let response = site.oneshot(request).await.unwrap();
        let policy = response.headers()[CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_owned();
        (response.status(), policy)
    }

    #[tokio::test]
    async fn only_the_page_fetched_and_served_whole_may_run_scripts() {
        for method in ["GET", "HEAD"] {
            assert_eq!(
                answer("/", method, "/").await,
                (StatusCode::OK, SCRIPTED_POLICY.to_owned()),
                "{method}"
            );
        }
        // Not what the page answers to a form, though it answers it well.
        assert_eq!(
            answer("/", "POST", "/").await,
            (StatusCode::OK, POLICY.to_owned())
        );
        // Not the page when it fails.
        assert_eq!(
            answer("/failed", "GET", "/failed").await,
            (StatusCode::INTERNAL_SERVER_ERROR, POLICY.to_owned())
        );
        // And not any other address.
        assert_eq!(
            answer("/failed", "GET", "/").await,
            (StatusCode::OK, POLICY.to_owned())
        );
        assert_eq!(
            SCRIPTED_POLICY,
            format!("{POLICY}; script-src 'self'; worker-src 'none'")
        );
    }
}
