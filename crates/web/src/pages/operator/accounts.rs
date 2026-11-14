//! Every account, and binding, unbinding and banning one.
//!
//! An action's outcome is shown on the page its form returns rather than
//! after a redirect: what the operator must still do elsewhere, such as
//! cancelling a subscription at Stripe or revoking a user in Mooring, is
//! said once, there, from what the action's own transaction read.
//! Unbinding and banning delete rows, so they end by erasing them from the
//! database files, as the commands on the host do. An operator's account
//! is not banned here: only the host takes away what only the host gives.

use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use portal_core::account::AccountCode;
use portal_core::snapshot::MooringUserId;
use portal_store::{AsOperator, BanError, BindError, UnbindError};
use serde::Deserialize;

use super::{Confirmed, Outcome, Section, chrome, turned_away};
use crate::error::AppError;
use crate::format::beijing;
use crate::i18n::{Strings, fill};
use crate::pages::{Chrome, PageContext, render_status};
use crate::security::csrf::VerifiedForm;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "operator_accounts.html")]
struct AccountsPage {
    chrome: Chrome,
    section: Section,
    outcome: Outcome,
    accounts: Vec<AccountRow>,
}

/// An account, as the list shows it.
struct AccountRow {
    code: String,
    email: String,
    created: String,
    /// Its Mooring user, if it is bound.
    user: Option<String>,
    operator: bool,
}

/// The binding form: the account, and the Mooring user the operator
/// typed.
#[derive(Debug, Deserialize)]
pub(crate) struct BindForm {
    account: String,
    user: String,
}

/// A form that names an account the page lists.
#[derive(Debug, Deserialize)]
pub(crate) struct AccountForm {
    account: String,
}

/// `GET /operator/accounts`: every account, oldest first.
pub(crate) async fn accounts(
    _: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    page(&state, &ctx, Outcome::default(), StatusCode::OK).await
}

/// `POST /operator/accounts/bind`: binds the account to the Mooring user
/// the operator typed.
pub(crate) async fn bind(
    gate: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
    VerifiedForm(form): VerifiedForm<BindForm>,
) -> Result<Response, AppError> {
    let account = listed(&form.account)?;
    let t = ctx.t();
    let Ok(user) = MooringUserId::parse(form.user.trim()) else {
        let refused = Outcome::refused(t, t.operator.bad_user);
        return page(&state, &ctx, refused, StatusCode::UNPROCESSABLE_ENTITY).await;
    };
    let now = state.now()?;
    let bound = state
        .database()
        .run(move |store| store.as_operator(&gate.digest, now, |pages| pages.bind(&account, &user)))
        .await?;
    let (outcome, status) = match bound {
        Ok(AsOperator::NotAllowed(not)) => return Ok(turned_away(not, Section::Accounts)),
        Ok(AsOperator::Done(())) => {
            tracing::info!("the operator bound an account on the operator pages");
            (
                Outcome::done(fill(t.operator.bound_notice, account)),
                StatusCode::OK,
            )
        }
        Err(BindError::UnknownAccount(_)) => unknown(t, account),
        Err(BindError::AccountBound { account, user }) => (
            Outcome::refused(t, &fill(&fill(t.operator.account_bound, account), user)),
            StatusCode::CONFLICT,
        ),
        Err(BindError::UserBound { user, account }) => (
            Outcome::refused(t, &fill(&fill(t.operator.user_bound, user), account)),
            StatusCode::CONFLICT,
        ),
        Err(BindError::Store(error)) => return Err(AppError::internal(error)),
    };
    page(&state, &ctx, outcome, status).await
}

