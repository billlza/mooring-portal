//! The orders that wait, approving one, and the latest orders
//! (`docs/payments.md`).
//!
//! The page lists what `orders` on the host lists as waiting and lets the
//! operator approve an order, as `approve-order` does, for the one thing
//! the page showed it waits for: the form names that thing, and an order
//! that waits for something else by the time the form arrives, such as one
//! a first click approved already, is not approved again. What an approval
//! leaves waiting is said at once, on the page its form returns. Refunds
//! and cancellations are made at Stripe, which the portal holds no key
//! for. A payment that is no order's is counted here and settled with the
//! commands on the host. An order disputed before it was granted is listed
//! apart and not counted: the portal does not follow how a dispute ends,
//! so nothing here would ever settle it.

use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use portal_core::payment::OrderId;
use portal_store::{AsOperator, Progress, StoreError};
use serde::Deserialize;

use super::{Confirmed, OperatorForm, Outcome, Section, chrome, turned_away};
use crate::error::AppError;
use crate::i18n::fill;
use crate::pages::{Chrome, PageContext, render_status};
use crate::state::AppState;
use crate::view::{OrderView, WaitingView, approve_refusal, shown_from_key, why_it_waits};

/// How many of the newest orders the page lists.
const LATEST_ORDERS: u32 = 20;

crate::pages::page_templates!(PaymentsPage);

#[derive(Template)]
#[template(path = "operator_payments.html")]
struct PaymentsPage {
    chrome: Chrome,
    section: Section,
    outcome: Outcome,
    waiting: Vec<WaitingView>,
    /// The orders that wait but are disputed: listed apart, as nothing
    /// the portal follows settles them.
    disputed: Vec<WaitingView>,
    /// How many payments are no order's, which wait for the operator on
    /// the host, if any are.
    strays: Option<String>,
    /// How many refunds and disputes are of payments no order names,
    /// which the host lists for reference alone, if any are.
    unmatched: Option<String>,
    latest: Vec<LatestRow>,
}

/// One of the newest orders.
struct LatestRow {
    order: OrderView,
    account: String,
}

/// The approving form: the order, and what the page showed it waits for.
#[derive(Debug, Deserialize)]
pub(crate) struct ApproveForm {
    order: String,
    shown: String,
}

/// `GET /operator/payments`: the orders that wait, and the latest.
pub(crate) async fn payments(
    _: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    page(&state, &ctx, Outcome::default(), StatusCode::OK).await
}

/// `POST /operator/payments/approve`: lets the order the form names
/// through the one thing the page showed it waits for.
pub(crate) async fn approve(
    State(state): State<AppState>,
    ctx: PageContext,
    OperatorForm { gate, form }: OperatorForm<ApproveForm>,
) -> Result<Response, AppError> {
    // The page writes both fields: a form with anything else in them is
    // not the page's.
    let id = OrderId::parse(&form.order)
        .map_err(|_| AppError::BadRequest("the order is not an order's id"))?;
    let shown = shown_from_key(&form.shown).ok_or(AppError::BadRequest(
        "the form does not say what the page showed",
    ))?;
    let now = state.now()?;
    let approved = state
        .database()
        .run(move |store| {
            store.as_operator(&gate.digest, now, |pages| pages.approve_order(&id, shown))
        })
        .await?;
    let t = ctx.t();
    let r = &t.review;
    let (outcome, status) = match approved {
        Ok(AsOperator::NotAllowed(not)) => return Ok(turned_away(not, Section::Payments)),
        Ok(AsOperator::Done(approved)) => {
            tracing::info!("the operator approved an order on the operator pages");
            let notice = match approved.standing {
                Progress::Granted { .. } => fill(r.approved_granted, id),
                Progress::Review(reason) => {
                    fill(&fill(r.approved_again, id), why_it_waits(reason, t))
                }
                // Paid, and waiting for word of its card: an order unpaid,
                // or refunded in full, is not approved at all.
                _ => fill(r.approved_checking, id),
            };
            (Outcome::done(notice), StatusCode::OK)
        }
        Err(error) => match approve_refusal(&error, t) {
            Some(refusal) => (Outcome::refused(t, &refusal), StatusCode::CONFLICT),
            None => return Err(AppError::internal(error)),
        },
    };
    page(&state, &ctx, outcome, status).await
}

/// The payments page, after `outcome`. Its counts are read with its
/// lists, after whatever `outcome` reports was done.
async fn page(
    state: &AppState,
    ctx: &PageContext,
    outcome: Outcome,
    status: StatusCode,
) -> Result<Response, AppError> {
    let viewer = ctx.account.as_ref().map(|account| account.id);
    let (waiting, stray_count, unmatched_count, latest, signals) = state
        .database()
        .run(move |store| -> Result<_, StoreError> {
            Ok((
                store.orders_waiting()?,
                store.stray_payments()?.len(),
                store.unmatched_payments()?.len(),
                store.recent_orders(LATEST_ORDERS)?,
                viewer.map(|account| store.signals(account)).transpose()?,
            ))
        })
        .await?
        .map_err(AppError::internal)?;
    let t = ctx.t();
    let (disputed, waiting): (Vec<_>, Vec<_>) = waiting
        .iter()
        .map(|listed| (listed.order.disputed, WaitingView::of(listed, t)))
        .partition(|(disputed, _)| *disputed);
    let strays = (stray_count > 0).then(|| fill(t.review.elsewhere_strays, stray_count));
    let unmatched =
        (unmatched_count > 0).then(|| fill(t.review.elsewhere_unmatched, unmatched_count));
    let mut chrome = chrome(ctx);
    if let Some(signals) = signals {
        chrome.signals = signals;
    }
    render_status(
        status,
        &PaymentsPage {
            section: Section::Payments,
            outcome,
            waiting: waiting.into_iter().map(|(_, row)| row).collect(),
            disputed: disputed.into_iter().map(|(_, row)| row).collect(),
            strays,
            unmatched,
            latest: latest
                .iter()
                .map(|listed| LatestRow {
                    order: OrderView::of(&listed.order, t),
                    account: listed.account.to_string(),
                })
                .collect(),
            chrome,
        },
    )
}
