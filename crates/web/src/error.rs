//! Request failures and the pages that report them.
//!
//! Error pages are bilingual and self-contained: they depend on neither the
//! session, the database nor the language cookie, any of which may be what
//! failed, and they are built from fixed text, so building one cannot fail.
//! Internal errors are logged with their causes and shown only as a
//! generic message.

use std::error::Error as StdError;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::assets::STABLE_STYLESHEET;

/// Why a request failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// No such page.
    #[error("not found")]
    NotFound,
    /// A state-changing request failed the CSRF or origin checks.
    #[error("forbidden: {0}")]
    Forbidden(&'static str),
    /// The request is malformed. The reason is fixed text, never anything
    /// the request carried, so logging it lets no client write the log.
    #[error("bad request: {0}")]
    BadRequest(&'static str),
    /// A server-side failure.
    #[error("internal error: {0}")]
    Internal(Box<dyn StdError + Send + Sync>),
}

impl AppError {
    /// An internal error caused by `error`.
    pub(crate) fn internal(error: impl StdError + Send + Sync + 'static) -> Self {
        Self::Internal(Box::new(error))
    }

    /// An internal error described by `message`.
    pub(crate) fn internal_message(message: impl Into<String>) -> Self {
        Self::Internal(message.into().into())
    }
}

impl From<askama::Error> for AppError {
    fn from(error: askama::Error) -> Self {
        Self::internal(error)
    }
}

/// `error` and its causes, joined by `: `.
pub(crate) fn chain(error: &dyn StdError) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

impl AppError {
    /// The failure as plain text, for an address a browser must not render
    /// as a page: a subscription link's address holds its token, which a
    /// page's stylesheet and links would send on as the referrer.
    pub(crate) fn plain(self) -> Response {
        let (status, zh, en) = self.report();
        (status, format!("{zh}\n{en}\n")).into_response()
    }

    /// The status and the text in both languages, logging the failure as
    /// its kind requires.
    fn report(&self) -> (StatusCode, &'static str, &'static str) {
        match self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "找不到这个页面。",
                "This page does not exist.",
            ),
            Self::Forbidden(reason) => {
                tracing::warn!(reason, "refused a state-changing request");
                (
                    StatusCode::FORBIDDEN,
                    "表单已过期，或者请求不是从本站发出的。请返回上一页，刷新后重试。",
                    "The form has expired, or the request did not come from this site. \
                     Go back, reload the page and try again.",
                )
            }
            Self::BadRequest(reason) => {
                tracing::info!(reason, "refused a malformed request");
                (
                    StatusCode::BAD_REQUEST,
                    "请求无效。",
                    "The request is not valid.",
                )
            }
            Self::Internal(error) => {
                tracing::error!(error = %chain(error.as_ref()), "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "服务器出错了，请稍后再试。如果一直这样，请告诉运营者。",
                    "The server ran into a problem. Try again later, and tell the \
                     operator if it keeps happening.",
                )
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, zh, en) = self.report();
        (status, Html(error_page(status, zh, en))).into_response()
    }
}

/// The bilingual page for `status`. Every argument is fixed text, so
/// nothing needs escaping.
fn error_page(status: StatusCode, zh: &str, en: &str) -> String {
    let code = status.as_u16();
    format!(
        "<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"robots\" content=\"noindex\">\n<title>{code}</title>\n\
         <link rel=\"stylesheet\" href=\"{STABLE_STYLESHEET}\">\n</head>\n\
         <body class=\"error-page\">\n<main class=\"error-card\">\n\
         <p class=\"error-code\">{code}</p>\n<p>{zh}</p>\n<p lang=\"en\">{en}</p>\n\
         <p><a class=\"button\" href=\"/\">返回首页 · Home</a></p>\n</main>\n</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("outer")]
    struct Outer(#[source] std::io::Error);

    #[test]
    fn chains_join_every_cause() {
        let error = Outer(std::io::Error::other("inner"));
        assert_eq!(chain(&error), "outer: inner");
        assert_eq!(
            AppError::internal_message("plain").to_string(),
            "internal error: plain"
        );
    }

    #[test]
    fn statuses_match_the_failure() {
        for (error, status) in [
            (AppError::NotFound, StatusCode::NOT_FOUND),
            (AppError::Forbidden("csrf"), StatusCode::FORBIDDEN),
            (AppError::BadRequest("x"), StatusCode::BAD_REQUEST),
            (
                AppError::internal_message("boom"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ] {
            assert_eq!(error.into_response().status(), status);
        }
    }

    #[test]
    fn plain_failures_carry_the_same_status_and_text_and_nothing_else() {
        let response = AppError::NotFound.plain();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            AppError::internal_message("boom").plain().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn error_pages_are_bilingual_and_link_the_stable_stylesheet() {
        let page = error_page(StatusCode::NOT_FOUND, "找不到", "Not here");
        assert!(page.contains("<p class=\"error-code\">404</p>"));
        assert!(page.contains("找不到") && page.contains("Not here"));
        assert!(page.contains(STABLE_STYLESHEET));
    }
}
