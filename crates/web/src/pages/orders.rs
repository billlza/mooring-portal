//! Ordering a plan, and an order's page.
//!
//! An order is placed for the signed-in account alone, as the store finds
//! it in the writing transaction; no address or form names an account.
//! The form names only a plan, by the id the catalogue gives it, and the
//! order takes its terms from the catalogue, never from the form. An
//! order's page is its owner's: anyone else's request for it is answered
//! as for an order that does not exist.
//!
//! Paying happens on Stripe's site. The page links there; it cannot post
//! a form there, since the content security policy lets this site's forms
//! go nowhere but here.

use askama::Template;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use portal_core::payment::{OrderId, OrderTerms};
use portal_store::{IntroState, OrderError, StoreError};
use serde::Deserialize;

use super::{Chrome, Page, PageContext, login_first, render, see_other};
use crate::error::AppError;
use crate::security::csrf::VerifiedForm;
use crate::security::session::presented_token;
use crate::state::AppState;
use crate::view::{OrderView, PayAction};

/// Where an order's page is: `/subscription/orders/<id>`.
const ORDER_PREFIX: &str = "/subscription/orders/";

#[derive(Template)]
#[template(path = "order.html")]
struct OrderPage {
    chrome: Chrome,
    order: OrderView,
    pay: PayAction,
    test_mode: bool,
}

/// The order form: the plan, by its id.
#[derive(Debug, Deserialize)]
pub(crate) struct OrderForm {
    plan: String,
}

/// `POST /subscription/orders`: places an order for the plan the form
/// names, or gives the account's unpaid order for it, and shows it.
pub(crate) async fn place(
    State(state): State<AppState>,
    headers: HeaderMap,
    VerifiedForm(form): VerifiedForm<OrderForm>,
) -> Result<Response, AppError> {
    let payments = state.selling()?;
    let Some(session) = presented_token(&headers) else {
        return Ok(login_first(Page::Subscription));
    };
    let Some(plan) = payments.catalogue().plan(&form.plan) else {
        return Ok(see_other("/subscription?notice=plan_gone"));
    };
    let terms = OrderTerms::of(plan);
    let introductory = plan.introductory;
    let session = session.digest();
    let fresh = OrderId::generate().map_err(AppError::internal)?;
    let now = state.now()?;
    let placed = state
        .database()
        .run(move |store| {
            if introductory {
                store.place_introductory_order(&session, &terms, fresh, now)
            } else {
                store.place_order(&session, &terms, fresh, now)
            }
        })
        .await?;
    match placed {
        Ok(order) => Ok(see_other(&format!("{ORDER_PREFIX}{order}"))),
        Err(OrderError::SignedOut) => Ok(login_first(Page::Subscription)),
        Err(OrderError::Unbound) => Ok(see_other("/subscription?notice=order_unbound")),
        Err(OrderError::Renewing) => Ok(see_other("/subscription?notice=order_renewing")),
        Err(OrderError::Introductory) => Err(AppError::NotFound),
        Err(OrderError::Store(error)) => Err(AppError::internal(error)),
    }
}

/// `GET /subscription/orders/<id>`: the signed-in account's order, with
/// the way to pay it while it is unpaid.
pub(crate) async fn show(
    State(state): State<AppState>,
    ctx: PageContext,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let payments = state.selling()?;
    let Some(account) = &ctx.account else {
        return Ok(login_first(Page::Subscription));
    };
    let Some(id) = id.ok().and_then(|Path(text)| OrderId::parse(&text).ok()) else {
        return Err(AppError::NotFound);
    };
    let account = account.id;
    let (order, binding, intro) = state
        .database()
        .run(move |store| -> Result<_, StoreError> {
            Ok((
                store.order_of(account, &id)?,
                store.binding(account)?,
                store.introductory_offer(account)?,
            ))
        })
        .await?
        .map_err(AppError::internal)?;
    let Some(order) = order else {
        return Err(AppError::NotFound);
    };
    let catalogue = payments.catalogue();
    let private_link = match intro {
        IntroState::Available(link) => Some(link),
        _ => None,
    };
    render(&OrderPage {
        pay: PayAction::with_intro(
            &order,
            catalogue,
            binding.as_ref().map(|binding| &binding.user),
            ctx.lang,
            private_link.as_ref(),
        ),
        order: OrderView::of(&order, ctx.t()),
        test_mode: !catalogue.livemode(),
        // Switching the language would leave the order for the
        // subscription page.
        chrome: ctx.chrome(Page::Subscription).without_language_switch(),
    })
}
