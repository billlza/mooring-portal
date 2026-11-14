//! The operator pages: what the operator may do from the site rather than
//! on the host (`docs/security.md`, section 13).
//!
//! Only the operator's command on the host makes an account an operator.
//! Every operator page asks for an operator's session that confirmed its
//! account's password within [`OPERATOR_CONFIRM_SECS`]: a signed-out
//! visitor is sent to log in, a signed-in account that is no operator is
//! told the page does not exist, and an operator whose confirmation is
//! missing or too old is asked for the password first. Confirming gives
//! the browser a new session. The pages answer feedback, bind, unbind and
//! ban accounts, close Mac bookings, and approve orders that wait for the
//! operator (`docs/payments.md`). A form's body can arrive long
//! after its headers, so a form is read only for a session that may still
//! act once its body has arrived ([`OperatorForm`]), and each write checks
//! the session once more in its own transaction ([`Store::as_operator`]):
//! the check made when a request's headers arrive is not the one that
//! lets its write through, nor the one that lets a page answer the form.
//! The pages show no CFM profile, subscription link or credential;
//! importing snapshots, bundles and releases, the pull token and the rest
//! stay with the commands on the host.

mod accounts;
mod bookings;
mod conversations;
mod payments;

use askama::Template;
use axum::extract::{FromRequest, FromRequestParts, Request, State};
use axum::http::header::SET_COOKIE;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use portal_core::account::PasswordAttempt;
use portal_core::auth::{TokenDigest, Verification};
use portal_core::time::UnixTime;
use portal_store::{NotAllowed, OPERATOR_CONFIRM_SECS, OperatorSession, Store, StoreError};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{Chrome, Page, PageContext, PageQuery, login_first, render, render_status, see_other};
use crate::error::{AppError, chain};
use crate::i18n::{Strings, fill};
use crate::security::csrf::{VerifiedForm, rotated_cookie};
use crate::security::limits::Throttle;
use crate::security::session::{self, presented_token};
use crate::state::AppState;
use crate::view::ReleaseView;

pub(crate) use accounts::{accounts, ban, bind, unbind};
pub(crate) use bookings::{bookings, close_booking};
pub(crate) use conversations::{conversation, conversations, reply};
pub(crate) use payments::{approve, payments};

/// A section of the operator pages, as their tabs and the confirmation
/// name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Section {
    Overview,
    Feedback,
    Accounts,
    Bookings,
    Payments,
}

impl Section {
    /// The section's address.
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Overview => "/operator",
            Self::Feedback => "/operator/feedback",
            Self::Accounts => "/operator/accounts",
            Self::Bookings => "/operator/bookings",
            Self::Payments => "/operator/payments",
        }
    }

    /// The section's name in forms.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Feedback => "feedback",
            Self::Accounts => "accounts",
            Self::Bookings => "bookings",
            Self::Payments => "payments",
        }
    }

    /// The section an address of the operator pages belongs to.
    fn of(path: &str) -> Self {
        match path
            .strip_prefix("/operator/")
            .and_then(|rest| rest.split('/').next())
        {
            Some("feedback") => Self::Feedback,
            Some("accounts") => Self::Accounts,
            Some("bookings") => Self::Bookings,
            Some("payments") => Self::Payments,
            _ => Self::Overview,
        }
    }

    /// `aria-current` for the tab `key`.
    pub(crate) fn current(self, key: &str) -> &'static str {
        if self.key() == key { "page" } else { "false" }
    }
}

/// Who asks for an operator page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visitor {
    /// No live session.
    SignedOut,
    /// A signed-in account that is no operator.
    NotOperator,
    /// An operator.
    Operator(OperatorSession),
}

/// Who presents the session in `headers` at `now`.
async fn visitor(
    state: &AppState,
    headers: &HeaderMap,
    now: UnixTime,
) -> Result<Visitor, AppError> {
    let Some(token) = presented_token(headers) else {
        return Ok(Visitor::SignedOut);
    };
    let digest = token.digest();
    state
        .database()
        .run(move |store| -> Result<_, StoreError> {
            if let Some(session) = store.operator_session(&digest, now)? {
                return Ok(Visitor::Operator(session));
            }
            Ok(match store.session_account(&digest, now)? {
                Some(_) => Visitor::NotOperator,
                None => Visitor::SignedOut,
            })
        })
        .await?
        .map_err(AppError::internal)
}

