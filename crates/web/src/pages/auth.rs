//! Logging in, registering and logging out.
//!
//! A failed login says only that the email or password is wrong, and takes
//! as long whether or not the email has an account (a decoy hash is
//! verified when it has none). Logins are limited per client network and
//! password checks per email; registrations per client network. A request
//! that would hash while the hashing queue is full is refused as busy.

use std::net::SocketAddr;

use askama::Template;
use axum::extract::{ConnectInfo, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use portal_core::account::{AccountCode, Email, NewPassword, PasswordAttempt, PasswordError};
use portal_core::auth::Verification;
use portal_store::{NewAccount, RegisterError};
use serde::Deserialize;

use super::{Chrome, Page, PageContext, PageQuery, render, render_status, see_other};
use crate::error::AppError;
use crate::i18n::{Strings, fill};
use crate::security::client_ip::client_network;
use crate::security::cookies;
use crate::security::csrf::{VerifiedForm, rotated_cookie};
use crate::security::limits::Throttle;
use crate::security::session::{self, presented_token};
use crate::state::AppState;

crate::pages::page_templates!(LoginPage, RegisterPage);

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage {
    chrome: Chrome,
    email: String,
    error: Option<String>,
    next: Option<Page>,
}

#[derive(Template)]
#[template(path = "register.html")]
struct RegisterPage {
    chrome: Chrome,
    email: String,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginQuery {
    next: Option<Page>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginForm {
    email: String,
    password: String,
    next: Option<Page>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterForm {
    email: String,
    password: String,
    confirm: String,
}

/// A form with nothing but its CSRF token.
#[derive(Debug, Deserialize)]
pub(crate) struct NoFields {}

/// Where a login goes next.
fn target(next: Option<Page>) -> &'static str {
    next.map_or(Page::Account.path(), Page::path)
}

/// What to say about a password that breaks the policy.
pub(crate) fn password_problem(t: &Strings, error: PasswordError) -> &'static str {
    match error {
        PasswordError::TooShort => t.auth.password_too_short,
        PasswordError::TooLong => t.auth.password_too_long,
        PasswordError::ControlCharacter => t.auth.password_control,
        PasswordError::MatchesEmail => t.auth.password_is_email,
    }
}

/// `GET /login`.
pub(crate) async fn login_page(
    ctx: PageContext,
    PageQuery(query): PageQuery<LoginQuery>,
) -> Result<Response, AppError> {
    if ctx.account.is_some() {
        return Ok(see_other(target(query.next)));
    }
    render(&LoginPage {
        chrome: ctx.chrome(Page::Login),
        email: String::new(),
        error: None,
        next: query.next,
    })
}

/// `POST /login`.
pub(crate) async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ctx: PageContext,
    VerifiedForm(form): VerifiedForm<LoginForm>,
) -> Result<Response, AppError> {
    let LoginForm {
        email: typed,
        password,
        next,
    } = form;
    let t = ctx.t();
    let refuse = |error: String, status: StatusCode| {
        render_status(
            status,
            &LoginPage {
                chrome: ctx.chrome(Page::Login),
                email: typed.clone(),
                error: Some(error),
                next,
            },
        )
    };
    let too_many = |minutes| {
        refuse(
            fill(t.auth.too_many_attempts, minutes),
            StatusCode::TOO_MANY_REQUESTS,
        )
    };
    let now = state.now()?;
    let network = client_network(state.config().client_ip, peer, &headers)?;
    if let Throttle::Wait { minutes } = state.limits().login_attempt(network, now)? {
        return too_many(minutes);
    }
    // Before anything is counted against the email, so that a busy portal
    // costs the account nothing.
    let Some(ticket) = state.hashing().ticket()? else {
        return refuse(t.auth.busy.to_owned(), StatusCode::SERVICE_UNAVAILABLE);
    };
    let attempt = PasswordAttempt::new(password);
    let Ok(email) = Email::parse(&typed) else {
        ticket.verify(attempt, None).await?;
        return refuse(t.auth.login_failed.to_owned(), StatusCode::UNAUTHORIZED);
    };
    if let Throttle::Wait { minutes } = state.limits().password_attempt(&email, now)? {
        return too_many(minutes);
    }
    let lookup = email.clone();
    let credentials = state
        .database()
        .run(move |store| store.credentials(&lookup))
        .await?
        .map_err(AppError::internal)?;
    let stored = credentials
        .as_ref()
        .map(|found| found.password_hash.clone());
    match (credentials, ticket.verify(attempt, stored).await?) {
        (Some(found), Verification::Match) => {
            state.limits().password_succeeded(&email)?;
            let session_cookie = session::start(&state, found.account).await?;
            let mut response = see_other(target(next));
            response.headers_mut().append(SET_COOKIE, session_cookie);
            response.headers_mut().append(SET_COOKIE, rotated_cookie()?);
            Ok(response)
        }
        _ => refuse(t.auth.login_failed.to_owned(), StatusCode::UNAUTHORIZED),
    }
}

/// `GET /register`.
pub(crate) async fn register_page(ctx: PageContext) -> Result<Response, AppError> {
    if ctx.account.is_some() {
        return Ok(see_other(Page::Account.path()));
    }
    render(&RegisterPage {
        chrome: ctx.chrome(Page::Register),
        email: String::new(),
        error: None,
    })
}

/// `POST /register`: creates the account and logs the browser in.
pub(crate) async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ctx: PageContext,
    VerifiedForm(form): VerifiedForm<RegisterForm>,
) -> Result<Response, AppError> {
    let t = ctx.t();
    let refuse = |error: &str, status: StatusCode| {
        render_status(
            status,
            &RegisterPage {
                chrome: ctx.chrome(Page::Register),
                email: form.email.clone(),
                error: Some(error.to_owned()),
            },
        )
    };
    let now = state.now()?;
    let network = client_network(state.config().client_ip, peer, &headers)?;
    if let Throttle::Wait { minutes } = state.limits().registration_attempt(network, now)? {
        return refuse(
            &fill(t.auth.too_many_attempts, minutes),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    let Ok(email) = Email::parse(&form.email) else {
        return refuse(t.auth.invalid_email, StatusCode::UNPROCESSABLE_ENTITY);
    };
    let password = match NewPassword::parse(form.password.clone(), &email) {
        Ok(password) => password,
        Err(error) => return refuse(password_problem(t, error), StatusCode::UNPROCESSABLE_ENTITY),
    };
    if form.password != form.confirm {
        return refuse(t.auth.confirm_mismatch, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let Some(ticket) = state.hashing().ticket()? else {
        return refuse(t.auth.busy, StatusCode::SERVICE_UNAVAILABLE);
    };
    // Hash before looking the email up, so a taken or banned email answers
    // as slowly as a new one.
    let password_hash = ticket.hash(password).await?;
    let email = ticket.digest_email(email).await?;
    let account = NewAccount {
        code: AccountCode::generate().map_err(AppError::internal)?,
        email,
        password_hash,
        created_at: now,
    };
    let limit = state.config().max_accounts;
    match state
        .database()
        .run(move |store| store.register(account, limit))
        .await?
    {
        Ok(account) => {
            // Which account it was stays out: the log outlives a deletion.
            tracing::info!("registered an account");
            let session_cookie = session::start(&state, account.id).await?;
            let mut response = see_other("/account?notice=welcome");
            response.headers_mut().append(SET_COOKIE, session_cookie);
            response.headers_mut().append(SET_COOKIE, rotated_cookie()?);
            Ok(response)
        }
        // A banned address is answered as a taken one: telling them apart
        // would tell anyone who types an address that its owner was banned.
        Err(RegisterError::EmailTaken | RegisterError::Banned) => {
            refuse(t.auth.email_taken, StatusCode::CONFLICT)
        }
        Err(RegisterError::Full { .. }) => refuse(t.auth.registration_full, StatusCode::FORBIDDEN),
        Err(RegisterError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// `POST /logout`: ends this browser's session.
pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    VerifiedForm(_): VerifiedForm<NoFields>,
) -> Result<Response, AppError> {
    if let Some(token) = presented_token(&headers) {
        let digest = token.digest();
        state
            .database()
            .run(move |store| store.end_session(&digest))
            .await?
            .map_err(AppError::internal)?;
    }
    let mut response = see_other(Page::Home.path());
    response
        .headers_mut()
        .append(SET_COOKIE, cookies::clear(cookies::SESSION)?);
    response.headers_mut().append(SET_COOKIE, rotated_cookie()?);
    Ok(response)
}
