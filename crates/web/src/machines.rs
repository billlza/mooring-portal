//! The two addresses that machines call: Stripe's deliveries, and
//! Mooring's pulls of the entitlements.
//!
//! Neither carries a cookie or gets one, and every failure is plain text:
//! there is no person to show a page to.
//!
//! A delivery is believed only if Stripe signed it with a secret the
//! portal holds, and is read only after that: until then its body is
//! bytes to hash, at most [`MAX_EVENT_BYTES`] of them. Nothing of a
//! delivery is logged but the kind of thing wrong with it. A delivery
//! Stripe did sign that the portal cannot act on (rendered for another
//! release of Stripe's API, or of the other mode, test or live, than the
//! payments the database holds) is refused loudly: Stripe then delivers
//! it again and shows the failures in its Dashboard, so nothing is lost
//! while the operator puts it right.
//!
//! A pull is answered only for the bearer of the token the operator made
//! for Mooring, of which the portal keeps the digest. The token travels
//! in a header, never in the address, so that no proxy's log holds it.

use std::net::SocketAddr;

use axum::extract::rejection::QueryRejection;
use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use portal_core::auth::SecretToken;
use portal_core::payment::feed::{MAX_ENTRIES, Page};
use portal_core::payment::stripe::{Event, EventError, MAX_EVENT_BYTES, SignatureError, verify};
use portal_store::{PullError, Recorded};
use serde::Deserialize;

use crate::error::AppError;
use crate::security::client_ip::client_network;
use crate::security::limits::Throttle;
use crate::state::AppState;

/// The header Stripe signs a delivery in.
const SIGNATURE: &str = "stripe-signature";

/// `POST /stripe/webhook`: a delivery from Stripe. Counted against the
/// client's network before anything of it is read.
pub(crate) async fn deliver(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    receive(&state, peer, request)
        .await
        .unwrap_or_else(AppError::plain)
}

async fn receive(
    state: &AppState,
    peer: SocketAddr,
    request: Request,
) -> Result<Response, AppError> {
    let payments = state.selling()?;
    let now = state.now()?;
    let network = client_network(state.config().client_ip, peer, request.headers())?;
    if let Throttle::Wait { minutes } = state.limits().webhook_from(network, now)? {
        return Ok(too_many(minutes));
    }
    let signature = request
        .headers()
        .get(SIGNATURE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = axum::body::to_bytes(request.into_body(), MAX_EVENT_BYTES)
        .await
        .map_err(|_| AppError::BadRequest("the delivery is too large or unreadable"))?;
    let Some(signature) = signature else {
        return Err(AppError::BadRequest("the delivery is not signed"));
    };
    verify(payments.secrets(), &signature, &body, now).map_err(|error| {
        AppError::BadRequest(match error {
            SignatureError::Header => "the delivery's signature header is malformed",
            SignatureError::Mismatch => "the delivery is not signed with the portal's secret",
            SignatureError::Stale => "the delivery was signed too long ago, or this clock is off",
        })
    })?;
    // From here on the delivery is Stripe's: what is wrong with it is the
    // portal's set-up, and the operator's to put right.
    let event = Event::parse(&body).map_err(|error| {
        let reason = match error {
            // Its size was bounded as it was read.
            EventError::TooLarge | EventError::Malformed(_) => {
                "an event from Stripe does not parse"
            }
            EventError::ApiRelease => {
                "an event from Stripe is rendered for another API release than the portal reads; \
                 create the webhook endpoint with the API version docs/payments.md names"
            }
            EventError::Object(_) => "an event from Stripe has an object the portal cannot read",
        };
        tracing::error!(reason, "refused an event Stripe signed");
        AppError::BadRequest(reason)
    })?;
    let on_sale = payments.on_sale();
    let recorded = state
        .database()
        .run(move |store| store.record_stripe_event(&event, &on_sale, now))
        .await?
        .map_err(AppError::internal)?;
    match recorded {
        Recorded::New | Recorded::Duplicate => Ok(StatusCode::OK.into_response()),
        Recorded::OtherMode => {
            let reason = "an event from Stripe is of the other mode (test or live) than the \
                          payments the database holds; point the webhook endpoint of the \
                          catalogue's mode at this portal";
            tracing::error!(reason, "refused an event Stripe signed");
            Err(AppError::BadRequest(reason))
        }
    }
}

/// What a pull asks: the number of the last entry Mooring has acted on.
#[derive(Debug, Deserialize)]
pub(crate) struct PullQuery {
    after: u64,
}

/// `GET /api/mooring/entitlements?after=<number>`: the entries of the
/// journal after a cursor, for the bearer of Mooring's token. Counted
/// against the client's network before the token is read.
pub(crate) async fn pull(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    query: Result<Query<PullQuery>, QueryRejection>,
) -> Response {
    answer(&state, peer, &headers, query)
        .await
        .unwrap_or_else(AppError::plain)
}

async fn answer(
    state: &AppState,
    peer: SocketAddr,
    headers: &HeaderMap,
    query: Result<Query<PullQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    state.selling()?;
    let now = state.now()?;
    let network = client_network(state.config().client_ip, peer, headers)?;
    if let Throttle::Wait { minutes } = state.limits().pull_from(network, now)? {
        return Ok(too_many(minutes));
    }
    let Some(token) = bearer(headers) else {
        return Ok(unauthorized());
    };
    let token = token.digest();
    let Ok(Query(PullQuery { after })) = query else {
        // Only Mooring is told that its request is malformed.
        let known = state
            .database()
            .run(move |store| store.is_pull_token(&token))
            .await?
            .map_err(AppError::internal)?;
        return if known {
            Err(AppError::BadRequest("a pull must say after which entry"))
        } else {
            Ok(unauthorized())
        };
    };
    let pulled = state
        .database()
        .run(move |store| store.pull_entitlements(&token, after, MAX_ENTRIES, now))
        .await?;
    let pulled = match pulled {
        Ok(pulled) => pulled,
        Err(PullError::Unauthorized) => return Ok(unauthorized()),
        Err(error @ PullError::Ahead { .. }) => {
            // Mooring has seen entries this journal never wrote: the
            // database is not the one it pulled from before.
            tracing::error!(%error, "refused a pull");
            return Ok((
                StatusCode::CONFLICT,
                "the cursor is beyond this journal's last entry\n",
            )
                .into_response());
        }
        // A portal that sells has made its database enter a mode.
        Err(error @ (PullError::NoMode | PullError::Store(_))) => {
            return Err(AppError::internal(error));
        }
    };
    let document = Page {
        generated_at: now,
        // The database's word, not the catalogue's: the entries are of
        // the mode the database was in when they were written.
        livemode: pulled.livemode,
        after,
        entries: pulled.entries,
        more: pulled.more,
    }
    .to_json()
    .map_err(AppError::internal)?;
    Ok((
        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        document,
    )
        .into_response())
}

/// The token a request bears: `Authorization: Bearer <64 hex digits>`.
fn bearer(headers: &HeaderMap) -> Option<SecretToken> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|token| SecretToken::parse(token).ok())
}

/// A pull without the token. Whether a token is set, and how the one
/// presented was wrong, is not said.
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
        "a pull needs Mooring's token\n",
    )
        .into_response()
}

/// A call refused for `minutes`.
fn too_many(minutes: u64) -> Response {
    let retry = HeaderValue::from(minutes.saturating_mul(60));
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(RETRY_AFTER, retry)],
        "too many requests; try again later\n",
    )
        .into_response()
}
