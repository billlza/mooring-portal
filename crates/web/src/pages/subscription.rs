//! The subscription page.

use askama::Template;
use axum::extract::State;
use axum::response::Response;

use super::{Chrome, Page, PageContext, login_first, render};
use crate::error::AppError;
use crate::state::AppState;
use crate::view::SubscriptionView;

#[derive(Template)]
#[template(path = "subscription.html")]
struct SubscriptionPage {
    chrome: Chrome,
    view: SubscriptionView,
}

/// `GET /subscription`: the signed-in account's own subscription. The
/// account comes from the session alone; no address or form names one.
pub(crate) async fn subscription(
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let id = account.id;
    let now = state.now()?;
    let subscription = state
        .database()
        .run(move |store| store.subscription(id))
        .await?
        .map_err(AppError::internal)?;
    render(&SubscriptionPage {
        view: SubscriptionView::of(&subscription, now, ctx.t()),
        chrome: ctx.chrome(Page::Subscription),
    })
}
