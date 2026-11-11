//! The subscription page.

use askama::Template;
use axum::extract::State;
use axum::response::Response;
use portal_store::{StoreError, Subscription};
use serde::Deserialize;

use super::{Chrome, Page, PageContext, PageQuery, login_first, render};
use crate::error::AppError;
use crate::state::AppState;
use crate::view::{CfmPanel, ShopView, SubscriptionView};

/// How many of the account's orders the page lists: the newest.
const ORDERS_SHOWN: u32 = 12;

#[derive(Template)]
#[template(path = "subscription.html")]
struct SubscriptionPage {
    chrome: Chrome,
    notice: Option<&'static str>,
    view: SubscriptionView,
    /// For a bound account.
    cfm: Option<CfmPanel>,
    /// For a bound account, once the portal sells anything.
    shop: Option<ShopView>,
}

/// A one-off message after a redirect, named by the redirect itself.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notice {
    LinkRevoked,
    LinkUnavailable,
    OrderUnbound,
    OrderRenewing,
    PlanGone,
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
    PageQuery(query): PageQuery<SubscriptionQuery>,
) -> Result<Response, AppError> {
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let id = account.id;
    let now = state.now()?;
    let selling = state.payments().is_some();
    let (subscription, delivery, bought) = state
        .database()
        .run(move |store| -> Result<_, StoreError> {
            let subscription = store.subscription(id)?;
            // Only a bound account can order, so only one has a shop.
            let bought = match (&subscription, selling) {
                (Subscription::Bound { .. }, true) => {
                    Some((store.orders_of(id, ORDERS_SHOWN)?, store.renewals_of(id)?))
                }
                _ => None,
            };
            Ok((subscription, store.cfm_delivery(id)?, bought))
        })
        .await?
        .map_err(AppError::internal)?;
    let t = ctx.t();
    let cfm = match subscription {
        Subscription::Unbound => None,
        Subscription::Bound { .. } => Some(CfmPanel::of(delivery, t)),
    };
    let shop = state
        .payments()
        .zip(bought)
        .map(|(payments, (orders, renewals))| {
            ShopView::of(payments.catalogue(), &orders, &renewals, t)
        });
    render(&SubscriptionPage {
        notice: query.notice.and_then(|notice| match notice {
            Notice::LinkRevoked => Some(t.cfm.link_revoked),
            Notice::LinkUnavailable => Some(t.cfm.link_unavailable),
            // Where nothing is sold, nothing is said of ordering.
            Notice::OrderUnbound => selling.then_some(t.pay.notice_unbound),
            Notice::OrderRenewing => selling.then_some(t.pay.auto_blocked),
            Notice::PlanGone => selling.then_some(t.pay.notice_plan_gone),
        }),
        view: SubscriptionView::of(&subscription, now, t),
        cfm,
        shop,
        chrome: ctx.chrome(Page::Subscription),
    })
}