/// What a request of `section` gets from a session that may not act on
/// the operator pages: the login page, a page that does not exist, or
/// the confirmation.
fn turned_away(not: NotAllowed, section: Section) -> Response {
    match not {
        NotAllowed::SignedOut => login_first(Page::Operator),
        NotAllowed::NotOperator => AppError::NotFound.into_response(),
        NotAllowed::Unconfirmed => see_other(&format!("/operator/confirm?next={}", section.key())),
    }
}

/// Whether the session `headers` present may act on the operator pages at
/// the time of asking: the digest of its token if it may, or what it
/// lacks.
async fn may_act(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Result<TokenDigest, NotAllowed>, AppError> {
    Ok(match visitor(state, headers, state.now()?).await? {
        // The visitor's session came from a token the headers hold.
        Visitor::Operator(session) if session.confirmed => presented_token(headers)
            .map(|token| token.digest())
            .ok_or(NotAllowed::SignedOut),
        Visitor::Operator(_) => Err(NotAllowed::Unconfirmed),
        Visitor::NotOperator => Err(NotAllowed::NotOperator),
        Visitor::SignedOut => Err(NotAllowed::SignedOut),
    })
}

/// An operator whose session confirmed its password lately when the
/// request's headers arrived: what every operator page but the
/// confirmation asks for. Anyone else is [`turned_away`]. A form asks for
/// more ([`OperatorForm`]), and a write checks the session again as it
/// runs, by its token's digest.
#[derive(Debug)]
pub(crate) struct Confirmed {
    /// The digest of the session's token.
    digest: TokenDigest,
}

impl FromRequestParts<AppState> for Confirmed {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        match may_act(state, &parts.headers)
            .await
            .map_err(IntoResponse::into_response)?
        {
            Ok(digest) => Ok(Self { digest }),
            Err(not) => Err(turned_away(not, Section::of(parts.uri.path()))),
        }
    }
}

/// A form of the operator pages, read for a session that could act on
/// them when the form's headers arrived ([`Confirmed`]) and still can once
/// its body has: what every operator form asks for. The body may come long
/// after the headers, and the page that answers a form, even one that
/// refuses it, shows what only an operator may see. A write checks the
/// session once more as it runs ([`Store::as_operator`]).
#[derive(Debug)]
pub(crate) struct OperatorForm<T> {
    /// The session, as it was when the body had arrived.
    gate: Confirmed,
    /// The form, which passed the CSRF checks.
    form: T,
}

impl<T: DeserializeOwned + Send> FromRequest<AppState> for OperatorForm<T> {
    type Rejection = Response;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Response> {
        let (mut parts, body) = request.into_parts();
        // A session that may not act already is turned away unread.
        Confirmed::from_request_parts(&mut parts, state).await?;
        let headers = parts.headers.clone();
        let path = parts.uri.path().to_owned();
        let VerifiedForm(form) =
            VerifiedForm::from_request(Request::from_parts(parts, body), state)
                .await
                .map_err(IntoResponse::into_response)?;
        match may_act(state, &headers)
            .await
            .map_err(IntoResponse::into_response)?
        {
            Ok(digest) => Ok(Self {
                gate: Confirmed { digest },
                form,
            }),
            Err(not) => Err(turned_away(not, Section::of(&path))),
        }
    }
}

/// The surroundings of an operator page.
fn chrome(ctx: &PageContext) -> Chrome {
    ctx.chrome(Page::Operator)
}

/// What an action did, as the page reports it.
#[derive(Debug, Default)]
struct Outcome {
    /// What was done.
    notice: Option<String>,
    /// Why nothing was done.
    refusal: Option<String>,
    /// What the operator must still do, or know.
    warnings: Vec<String>,
}

impl Outcome {
    /// `notice` was done.
    fn done(notice: String) -> Self {
        Self {
            notice: Some(notice),
            ..Self::default()
        }
    }

    /// Nothing was done, for `reason`.
    fn refused(t: &Strings, reason: &str) -> Self {
        Self {
            refusal: Some(fill(t.operator.refused, reason)),
            ..Self::default()
        }
    }

