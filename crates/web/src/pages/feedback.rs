//! The feedback page: the signed-in account's conversation with the
//! operator (`portal_core::feedback`). Every other page of a signed-in
//! account offers the box that writes to it (`base.html`).
//!
//! A message is sent, and the conversation read, for the account of the
//! session the store finds in the same transaction; no address or form
//! names an account. The site sends no email: the owner reads the
//! operator's reply here, and the box says when one is unread. Where the
//! page runs the chat script, the conversation it shows stays current and
//! a message is sent without leaving the page (`pages::chat`); each
//! message stored tells the host, for the operator, that one waits
//! (`crate::NewMessageSignal`).

use askama::Template;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use portal_core::auth::TokenDigest;
use portal_core::feedback::{MAX_MESSAGE_CHARS, MessageBody, MessageBodyError};
use portal_store::{FeedbackId, MAX_SENT_PER_DAY, SendError};
use serde::Deserialize;

use super::chat::{self, AfterQuery};
use super::{
    Chrome, Page, PageContext, PageQuery, login_first, render, render_status, request_lang,
    see_other,
};
use crate::error::AppError;
use crate::i18n::{Strings, fill};
use crate::security::csrf::{FromCurrentPage, MAX_FORM_BYTES, VerifiedForm};
use crate::security::session::{Viewer, presented_token};
use crate::state::AppState;
use crate::view::{MessageView, Reader};

#[derive(Template)]
#[template(path = "feedback.html")]
struct FeedbackPage {
    chrome: Chrome,
    notice: Option<&'static str>,
    error: Option<String>,
    messages: Vec<MessageView>,
    /// What the form holds: a message sent back with its error.
    draft: String,
}

crate::pages::page_templates!(FeedbackPage);

impl FeedbackPage {
    /// The number of the last message shown, which the chat script asks
    /// for the messages after; 0 if there is none.
    fn after(&self) -> u64 {
        self.messages.last().map_or(0, |message| message.id)
    }
}

/// A one-off message after a redirect, named by the redirect itself.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notice {
    Sent,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FeedbackQuery {
    notice: Option<Notice>,
}

/// A message, from this page or the box on any other.
#[derive(Debug, Deserialize)]
pub(crate) struct MessageForm {
    body: String,
}

/// `GET /feedback`: the conversation, which counts as read from now.
pub(crate) async fn feedback(
    State(state): State<AppState>,
    ctx: PageContext,
    headers: HeaderMap,
    PageQuery(query): PageQuery<FeedbackQuery>,
) -> Result<Response, AppError> {
    let Some(messages) = read(&state, &headers, FeedbackId::START, ctx.t()).await? else {
        return Ok(login_first(Page::Feedback));
    };
    render(&FeedbackPage {
        notice: query.notice.map(|Notice::Sent| ctx.t().feedback.sent),
        error: None,
        messages,
        draft: String::new(),
        chrome: chrome(&ctx),
    })
}

