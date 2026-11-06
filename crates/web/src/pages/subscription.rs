//! The subscription page.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use portal_store::{StoreError, Subscription};
use serde::Deserialize;

use super::{Chrome, Page, PageContext, login_first, render};
use crate::error::AppError;
use crate::state::AppState;
use crate::view::{CfmPanel, SubscriptionView};

#[derive(Template)]
#[template(path = "subscription.html")]
struct SubscriptionPage {
    chrome: Chrome,
    notice: Option<&'static str>,
    view: SubscriptionView,
    /// For a bound account.
    cfm: Option<CfmPanel>,
}

/// A one-off message after a redirect, named by the redirect itself.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notice {
    LinkRevoked,
    LinkUnavailable,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionQuery {
    notice: Option<Notice>,
}

/// `GET /subscription`: the signed-in account's own subscription and how
/// to get its CFM profile. The account comes from the session alone; no
/// address or form names one.
pub(crate) async fn subscription(
    State(state): State<AppState>,
    ctx: PageContext,
    Query(query): Query<SubscriptionQuery>,
) -> Result<Response, AppError> {
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let id = account.id;
    let now = state.now()?;
    let (subscription, delivery) = state
        .database()
        .run(move |store| -> Result<_, StoreError> {
            Ok((store.subscription(id)?, store.cfm_delivery(id)?))
        })
        .await?
        .map_err(AppError::internal)?;
    let t = ctx.t();
    let cfm = match subscription {
        Subscription::Unbound => None,
        Subscription::Bound { .. } => Some(CfmPanel::of(delivery, t)),
    };
    render(&SubscriptionPage {
        notice: query.notice.map(|notice| match notice {
            Notice::LinkRevoked => t.cfm.link_revoked,
            Notice::LinkUnavailable => t.cfm.link_unavailable,
        }),
        view: SubscriptionView::of(&subscription, now, t),
        cfm,
        chrome: ctx.chrome(Page::Subscription),
    })
}
