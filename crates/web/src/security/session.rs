//! Login sessions as browsers hold them.
//!
//! The browser holds a 256-bit random token in `__Host-session`; the
//! database holds only its digest. A missing, malformed, unknown or
//! expired token means the request is anonymous.

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::request::Parts;
use portal_core::auth::SecretToken;
use portal_store::{Account, AccountId, NewSession};

use super::cookies::{self, SameSite};
use crate::config::SESSION_TTL_SECS;
use crate::error::AppError;
use crate::state::AppState;

/// The account signed in on this request, if any.
#[derive(Debug, Clone)]
pub(crate) struct Viewer(pub(crate) Option<Account>);

impl FromRequestParts<AppState> for Viewer {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let Some(token) = presented_token(&parts.headers) else {
            return Ok(Self(None));
        };
        let now = state.now()?;
        let digest = token.digest();
        let account = state
            .database()
            .run(move |store| store.session_account(&digest, now))
            .await?
            .map_err(AppError::internal)?;
        Ok(Self(account))
    }
}

/// The session token the browser presented, if it is well formed.
pub(crate) fn presented_token(headers: &HeaderMap) -> Option<SecretToken> {
    cookies::read(headers, cookies::SESSION).and_then(|value| SecretToken::parse(value).ok())
}

/// A new session for `account`: the record to store and the cookie that
/// hands its token to the browser.
pub(crate) fn new_session(
    state: &AppState,
    account: AccountId,
) -> Result<(NewSession, HeaderValue), AppError> {
    let token = SecretToken::generate().map_err(AppError::internal)?;
    let now = state.now()?;
    let session = NewSession {
        digest: token.digest(),
        account,
        created_at: now,
        expires_at: now
            .checked_add_secs(SESSION_TTL_SECS)
            .map_err(AppError::internal)?,
    };
    let cookie = cookies::set(
        cookies::SESSION,
        &token.encode(),
        SameSite::Lax,
        Some(SESSION_TTL_SECS),
    )?;
    Ok((session, cookie))
}

/// Starts a session for `account` and returns its cookie.
pub(crate) async fn start(state: &AppState, account: AccountId) -> Result<HeaderValue, AppError> {
    let (session, cookie) = new_session(state, account)?;
    state
        .database()
        .run(move |store| store.start_session(&session))
        .await?
        .map_err(AppError::internal)?;
    Ok(cookie)
}

#[cfg(test)]
mod tests {
    use axum::http::header::COOKIE;

    use super::*;

    #[test]
    fn only_well_formed_tokens_are_presented() {
        let mut headers = HeaderMap::new();
        assert!(presented_token(&headers).is_none());
        headers.insert(COOKIE, HeaderValue::from_static("__Host-session=nope"));
        assert!(presented_token(&headers).is_none());
        let token = SecretToken::generate().unwrap();
        headers.insert(
            COOKIE,
            HeaderValue::try_from(format!("__Host-session={}", token.encode())).unwrap(),
        );
        assert!(presented_token(&headers).unwrap().matches(&token));
    }
}
