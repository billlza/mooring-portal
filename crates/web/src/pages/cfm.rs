//! Getting the CFM profile: the download, the subscription link's
//! management, and CFM's fetch of the link.
//!
//! The download and the link's management act on the signed-in account
//! alone: no address or form names an account. Making and removing a link
//! act for the session's account as the store finds it in the writing
//! transaction, since the session may end while the form's body is still
//! arriving. A link's token is shown once, on the page that makes it; the
//! store keeps only its digest.
//!
//! The fetch answers an unknown, malformed or revoked token and a token
//! whose account has no profile alike, with 404, and every failure in plain
//! text: a page's stylesheet and links would send the address, token and
//! all, on as the referrer. Nothing about a fetch is logged, so the token
//! never reaches a log.

use std::net::SocketAddr;

use askama::Template;
use axum::extract::rejection::PathRejection;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use portal_core::auth::SecretToken;
use portal_core::profile::CfmProfile;
use portal_store::{LinkError, LinkedProfile, RevokeError};

use super::auth::NoFields;
use super::{Chrome, Page, PageContext, login_first, render, see_other};
use crate::error::AppError;
use crate::security::client_ip::client_network;
use crate::security::csrf::VerifiedForm;
use crate::security::limits::Throttle;
use crate::security::session::presented_token;
use crate::state::AppState;

/// Where CFM fetches a link: `/sub/<token>`.
pub(crate) const LINK_PREFIX: &str = "/sub/";
/// The name a downloaded or fetched profile is saved under.
const PROFILE_FILE: &str = "attachment; filename=\"moorlight-cfm.json\"";

crate::pages::page_templates!(LinkPage);

#[derive(Template)]
#[template(path = "link.html")]
struct LinkPage {
    chrome: Chrome,
    url: String,
}

/// `GET /subscription/profile.json`: the signed-in account's profile, as a
/// file to import.
pub(crate) async fn download(
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let id = account.id;
    let stored = state
        .database()
        .run(move |store| store.cfm_profile(id))
        .await?
        .map_err(AppError::internal)?;
    stored
        .map(|stored| profile_response(&stored.profile))
        .ok_or(AppError::NotFound)
}

/// `POST /subscription/link`: makes the account's link, replacing any it
/// had, and shows it once.
pub(crate) async fn create_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    ctx: PageContext,
    VerifiedForm(_): VerifiedForm<NoFields>,
) -> Result<Response, AppError> {
    let Some(session) = presented_token(&headers) else {
        return Ok(login_first(Page::Subscription));
    };
    let session = session.digest();
    let now = state.now()?;
    let token = SecretToken::generate().map_err(AppError::internal)?;
    let link = token.digest();
    let made = state
        .database()
        .run(move |store| store.replace_subscription_link(&session, &link, now))
        .await?;
    match made {
        Ok(()) => {}
        Err(LinkError::SignedOut) => return Ok(login_first(Page::Subscription)),
        // A link without a profile would fetch nothing.
        Err(LinkError::Unbound | LinkError::NoProfile) => {
            return Ok(see_other("/subscription?notice=link_unavailable"));
        }
        Err(LinkError::Store(error)) => return Err(AppError::internal(error)),
    }
    render(&LinkPage {
        url: format!(
            "{}{LINK_PREFIX}{}",
            state.config().origin.as_str(),
            token.encode()
        ),
        // Switching the language would leave the page, and the link with it.
        chrome: ctx.chrome(Page::Subscription).without_language_switch(),
    })
}

/// `POST /subscription/link/revoke`: turns the account's link off.
pub(crate) async fn revoke_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    VerifiedForm(_): VerifiedForm<NoFields>,
) -> Result<Response, AppError> {
    let Some(session) = presented_token(&headers) else {
        return Ok(login_first(Page::Subscription));
    };
    let session = session.digest();
    let now = state.now()?;
    let revoked = state
        .database()
        .run(move |store| store.revoke_subscription_link(&session, now))
        .await?;
    match revoked {
        Ok(_) => Ok(see_other("/subscription?notice=link_revoked")),
        Err(RevokeError::SignedOut) => Ok(login_first(Page::Subscription)),
        Err(RevokeError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// `GET /sub/<token>`: what CFM fetches. Each fetch counts against the
/// client's network before the token is looked at, and each fetch of a
/// live link against that link.
pub(crate) async fn fetch(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    token: Result<Path<String>, PathRejection>,
) -> Response {
    serve_link(&state, peer, &headers, token)
        .await
        .unwrap_or_else(AppError::plain)
}

async fn serve_link(
    state: &AppState,
    peer: SocketAddr,
    headers: &HeaderMap,
    token: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let now = state.now()?;
    let network = client_network(state.config().client_ip, peer, headers)?;
    if let Throttle::Wait { minutes } = state.limits().link_fetch_from(network, now)? {
        return Ok(too_many(minutes));
    }
    let Some(token) = token
        .ok()
        .and_then(|Path(text)| SecretToken::parse(&text).ok())
    else {
        return Err(AppError::NotFound);
    };
    let digest = token.digest();
    let linked = state
        .database()
        .run(move |store| store.linked_profile(&digest))
        .await?
        .map_err(AppError::internal)?;
    let LinkedProfile::Profile(profile) = linked else {
        return Err(AppError::NotFound);
    };
    if let Throttle::Wait { minutes } = state.limits().link_fetch_of(digest, now)? {
        return Ok(too_many(minutes));
    }
    Ok(profile_response(&profile))
}

/// `profile` as a JSON file. Uncompressed, as CFM 0.5.1 accepts only the
/// identity encoding; not cached, by the security headers' default.
fn profile_response(profile: &CfmProfile) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (CONTENT_DISPOSITION, HeaderValue::from_static(PROFILE_FILE)),
        ],
        profile.as_str().to_owned(),
    )
        .into_response()
}

/// A fetch refused for `minutes`: CFM shows the status, a person the text.
fn too_many(minutes: u64) -> Response {
    let retry = HeaderValue::from(minutes.saturating_mul(60));
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(RETRY_AFTER, retry)],
        "请求太频繁，请稍后再试。Too many requests; try again later.\n",
    )
        .into_response()
}