    /// Adds what the operator must know of the erasure that followed a
    /// deletion: nothing, if it erased all; otherwise that the copies may
    /// remain, and how to erase them.
    fn erased(mut self, t: &Strings, erased: Result<(), StoreError>) -> Self {
        match erased {
            Ok(()) => {}
            Err(StoreError::EraseBusy) => self.warnings.push(t.operator.erase_busy.to_owned()),
            Err(error) => {
                tracing::error!(error = %chain(&error), "erasing the deleted rows failed");
                self.warnings.push(t.operator.erase_failed.to_owned());
            }
        }
        self
    }
}

#[derive(Template)]
#[template(path = "operator_confirm.html")]
struct ConfirmPage {
    chrome: Chrome,
    lead: String,
    next: Section,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConfirmQuery {
    next: Option<Section>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConfirmForm {
    password: String,
    next: Section,
}

/// The confirmation page for `next`, with `error`.
fn confirm_page(
    ctx: &PageContext,
    next: Section,
    error: Option<String>,
    status: StatusCode,
) -> Result<Response, AppError> {
    render_status(
        status,
        &ConfirmPage {
            lead: fill(ctx.t().operator.confirm_lead, OPERATOR_CONFIRM_SECS / 60),
            next,
            error,
            chrome: chrome(ctx),
        },
    )
}

/// `GET /operator/confirm`: asks an operator for the password.
pub(crate) async fn confirm_form(
    State(state): State<AppState>,
    ctx: PageContext,
    headers: HeaderMap,
    PageQuery(query): PageQuery<ConfirmQuery>,
) -> Result<Response, AppError> {
    match visitor(&state, &headers, state.now()?).await? {
        Visitor::Operator(_) => confirm_page(
            &ctx,
            query.next.unwrap_or(Section::Overview),
            None,
            StatusCode::OK,
        ),
        Visitor::NotOperator => Err(AppError::NotFound),
        Visitor::SignedOut => Ok(login_first(Page::Operator)),
    }
}

/// `POST /operator/confirm`: checks the password as a login would, and
/// records that the session confirmed it.
pub(crate) async fn confirm(
    State(state): State<AppState>,
    ctx: PageContext,
    headers: HeaderMap,
    VerifiedForm(form): VerifiedForm<ConfirmForm>,
) -> Result<Response, AppError> {
    let now = state.now()?;
    let session = match visitor(&state, &headers, now).await? {
        Visitor::Operator(session) => session,
        Visitor::NotOperator => return Err(AppError::NotFound),
        Visitor::SignedOut => return Ok(login_first(Page::Operator)),
    };
    let Some(account) = ctx
        .account
        .as_ref()
        .filter(|account| account.id == session.account)
    else {
        return Ok(login_first(Page::Operator));
    };
    let t = ctx.t();
    let Some(ticket) = state.hashing().ticket()? else {
        return confirm_page(
            &ctx,
            form.next,
            Some(t.auth.busy.to_owned()),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    if let Throttle::Wait { minutes } = state.limits().password_attempt(&account.email, now)? {
        return confirm_page(
            &ctx,
            form.next,
            Some(fill(t.auth.too_many_attempts, minutes)),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    let id = session.account;
    let credentials = state
        .database()
        .run(move |store| store.credentials_of(id))
        .await?
        .map_err(AppError::internal)?;
    if ticket
        .verify(
            PasswordAttempt::new(form.password),
            Some(credentials.password_hash),
        )
        .await?
        == Verification::Mismatch
    {
        return confirm_page(
            &ctx,
            form.next,
            Some(t.operator.confirm_wrong.to_owned()),
            StatusCode::UNAUTHORIZED,
        );
    }
    state.limits().password_succeeded(&account.email)?;
    let Some(token) = presented_token(&headers) else {
        return Ok(login_first(Page::Operator));
    };
    let digest = token.digest();
    // The browser that confirmed gets a new session, which alone is
    // confirmed: a copy of the old token, taken before, opens nothing.
    // Both cookies are made before the old session is deleted, so nothing
    // can fail between that and the browser receiving its new one.
    let (replacement, cookie) = session::new_session(&state, id)?;
    let csrf = rotated_cookie()?;
    let confirmed = state
        .database()
        .run(move |store| store.confirm_session(&digest, &replacement))
        .await?
        .map_err(AppError::internal)?;
    if !confirmed {
        return Ok(login_first(Page::Operator));
    }
    let mut response = see_other(form.next.path());
    response.headers_mut().append(SET_COOKIE, cookie);
    response.headers_mut().append(SET_COOKIE, csrf);
    Ok(response)
}

#[derive(Template)]
#[template(path = "operator.html")]
struct OverviewPage {
    chrome: Chrome,
    section: Section,
    awaiting: u64,
    payments: u64,
    accounts: usize,
    unbound: usize,
    bookings: usize,
    /// The release on the download page, and whether the host has
    /// checked it within the day: nothing tells the operator otherwise
    /// when the host's imports keep failing.
    release: ReleaseTile,
}

/// The overview's tile for the release on the download page.
enum ReleaseTile {
    /// As the download page shows it.
    Read(ReleaseView),
    /// The stored release does not read. The download page and the
    /// imports fail on it (`docs/releases.md`, section 4); the overview
    /// says so instead of failing too, so that the operator still sees
    /// what else waits.
    Unreadable,
}

/// `GET /operator`: what waits for the operator.
pub(crate) async fn overview(
    _: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let now = state.now()?;
    let (accounts, unbound, bookings, release) = state
        .database()
        .run(|store: &mut Store| -> Result<_, StoreError> {
            let accounts = store.accounts()?;
            let unbound = accounts
                .iter()
                .filter(|account| account.binding.is_none())
                .count();
            let release = match store.offered_release() {
                Ok(offered) => Ok(offered),
                // A stored release that does not read is said on its
                // tile; any other failure of the database fails the page,
                // as it fails every page.
                Err(error @ StoreError::Corrupt { .. }) => Err(error),
                Err(error) => return Err(error),
            };
            Ok((accounts.len(), unbound, store.bookings()?.len(), release))
        })
        .await?
        .map_err(AppError::internal)?;
    let release = match release {
        Ok(offered) => ReleaseTile::Read(ReleaseView::of(offered, now, ctx.t())),
        Err(error) => {
            tracing::error!(error = %chain(&error), "the release on offer does not read");
            ReleaseTile::Unreadable
        }
    };
    render(&OverviewPage {
        section: Section::Overview,
        awaiting: ctx.signals.awaiting_reply,
        payments: ctx.signals.payments_waiting,
        accounts,
        unbound,
        bookings,
        release,
        chrome: chrome(&ctx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_address_belongs_to_its_section() {
        for (path, section) in [
            ("/operator", Section::Overview),
            ("/operator/", Section::Overview),
            ("/operator/confirm", Section::Overview),
            ("/operator/feedback", Section::Feedback),
            ("/operator/feedback/abcd-efgh-2345", Section::Feedback),
            ("/operator/accounts/ban", Section::Accounts),
            ("/operator/bookings/close", Section::Bookings),
            ("/operator/payments", Section::Payments),
            ("/operator/payments/approve", Section::Payments),
        ] {
            assert_eq!(Section::of(path), section, "{path}");
            assert!(section.path().starts_with("/operator"));
            assert_eq!(section.current(section.key()), "page");
        }
        assert_eq!(Section::Feedback.current("accounts"), "false");
    }

    #[test]
    fn a_session_that_may_not_act_is_turned_away_by_what_it_lacks() {
        let away = |not| turned_away(not, Section::Bookings);
        let signed_out = away(NotAllowed::SignedOut);
        assert_eq!(signed_out.status(), StatusCode::SEE_OTHER);
        assert_eq!(signed_out.headers()["location"], "/login?next=operator");
        assert_eq!(
            away(NotAllowed::NotOperator).status(),
            StatusCode::NOT_FOUND
        );
        let unconfirmed = away(NotAllowed::Unconfirmed);
        assert_eq!(unconfirmed.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            unconfirmed.headers()["location"],
            "/operator/confirm?next=bookings"
        );
    }

    #[test]
    fn an_erasure_is_reported_as_it_went() {
        let t = crate::i18n::Lang::Zh.strings();
        let done = || Outcome::done("done".to_owned());
        assert!(done().erased(t, Ok(())).warnings.is_empty());
        assert_eq!(
            done().erased(t, Err(StoreError::EraseBusy)).warnings,
            [t.operator.erase_busy]
        );
        let failed = done().erased(t, Err(StoreError::Missing { what: "account" }));
        assert_eq!(failed.warnings, [t.operator.erase_failed]);
        assert_eq!(failed.notice.as_deref(), Some("done"));
    }
}
