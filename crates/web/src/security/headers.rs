//! Security headers on every response.
//!
//! The content security policy allows only this origin's stylesheets and
//! images and no scripts at all: the portal works without JavaScript, so a
//! script that got into a page would not run. Pages are not cached (they
//! carry CSRF tokens and account data); hashed assets set their own policy.
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
use axum::http::{HeaderName, HeaderValue, Response};
use axum::middleware::Next;

/// The headers set on every response.
const HEADERS: [(HeaderName, &str); 8] = [
    (
        CONTENT_SECURITY_POLICY,
        "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; \
         frame-ancestors 'none'; base-uri 'none'",
    ),
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

/// Middleware: adds [`HEADERS`], and `Cache-Control: no-store` unless the
/// handler chose a cache policy.
pub(crate) async fn add(request: Request, next: Next) -> Response<Body> {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in HEADERS {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if !headers.contains_key(CACHE_CONTROL) {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}
