//! The portal's cookies.
//!
//! Every cookie carries the `__Host-` prefix, so browsers accept it only
//! with `Secure`, `Path=/` and no `Domain`: a sibling subdomain or a plain
//! HTTP response cannot plant or overwrite one. All are `HttpOnly`.

use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue};

use crate::error::AppError;

/// The session token.
pub(crate) const SESSION: &str = "__Host-session";
/// The CSRF token, compared with the one each form carries.
pub(crate) const CSRF: &str = "__Host-csrf";
/// The chosen language.
pub(crate) const LANG: &str = "__Host-lang";

/// Which cross-site requests carry a cookie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SameSite {
    /// Top-level navigations from other sites carry it.
    Lax,
    /// Only requests from this site carry it.
    Strict,
}

/// The value of the first cookie named `name` in the request, if it is
/// UTF-8. The header is split as bytes, so a cookie of another site's that
/// is not ASCII (a parent domain's, or another app's on localhost) hides
/// none of ours; callers parse the value they get strictly.
pub(crate) fn read<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b';'))
        .filter_map(|pair| {
            let mut parts = pair.trim_ascii().splitn(2, |byte| *byte == b'=');
            Some((parts.next()?, parts.next()?))
        })
        .find(|(key, _)| *key == name.as_bytes())
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
}

/// A `Set-Cookie` value; without `max_age_secs` the cookie lasts until the
/// browser closes.
pub(crate) fn set(
    name: &str,
    value: &str,
    same_site: SameSite,
    max_age_secs: Option<u64>,
) -> Result<HeaderValue, AppError> {
    let same_site = match same_site {
        SameSite::Lax => "Lax",
        SameSite::Strict => "Strict",
    };
    let mut text = format!("{name}={value}; Path=/; Secure; HttpOnly; SameSite={same_site}");
    if let Some(secs) = max_age_secs {
        text.push_str(&format!("; Max-Age={secs}"));
    }
    HeaderValue::try_from(text).map_err(AppError::internal)
}

/// A `Set-Cookie` value that deletes the cookie `name`.
pub(crate) fn clear(name: &str) -> Result<HeaderValue, AppError> {
    set(name, "", SameSite::Lax, Some(0))
}

/// Whether `headers` (of a response) already set the cookie `name`.
pub(crate) fn sets(headers: &HeaderMap, name: &str) -> bool {
    headers
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.split_once('=').is_some_and(|(key, _)| key == name))
}

#[cfg(test)]
mod tests {
    use axum::http::header::SET_COOKIE;

    use super::*;

    #[test]
    fn the_first_cookie_of_a_name_is_read() {
        let mut headers = HeaderMap::new();
        headers.append(
            COOKIE,
            HeaderValue::from_static("a=1; __Host-csrf=abc ;b=2"),
        );
        headers.append(COOKIE, HeaderValue::from_static("__Host-csrf=later; c"));
        assert_eq!(read(&headers, CSRF), Some("abc"));
        assert_eq!(read(&headers, "b"), Some("2"));
        assert_eq!(read(&headers, "c"), None);
        assert_eq!(read(&headers, SESSION), None);
        headers.append(COOKIE, HeaderValue::from_bytes(b"x=\xff").unwrap());
        assert_eq!(read(&headers, "x"), None);
    }

    #[test]
    fn a_cookie_that_is_not_ascii_hides_no_other() {
        // A parent domain's cookie, or another local app's, may hold any
        // bytes; browsers send it in the same header as ours.
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_bytes(b"theirs=caf\xc3\xa9; __Host-csrf=abc; __Host-session=t0k3n")
                .unwrap(),
        );
        assert_eq!(read(&headers, CSRF), Some("abc"));
        assert_eq!(read(&headers, SESSION), Some("t0k3n"));
        assert_eq!(read(&headers, "theirs"), Some("café"));
        // One of ours that is not text at all is absent, not guessed at.
        headers.insert(
            COOKIE,
            HeaderValue::from_bytes(b"__Host-csrf=\xff; __Host-session=t0k3n").unwrap(),
        );
        assert_eq!(read(&headers, CSRF), None);
        assert_eq!(read(&headers, SESSION), Some("t0k3n"));
    }

    #[test]
    fn cookies_are_secure_host_only_and_http_only() {
        assert_eq!(
            set(SESSION, "t0k3n", SameSite::Lax, Some(60)).unwrap(),
            "__Host-session=t0k3n; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=60"
        );
        assert_eq!(
            set(CSRF, "abc", SameSite::Strict, None).unwrap(),
            "__Host-csrf=abc; Path=/; Secure; HttpOnly; SameSite=Strict"
        );
        assert_eq!(
            clear(SESSION).unwrap(),
            "__Host-session=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0"
        );
        assert!(matches!(
            set(SESSION, "bad\nvalue", SameSite::Lax, None),
            Err(AppError::Internal(_))
        ));
    }

    #[test]
    fn responses_are_searched_for_a_cookie() {
        let mut headers = HeaderMap::new();
        assert!(!sets(&headers, CSRF));
        headers.append(SET_COOKIE, set(LANG, "en", SameSite::Lax, None).unwrap());
        assert!(!sets(&headers, CSRF));
        headers.append(SET_COOKIE, set(CSRF, "x", SameSite::Strict, None).unwrap());
        assert!(sets(&headers, CSRF));
    }
}