/// `POST /operator/accounts/unbind`: unbinds the account, which deletes
/// its subscription record, CFM profile and subscription link.
pub(crate) async fn unbind(
    gate: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
    VerifiedForm(form): VerifiedForm<AccountForm>,
) -> Result<Response, AppError> {
    let account = listed(&form.account)?;
    let t = ctx.t();
    let now = state.now()?;
    let unbound = state
        .database()
        .run(move |store| {
            store
                .as_operator(&gate.digest, now, |pages| pages.unbind(&account))
                .map(|unbound| unbound.map(|unbound| (unbound, store.erase_deleted())))
        })
        .await?;
    let (outcome, status) = match unbound {
        Ok(AsOperator::NotAllowed(not)) => return Ok(turned_away(not, Section::Accounts)),
        Ok(AsOperator::Done((unbound, erased))) => {
            tracing::info!("the operator unbound an account on the operator pages");
            // Its orders stay, and with them its subscriptions at Stripe.
            let mut outcome = Outcome::done(fill(
                &fill(t.operator.unbound_notice, account),
                &unbound.user,
            ));
            outcome.warnings.extend(
                unbound
                    .subscriptions
                    .iter()
                    .map(|subscription| fill(t.operator.unbind_stripe, subscription)),
            );
            (outcome.erased(t, erased), StatusCode::OK)
        }
        Err(UnbindError::UnknownAccount(_)) => unknown(t, account),
        Err(UnbindError::NotBound(_)) => (
            Outcome::refused(t, &fill(t.operator.not_bound, account)),
            StatusCode::CONFLICT,
        ),
        Err(UnbindError::Store(error)) => return Err(AppError::internal(error)),
    };
    page(&state, &ctx, outcome, status).await
}

/// `POST /operator/accounts/ban`: bans the account, as `ban` on the host
/// does without cards: deletes it with everything of it, and keeps its
/// email address from registering again.
pub(crate) async fn ban(
    gate: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
    VerifiedForm(form): VerifiedForm<AccountForm>,
) -> Result<Response, AppError> {
    let account = listed(&form.account)?;
    let t = ctx.t();
    let now = state.now()?;
    let banned = state
        .database()
        .run(move |store| {
            store
                .as_operator(&gate.digest, now, |pages| pages.ban(&account))
                .map(|banned| banned.map(|ban| ban.map(|ban| (ban, store.erase_deleted()))))
        })
        .await?;
    let (outcome, status) = match banned {
        Ok(AsOperator::NotAllowed(not)) => return Ok(turned_away(not, Section::Accounts)),
        Ok(AsOperator::Done(Some((ban, erased)))) => {
            tracing::info!("the operator banned an account on the operator pages");
            let mut outcome = Outcome::done(fill(t.operator.banned_notice, account));
            outcome.warnings.extend(
                ban.subscriptions
                    .iter()
                    .map(|subscription| fill(t.operator.ban_stripe, subscription)),
            );
            if let Some(user) = &ban.user {
                outcome
                    .warnings
                    .push(fill(t.operator.revoke_in_mooring, user));
            }
            (outcome.erased(t, erased), StatusCode::OK)
        }
        Ok(AsOperator::Done(None)) => (
            Outcome::refused(t, t.operator.ban_operator),
            StatusCode::CONFLICT,
        ),
        Err(BanError::Unknown(_)) => unknown(t, account),
        Err(BanError::Store(error)) => return Err(AppError::internal(error)),
        Err(BanError::Hash(error)) => return Err(AppError::internal(error)),
    };
    page(&state, &ctx, outcome, status).await
}

/// The account a form names. The page writes it, so one that is no
/// account's code is not the page's form.
fn listed(code: &str) -> Result<AccountCode, AppError> {
    AccountCode::parse(code).map_err(|_| AppError::BadRequest("the account is not a code"))
}

/// The refusal for `account`, which no account has any more.
fn unknown(t: &Strings, account: AccountCode) -> (Outcome, StatusCode) {
    (
        Outcome::refused(t, &fill(t.operator.unknown_account, account)),
        StatusCode::CONFLICT,
    )
}

/// The accounts page, after `outcome`.
async fn page(
    state: &AppState,
    ctx: &PageContext,
    outcome: Outcome,
    status: StatusCode,
) -> Result<Response, AppError> {
    let accounts = state
        .database()
        .run(|store| store.accounts())
        .await?
        .map_err(AppError::internal)?;
    render_status(
        status,
        &AccountsPage {
            section: Section::Accounts,
            outcome,
            accounts: accounts
                .into_iter()
                .map(|account| AccountRow {
                    code: account.code.to_string(),
                    email: account.email.as_str().to_owned(),
                    created: beijing(account.created_at),
                    user: account.binding.map(|binding| binding.user.to_string()),
                    operator: account.operator,
                })
                .collect(),
            chrome: chrome(ctx),
        },
    )
}