/// `POST /feedback`: sends the message to the operator.
pub(crate) async fn send(
    State(state): State<AppState>,
    ctx: PageContext,
    headers: HeaderMap,
    VerifiedForm(form): VerifiedForm<MessageForm>,
) -> Result<Response, AppError> {
    let Some(session) = presented_token(&headers) else {
        return Ok(login_first(Page::Feedback));
    };
    let t = ctx.t();
    let body = match MessageBody::parse(&form.body) {
        Ok(body) => body,
        Err(problem) => {
            return refuse(
                &state,
                &ctx,
                &headers,
                form.body,
                message_problem(t, problem, t.feedback.blank),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await;
        }
    };
    match deliver(&state, session.digest(), body).await? {
        Ok(()) => Ok(see_other("/feedback?notice=sent")),
        Err(SendError::SignedOut) => Ok(login_first(Page::Feedback)),
        Err(SendError::TooMany) => {
            refuse(
                &state,
                &ctx,
                &headers,
                form.body,
                fill(t.feedback.too_many, MAX_SENT_PER_DAY),
                StatusCode::TOO_MANY_REQUESTS,
            )
            .await
        }
        Err(SendError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// `GET /feedback/unread`: whether a reply waits unread, for the script of
/// a box that is closed, as long as the browser holds the token the box's
/// page was shown with. Asking marks nothing.
pub(crate) async fn unread(
    _: FromCurrentPage,
    Viewer(viewer): Viewer,
) -> Result<Response, AppError> {
    match viewer {
        Some((_, signals)) => chat::unread(signals.unread_reply),
        None => Ok(chat::signed_out()),
    }
}

/// `GET /feedback/messages?after=<n>`: the messages after number `n`, for
/// the script of a page that shows the conversation, as long as the browser
/// holds the token that page was shown with: a page rendered for one
/// account never reads another's. Every reply counts as read from now, as
/// it does when the feedback page is read.
pub(crate) async fn messages(
    _: FromCurrentPage,
    State(state): State<AppState>,
    headers: HeaderMap,
    PageQuery(query): PageQuery<AfterQuery>,
) -> Result<Response, AppError> {
    let t = request_lang(&headers).strings();
    match read(&state, &headers, FeedbackId::new(query.after), t).await? {
        Some(messages) => chat::messages(&messages),
        None => Ok(chat::signed_out()),
    }
}

/// `POST /feedback/messages`: sends the message, as `POST /feedback` does,
/// for the script, which then asks for it with whatever else came.
pub(crate) async fn send_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    VerifiedForm(form): VerifiedForm<MessageForm>,
) -> Result<Response, AppError> {
    let Some(session) = presented_token(&headers) else {
        return Ok(chat::signed_out());
    };
    let t = request_lang(&headers).strings();
    let body = match MessageBody::parse(&form.body) {
        Ok(body) => body,
        Err(problem) => {
            return chat::refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                &message_problem(t, problem, t.feedback.blank),
            );
        }
    };
    match deliver(&state, session.digest(), body).await? {
        Ok(()) => Ok(chat::sent()),
        Err(SendError::SignedOut) => Ok(chat::signed_out()),
        Err(SendError::TooMany) => chat::refused(
            StatusCode::TOO_MANY_REQUESTS,
            &fill(t.feedback.too_many, MAX_SENT_PER_DAY),
        ),
        Err(SendError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// Stores `body` as a message to the operator from the account signed in
/// with the session whose token has `session`, and once it is stored tells
/// the host that a message waits.
///
/// Both run in a task of their own. A request is dropped when its browser
/// goes away, and a request dropped between the two would leave a stored
/// message the operator is never told of; the task runs on to the end.
async fn deliver(
    state: &AppState,
    session: TokenDigest,
    body: MessageBody,
) -> Result<Result<(), SendError>, AppError> {
    let now = state.now()?;
    let state = state.clone();
    tokio::spawn(async move {
        let sent = state
            .database()
            .run(move |store| store.send_feedback(&session, &body, now))
            .await?;
        if sent.is_ok() {
            state.signal_new_message(now).await;
        }
        Ok::<_, AppError>(sent)
    })
    .await
    .map_err(AppError::internal)?
}

// The longest message, every character of it four bytes long and every
// byte sent as `%XX`, fits in a form with room for the CSRF token and the
// field names, so that no message the page allows is refused as a form.
const _: () = assert!(MAX_MESSAGE_CHARS * 4 * 3 + 1024 <= MAX_FORM_BYTES);

/// What is wrong with a message, in the reader's language; `blank` says
/// what a blank one lacks.
pub(crate) fn message_problem(t: &Strings, problem: MessageBodyError, blank: &str) -> String {
    match problem {
        MessageBodyError::Blank => blank.to_owned(),
        MessageBodyError::TooLong => fill(t.feedback.too_long, MAX_MESSAGE_CHARS),
        MessageBodyError::ControlCharacter => t.feedback.control.to_owned(),
    }
}

/// The page with `draft` sent back in the form, and `error`.
async fn refuse(
    state: &AppState,
    ctx: &PageContext,
    headers: &HeaderMap,
    draft: String,
    error: String,
    status: StatusCode,
) -> Result<Response, AppError> {
    let Some(messages) = read(state, headers, FeedbackId::START, ctx.t()).await? else {
        return Ok(login_first(Page::Feedback));
    };
    render_status(
        status,
        &FeedbackPage {
            notice: None,
            error: Some(error),
            messages,
            draft,
            chrome: chrome(ctx),
        },
    )
}

/// The messages after `after` of the conversation of the account signed in
/// with the session `headers` present, which its owner reads now; `None`
/// if no session is live. The session is read with the conversation, not
/// taken from when the request's headers arrived: the account it had then
/// may be gone, and its number another account's.
async fn read(
    state: &AppState,
    headers: &HeaderMap,
    after: FeedbackId,
    t: &'static Strings,
) -> Result<Option<Vec<MessageView>>, AppError> {
    let Some(token) = presented_token(headers) else {
        return Ok(None);
    };
    let digest = token.digest();
    let now = state.now()?;
    let messages = state
        .database()
        .run(move |store| store.feedback_for_session(&digest, after, now))
        .await?
        .map_err(AppError::internal)?;
    Ok(messages.map(|messages| {
        MessageView::all(
            &messages,
            Reader::Owner,
            t.feedback.you,
            t.feedback.operator,
        )
    }))
}

/// The page's surroundings. The page has just been read, so no reply is
/// unread any more; the chat script keeps it current.
fn chrome(ctx: &PageContext) -> Chrome {
    let mut chrome = ctx.chrome(Page::Feedback).with_live_thread();
    chrome.signals.unread_reply = false;
    chrome
}
