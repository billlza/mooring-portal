//! The account page and changing the password.

use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::Response;
use portal_core::account::{NewPassword, PasswordAttempt};
use portal_core::auth::Verification;
use serde::Deserialize;

use super::auth::password_problem;
use super::{Chrome, Page, PageContext, PageQuery, login_first, render, render_status, see_other};
use crate::error::AppError;
use crate::format::{beijing, beijing_date};
use crate::i18n::fill;
use crate::security::csrf::VerifiedForm;
use crate::security::limits::Throttle;
use crate::security::session;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "account.html")]
struct AccountPage {
    chrome: Chrome,
    notice: Option<&'static str>,
    code: String,
    email: String,
    created: String,
    bound_since: Option<String>,
}

#[derive(Template)]
#[template(path = "password.html")]
struct PasswordPage {
    chrome: Chrome,
    error: Option<String>,
}

/// A one-off message after a redirect, named by the redirect itself.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notice {
    Welcome,
    PasswordChanged,
    PasswordChangedLinkRevoked,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountQuery {
    notice: Option<Notice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PasswordForm {
    current: String,
    password: String,
    confirm: String,
}

/// `GET /account`.
pub(crate) async fn account(
    State(state): State<AppState>,
    ctx: PageContext,
    PageQuery(query): PageQuery<AccountQuery>,
) -> Result<Response, AppError> {
    let Some(account) = ctx.account.clone() else {
        return Ok(login_first(Page::Account));
    };
    let t = ctx.t();
    let id = account.id;
    let binding = state
        .database()
        .run(move |store| store.binding(id))
        .await?
        .map_err(AppError::internal)?;
    render(&AccountPage {
        chrome: ctx.chrome(Page::Account),
        notice: query.notice.map(|notice| match notice {
            Notice::Welcome => t.account.welcome,
            Notice::PasswordChanged => t.account.password_changed,
            Notice::PasswordChangedLinkRevoked => t.account.password_changed_link_revoked,
        }),
        code: account.code.to_string(),
        email: account.email.as_str().to_owned(),
        created: beijing(account.created_at),
        bound_since: binding
            .map(|binding| fill(t.account.bound_since, beijing_date(binding.bound_at))),
    })
}

/// `GET /account/password`.
pub(crate) async fn password_page(ctx: PageContext) -> Result<Response, AppError> {
    if ctx.account.is_none() {
        return Ok(login_first(Page::Password));
    }
    render(&PasswordPage {
        chrome: ctx.chrome(Page::Password),
        error: None,
    })
}

/// `POST /account/password`: checks the current password, sets the new
/// one, replaces every session with a fresh one for this browser and turns
/// the subscription link off.
pub(crate) async fn change_password(
    State(state): State<AppState>,
    ctx: PageContext,
    VerifiedForm(form): VerifiedForm<PasswordForm>,
) -> Result<Response, AppError> {
    let Some(account) = ctx.account.clone() else {
        return Ok(login_first(Page::Password));
    };
    let t = ctx.t();
    let refuse = |error: &str, status: StatusCode| {
        render_status(
            status,
            &PasswordPage {
                chrome: ctx.chrome(Page::Password),
                error: Some(error.to_owned()),
            },
        )
    };
    let too_many = |minutes| {
        refuse(
            &fill(t.auth.too_many_attempts, minutes),
            StatusCode::TOO_MANY_REQUESTS,
        )
    };
    let now = state.now()?;
    let password = match NewPassword::parse(form.password.clone(), &account.email) {
        Ok(password) => password,
        Err(error) => return refuse(password_problem(t, error), StatusCode::UNPROCESSABLE_ENTITY),
    };
    if form.password != form.confirm {
        return refuse(t.auth.confirm_mismatch, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let Some(ticket) = state.hashing().ticket()? else {
        return refuse(t.auth.busy, StatusCode::SERVICE_UNAVAILABLE);
    };
    if let Throttle::Wait { minutes } = state.limits().password_attempt(&account.email, now)? {
        return too_many(minutes);
    }
    let id = account.id;
    let credentials = state
        .database()
        .run(move |store| store.credentials_of(id))
        .await?
        .map_err(AppError::internal)?;
    let attempt = PasswordAttempt::new(form.current);
    if ticket
        .verify(attempt, Some(credentials.password_hash))
        .await?
        == Verification::Mismatch
    {
        return refuse(t.account.current_password_wrong, StatusCode::UNAUTHORIZED);
    }
    state.limits().password_succeeded(&account.email)?;
    let password_hash = ticket.hash(password).await?;
    let (replacement, cookie) = session::new_session(&state, id)?;
    let change = state
        .database()
        .run(move |store| store.change_password(&password_hash, &replacement))
        .await?
        .map_err(AppError::internal)?;
    let mut response = see_other(if change.link_revoked {
        "/account?notice=password_changed_link_revoked"
    } else {
        "/account?notice=password_changed"
    });
    response.headers_mut().append(SET_COOKIE, cookie);
    Ok(response)
}
