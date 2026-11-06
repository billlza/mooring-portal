//! Getting the CFM profile: the download, the subscription link's
//! management, and CFM's fetch of the link.
//!
//! The download and the link's management act on the signed-in account
//! alone: no address or form names an account. A link's token is shown
//! once, on the page that makes it; the store keeps only its digest. The
//! fetch answers an unknown, malformed or revoked token and a token whose
//! account has no profile alike, with 404, and nothing about a fetch is
//! logged, so the token never reaches a log.

use std::net::SocketAddr;

use askama::Template;
use axum::extract::rejection::PathRejection;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use portal_core::auth::SecretToken;
use portal_core::profile::CfmProfile;
use portal_store::{LinkError, LinkedProfile};

use super::auth::NoFields;
use super::{Chrome, Page, PageContext, login_first, render, see_other};
use crate::error::AppError;
use crate::security::client_ip::client_network;
use crate::security::csrf::VerifiedForm;
use crate::security::limits::Throttle;
use crate::state::AppState;

/// Where CFM fetches a link: `/sub/<token>`.
pub(crate) const LINK_PREFIX: &str = "/sub/";
/// The name a downloaded or fetched profile is saved under.
const PROFILE_FILE: &str = "attachment; filename=\"moorlight-cfm.json\"";

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
    ctx: PageContext,
    VerifiedForm(_): VerifiedForm<NoFields>,
) -> Result<Response, AppError> {
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let id = account.id;
    let now = state.now()?;
    let token = SecretToken::generate().map_err(AppError::internal)?;
    let digest = token.digest();
    let made = state
        .database()
        .run(move |store| store.replace_subscription_link(id, &digest, now))
        .await?;
    match made {
        Ok(()) => {}
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
        chrome: ctx.chrome(Page::Subscription),
    })
}

/// `POST /subscription/link/revoke`: turns the account's link off.
pub(crate) async fn revoke_link(
    State(state): State<AppState>,
    ctx: PageContext,
    VerifiedForm(_): VerifiedForm<NoFields>,
) -> Result<Response, AppError> {
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let id = account.id;
    state
        .database()
        .run(move |store| store.revoke_subscription_link(id))
        .await?
        .map_err(AppError::internal)?;
    Ok(see_other("/subscription?notice=link_revoked"))
}

/// `GET /sub/<token>`: what CFM fetches. Each fetch counts against the
/// client's network before the token is looked at.
pub(crate) async fn fetch(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    token: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let now = state.now()?;
    let network = client_network(state.config().client_ip, peer, &headers)?;
    if let Throttle::Wait { minutes } = state.limits().link_fetch(network, now)? {
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
    match linked {
        LinkedProfile::Profile(profile) => Ok(profile_response(&profile)),
        LinkedProfile::Unknown | LinkedProfile::NoProfile => Err(AppError::NotFound),
    }
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
