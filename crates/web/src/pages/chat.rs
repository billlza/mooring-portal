//! What the chat script is answered (`assets/js/chat.mjs`), on the
//! feedback page and in the box on a signed-in user's pages, and on the
//! operator's page of one conversation.
//!
//! The script asks for the messages after the last its page shows, by the
//! number of that message (`?after=`), and sends a message as the page's
//! form does, CSRF token included. Its questions carry the page's token
//! too, and are answered only while it is the browser's
//! (`security::csrf::FromCurrentPage`): a page shown before someone signed
//! in or out is answered 403. Each answer is a JSON document of this
//! module's types, or a status alone; nothing in it is markup, and the
//! script writes it into the page as text. A request the pages would turn
//! away is turned away by status: a signed-out user is answered 401, and
//! the operator pages answer as they answer any of their requests
//! (`pages::operator`). The script tells its reader to load the page again
//! when it meets any of these.

use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::view::MessageView;

/// The script's question: the messages after which.
#[derive(Debug, Deserialize)]
pub(crate) struct AfterQuery {
    /// The number of the last message the page shows; 0 if it shows none.
    pub(crate) after: u64,
}

/// The messages asked for, oldest first.
#[derive(Debug, Serialize)]
struct Messages<'a> {
    messages: &'a [MessageView],
}

/// Whether a reply waits unread.
#[derive(Debug, Serialize)]
struct Unread {
    unread: bool,
}

/// Why a message was not sent, in the reader's language.
#[derive(Debug, Serialize)]
struct Refusal<'a> {
    error: &'a str,
}

/// `messages`, oldest first.
pub(crate) fn messages(messages: &[MessageView]) -> Result<Response, AppError> {
    json(StatusCode::OK, &Messages { messages })
}

/// Whether a reply waits unread.
pub(crate) fn unread(unread: bool) -> Result<Response, AppError> {
    json(StatusCode::OK, &Unread { unread })
}

/// A message refused with `status`, for `reason`.
pub(crate) fn refused(status: StatusCode, reason: &str) -> Result<Response, AppError> {
    json(status, &Refusal { error: reason })
}

/// A message stored: the script asks for it with whatever else came.
pub(crate) fn sent() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// No live session: the script tells its reader to load the page again,
/// which logs them in first.
pub(crate) fn signed_out() -> Response {
    StatusCode::UNAUTHORIZED.into_response()
}

fn json(status: StatusCode, document: &impl Serialize) -> Result<Response, AppError> {
    let body = serde_json::to_vec(document).map_err(AppError::internal)?;
    Ok((
        status,
        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        body,
    )
        .into_response())
}
