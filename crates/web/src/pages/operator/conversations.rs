//! The operator's side of feedback: every account's conversation, and
//! replying to one. The page of one conversation runs the chat script,
//! which keeps it current and sends a reply without leaving the page
//! (`pages::chat`); its requests pass the same checks as the page's.

use askama::Template;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use portal_core::account::AccountCode;
use portal_core::feedback::MessageBody;
use portal_store::{AsOperator, FeedbackId, FeedbackMessage, ReplyError};
use serde::Deserialize;

use super::{Confirmed, OperatorForm, Section, chrome, turned_away};
use crate::error::AppError;
use crate::format::beijing;
use crate::i18n::{Strings, fill};
use crate::pages::chat::{self, AfterQuery};
use crate::pages::feedback::message_problem;
use crate::pages::{
    Chrome, PageContext, PageQuery, render, render_status, request_lang, see_other,
};
use crate::security::csrf::FromCurrentPage;
use crate::state::AppState;
use crate::view::{MessageView, Reader};

#[derive(Template)]
#[template(path = "operator_feedback.html")]
struct ConversationsPage {
    chrome: Chrome,
    section: Section,
    conversations: Vec<ConversationRow>,
}

/// A conversation, as the list shows it.
struct ConversationRow {
    account: String,
    email: String,
    last: String,
    awaiting_reply: bool,
    messages: u64,
}

#[derive(Template)]
#[template(path = "operator_conversation.html")]
struct ConversationPage {
    chrome: Chrome,
    section: Section,
    title: String,
    account: String,
    notice: Option<&'static str>,
    error: Option<String>,
    messages: Vec<MessageView>,
    /// What the form holds: a reply sent back with its error.
    draft: String,
}

crate::pages::page_templates!(ConversationsPage, ConversationPage);

