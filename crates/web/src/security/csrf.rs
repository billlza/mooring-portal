//! Cross-site request forgery protection.
//!
//! Every page request carries a CSRF token: the one in the browser's
//! `__Host-csrf` cookie, or a fresh one issued with the response. Every
//! form embeds it, and [`VerifiedForm`] accepts a form only when
//!
//! 1. `Sec-Fetch-Site`, if sent, says the request came from this origin
//!    (or from the user, `none`);
//! 2. `Origin`, if sent, is the portal's configured origin;
//! 3. the body is a URL-encoded form whose `csrf` field equals the
//!    cookie's token (compared in constant time).
//!
//! The cookie cannot be planted from another site or a subdomain (see
//! [`super::cookies`]), so a forged form cannot know the token.

use axum::body::Body;
use axum::extract::{FromRequest, Request};
use axum::http::header::{CONTENT_TYPE, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, Response};
use axum::middleware::Next;
use portal_core::auth::SecretToken;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::cookies::{self, SameSite};
use crate::config::PublicOrigin;
use crate::error::AppError;
use crate::state::AppState;

/// The largest form body accepted.
pub(crate) const MAX_FORM_BYTES: usize = 16 * 1024;

/// This request's CSRF token.
#[derive(Debug, Clone)]
pub(crate) struct CsrfToken {
    token: SecretToken,
    /// The browser sent no valid token; this one is new.
    issued: bool,
}

impl CsrfToken {
    /// The token as forms embed it.
    pub(crate) fn form_value(&self) -> String {
        self.token.encode()
    }
}

/// Middleware: gives every request a [`CsrfToken`], setting the cookie
/// when the browser had none (unless the handler set a new one itself).
pub(crate) async fn ensure_token(
    mut request: Request,
    next: Next,
) -> Result<Response<Body>, AppError> {
    let existing = cookies::read(request.headers(), cookies::CSRF)
        .and_then(|value| SecretToken::parse(value).ok());
    let token = match existing {
        Some(token) => CsrfToken {
            token,
            issued: false,
        },
        None => CsrfToken {
            token: SecretToken::generate().map_err(AppError::internal)?,
            issued: true,
        },
    };
    request.extensions_mut().insert(token.clone());
    let mut response = next.run(request).await;
    if token.issued && !cookies::sets(response.headers(), cookies::CSRF) {
        let cookie = cookies::set(cookies::CSRF, &token.form_value(), SameSite::Strict, None)?;
        response.headers_mut().append(SET_COOKIE, cookie);
    }
    Ok(response)
}

/// A `Set-Cookie` value with a fresh CSRF token, for logins and logouts.
pub(crate) fn rotated_cookie() -> Result<axum::http::HeaderValue, AppError> {
    let token = SecretToken::generate().map_err(AppError::internal)?;
    cookies::set(cookies::CSRF, &token.encode(), SameSite::Strict, None)
}

#[derive(Deserialize)]
struct CsrfField {
    csrf: String,
}

/// A URL-encoded form that passed the CSRF checks.
#[derive(Debug)]
pub(crate) struct VerifiedForm<T>(pub(crate) T);

impl<T: DeserializeOwned> FromRequest<AppState> for VerifiedForm<T> {
    type Rejection = AppError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, AppError> {
        check_origin(request.headers(), &state.config().origin)?;
        let is_form = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"));
        if !is_form {
            return Err(AppError::BadRequest("the body is not a URL-encoded form"));
        }
        let expected = request
            .extensions()
            .get::<CsrfToken>()
            .cloned()
            .ok_or_else(|| AppError::internal_message("the CSRF middleware did not run"))?;
        let body = axum::body::to_bytes(request.into_body(), MAX_FORM_BYTES)
            .await
            .map_err(|_| AppError::BadRequest("the form is too large or unreadable"))?;
        let field: CsrfField = serde_urlencoded::from_bytes(&body)
            .map_err(|_| AppError::Forbidden("the form carries no CSRF token"))?;
        let presented = SecretToken::parse(&field.csrf)
            .map_err(|_| AppError::Forbidden("the form's CSRF token is malformed"))?;
        if expected.issued || !presented.matches(&expected.token) {
            return Err(AppError::Forbidden(
                "the form's CSRF token does not match the cookie",
            ));
        }
        // The decoder's message quotes what the form held; drop it.
        let form = serde_urlencoded::from_bytes(&body)
            .map_err(|_| AppError::BadRequest("the form's fields are missing or malformed"))?;
        Ok(Self(form))
    }
}

/// Refuses requests that browsers mark as coming from elsewhere.
fn check_origin(headers: &HeaderMap, origin: &PublicOrigin) -> Result<(), AppError> {
    if let Some(site) = headers.get("sec-fetch-site")
        && !matches!(site.as_bytes(), b"same-origin" | b"none")
    {
        return Err(AppError::Forbidden(
            "Sec-Fetch-Site says the request is from another site",
        ));
    }
    if let Some(sent) = headers.get(ORIGIN)
        && sent.as_bytes() != origin.as_str().as_bytes()
    {
        return Err(AppError::Forbidden(
            "the request's Origin is not the portal's",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn origin() -> PublicOrigin {
        PublicOrigin::parse("https://portal.example").unwrap()
    }

    #[test]
    fn same_origin_and_user_initiated_requests_pass() {
        let mut headers = HeaderMap::new();
        check_origin(&headers, &origin()).unwrap();
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://portal.example"));
        check_origin(&headers, &origin()).unwrap();
        headers.insert("sec-fetch-site", HeaderValue::from_static("none"));
        check_origin(&headers, &origin()).unwrap();
    }

    #[test]
    fn requests_from_elsewhere_are_refused() {
        for (site, sent) in [
            (Some("cross-site"), None),
            (Some("same-site"), None),
            (None, Some("https://evil.example")),
            (None, Some("null")),
            (None, Some("http://portal.example")),
        ] {
            let mut headers = HeaderMap::new();
            if let Some(site) = site {
                headers.insert("sec-fetch-site", HeaderValue::from_static(site));
            }
            if let Some(sent) = sent {
                headers.insert(ORIGIN, HeaderValue::from_static(sent));
            }
            assert!(
                matches!(
                    check_origin(&headers, &origin()),
                    Err(AppError::Forbidden(_))
                ),
                "{site:?} {sent:?}"
            );
        }
    }

    #[test]
    fn rotated_cookies_carry_a_fresh_token() {
        let first = rotated_cookie().unwrap();
        let second = rotated_cookie().unwrap();
        assert_ne!(first, second);
        assert!(first.to_str().unwrap().starts_with("__Host-csrf="));
    }
}