impl ConversationPage {
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
pub(crate) struct ConversationQuery {
    notice: Option<Notice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReplyForm {
    body: String,
}

/// `GET /operator/feedback`: every conversation, those awaiting a reply
/// first.
pub(crate) async fn conversations(
    _: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let conversations = state
        .database()
        .run(|store| store.conversations())
        .await?
        .map_err(AppError::internal)?;
    render(&ConversationsPage {
        section: Section::Feedback,
        conversations: conversations
            .into_iter()
            .map(|conversation| ConversationRow {
                account: conversation.account.to_string(),
                email: conversation.email.as_str().to_owned(),
                last: beijing(conversation.last_sent_at),
                awaiting_reply: conversation.awaiting_reply,
                messages: conversation.messages,
            })
            .collect(),
        chrome: chrome(&ctx),
    })
}

/// `GET /operator/feedback/<code>`: the account's conversation, with the
/// reply form.
pub(crate) async fn conversation(
    _: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
    code: Result<Path<String>, PathRejection>,
    PageQuery(query): PageQuery<ConversationQuery>,
) -> Result<Response, AppError> {
    let code = account_of(code)?;
    show(
        &state,
        &ctx,
        code,
        Shown {
            notice: query.notice.map(|Notice::Sent| ctx.t().operator.reply_sent),
            error: None,
            draft: String::new(),
            status: StatusCode::OK,
        },
    )
    .await
}

/// `POST /operator/feedback/<code>`: sends the operator's reply to the
/// account.
pub(crate) async fn reply(
    State(state): State<AppState>,
    ctx: PageContext,
    code: Result<Path<String>, PathRejection>,
    OperatorForm { gate, form }: OperatorForm<ReplyForm>,
) -> Result<Response, AppError> {
    let code = account_of(code)?;
    let body = match MessageBody::parse(&form.body) {
        Ok(body) => body,
        Err(problem) => {
            return show(
                &state,
                &ctx,
                code,
                Shown {
                    notice: None,
                    error: Some(message_problem(
                        ctx.t(),
                        problem,
                        ctx.t().operator.reply_blank,
                    )),
                    draft: form.body,
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                },
            )
            .await;
        }
    };
    let now = state.now()?;
    let replied = state
        .database()
        .run(move |store| store.as_operator(&gate.digest, now, |pages| pages.reply(&code, &body)))
        .await?;
    match replied {
        Ok(AsOperator::Done(())) => Ok(see_other(&format!(
            "{}/{code}?notice=sent",
            Section::Feedback.path()
        ))),
        Ok(AsOperator::NotAllowed(not)) => Ok(turned_away(not, Section::Feedback)),
        Err(ReplyError::UnknownAccount(_)) => Err(AppError::NotFound),
        Err(ReplyError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// `GET /operator/feedback/<code>/messages?after=<n>`: the messages after
/// number `n` of the account's conversation, for the script of its page,
/// as long as the browser holds the token that page was shown with. The
/// operator reading them marks nothing.
pub(crate) async fn conversation_messages(
    _: Confirmed,
    _: FromCurrentPage,
    State(state): State<AppState>,
    headers: HeaderMap,
    code: Result<Path<String>, PathRejection>,
    PageQuery(query): PageQuery<AfterQuery>,
) -> Result<Response, AppError> {
    let code = account_of(code)?;
    let after = FeedbackId::new(query.after);
    let messages = state
        .database()
        .run(move |store| store.conversation_with(&code, after))
        .await?
        .map_err(AppError::internal)?
        .ok_or(AppError::NotFound)?;
    let t = request_lang(&headers).strings();
    chat::messages(&views(&messages, code, t))
}

/// `POST /operator/feedback/<code>/messages`: sends the operator's reply,
/// as `POST /operator/feedback/<code>` does, for the script, which then
/// asks for it with whatever else came.
pub(crate) async fn reply_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    code: Result<Path<String>, PathRejection>,
    OperatorForm { gate, form }: OperatorForm<ReplyForm>,
) -> Result<Response, AppError> {
    let code = account_of(code)?;
    let t = request_lang(&headers).strings();
    let body = match MessageBody::parse(&form.body) {
        Ok(body) => body,
        Err(problem) => {
            return chat::refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                &message_problem(t, problem, t.operator.reply_blank),
            );
        }
    };
    let now = state.now()?;
    let replied = state
        .database()
        .run(move |store| store.as_operator(&gate.digest, now, |pages| pages.reply(&code, &body)))
        .await?;
    match replied {
        Ok(AsOperator::Done(())) => Ok(chat::sent()),
        Ok(AsOperator::NotAllowed(not)) => Ok(turned_away(not, Section::Feedback)),
        Err(ReplyError::UnknownAccount(_)) => Err(AppError::NotFound),
        Err(ReplyError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// `messages` of the account with `code`, as the operator reads them.
fn views(messages: &[FeedbackMessage], code: AccountCode, t: &Strings) -> Vec<MessageView> {
    MessageView::all(
        messages,
        Reader::Operator,
        &code.to_string(),
        t.feedback.operator,
    )
}

/// The account an address names; one that names none does not exist.
fn account_of(code: Result<Path<String>, PathRejection>) -> Result<AccountCode, AppError> {
    code.ok()
        .and_then(|Path(text)| AccountCode::parse(&text).ok())
        .ok_or(AppError::NotFound)
}

/// What the conversation page shows besides the conversation.
struct Shown {
    notice: Option<&'static str>,
    error: Option<String>,
    draft: String,
    status: StatusCode,
}

/// The conversation of the account with `code`, as `shown` says.
async fn show(
    state: &AppState,
    ctx: &PageContext,
    code: AccountCode,
    shown: Shown,
) -> Result<Response, AppError> {
    let messages = state
        .database()
        .run(move |store| store.conversation_with(&code, FeedbackId::START))
        .await?
        .map_err(AppError::internal)?
        .ok_or(AppError::NotFound)?;
    let t = ctx.t();
    let account = code.to_string();
    render_status(
        shown.status,
        &ConversationPage {
            section: Section::Feedback,
            title: fill(t.operator.conversation_title, &account),
            notice: shown.notice,
            error: shown.error,
            messages: views(&messages, code, t),
            draft: shown.draft,
            account,
            chrome: chrome(ctx).with_live_thread(),
        },
    )
}
