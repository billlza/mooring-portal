//! The HTTP front end of the Mooring portal.
//!
//! Server-rendered pages that work without JavaScript: the public pages
//! (home, download, how it works, help), registration, login and logout,
//! and the signed-in account and subscription pages, from which a user
//! downloads their CFM profile or makes the subscription link CFM fetches
//! it from; and the Mac rental page, where a signed-in user books a machine
//! for the operator to buy and let to them. A signed-in user writes to the
//! operator from any page and reads the reply on the feedback page; the
//! operator answers on the operator pages, where it also binds, unbinds
//! and bans accounts, closes bookings and approves orders that wait for it.
//! Only an account the host made an
//! operator opens them, and only after confirming its password lately
//! (`pages::operator`). Every state-changing request is a form protected
//! against CSRF; every response carries security headers; no asset comes
//! from another host. The one script is an ornament, the home page's live
//! scene, and only that page may run it.
//!
//! Once the operator has given it a catalogue, the portal also takes
//! orders: a bound account orders a plan and is sent to pay on Stripe's
//! site, Stripe tells the portal what was paid, and Mooring pulls what to
//! grant. The portal holds no key that could move money (`machines`).

mod assets;
mod brand;
mod config;
mod db;
mod error;
mod format;
mod i18n;
mod machines;
mod pages;
mod security;
mod state;
mod view;

use std::future::Future;
use std::io;
use std::net::SocketAddr;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use tokio::net::TcpListener;

pub use config::{
    ClientIpSource, OriginError, Payments, PublicOrigin, SESSION_TTL_SECS, SecretCount, WebConfig,
};
pub use state::{AppState, StartupError};

use pages::{
    Page, account, auth, cfm, feedback, lang, operator, orders, public, rental, subscription,
};
use security::{csrf, headers};

/// The portal's routes. The addresses of ordering and paying exist only
/// for a portal that sells: without a catalogue they are answered like
/// any address that was never one, whatever the method.
pub fn router(state: AppState) -> Router {
    let selling = state.payments().is_some();
    let mut pages = Router::new()
        .route(Page::Home.path(), get(public::home))
        .route("/download", get(public::download))
        .route("/plans", get(public::plans))
        .route("/help", get(public::help))
        .route(Page::Mac.path(), get(rental::mac))
        .route("/mac/bookings", post(rental::book))
        .route("/mac/bookings/withdraw", post(rental::withdraw))
        .route("/login", get(auth::login_page).post(auth::login))
        .route("/register", get(auth::register_page).post(auth::register))
        .route("/logout", post(auth::logout))
        .route("/account", get(account::account))
        .route(
            "/account/password",
            get(account::password_page).post(account::change_password),
        )
        .route("/subscription", get(subscription::subscription))
        .route("/subscription/profile.json", get(cfm::download))
        .route("/subscription/link", post(cfm::create_link))
        .route("/subscription/link/revoke", post(cfm::revoke_link))
        .route(
            Page::Feedback.path(),
            get(feedback::feedback).post(feedback::send),
        )
        .route(Page::Operator.path(), get(operator::overview))
        .route(
            "/operator/confirm",
            get(operator::confirm_form).post(operator::confirm),
        )
        .route("/operator/feedback", get(operator::conversations))
        .route(
            "/operator/feedback/{code}",
            get(operator::conversation).post(operator::reply),
        )
        .route("/operator/accounts", get(operator::accounts))
        .route("/operator/accounts/bind", post(operator::bind))
        .route("/operator/accounts/unbind", post(operator::unbind))
        .route("/operator/accounts/ban", post(operator::ban))
        .route("/operator/bookings", get(operator::bookings))
        .route("/operator/bookings/close", post(operator::close_booking))
        .route("/operator/payments", get(operator::payments))
        .route("/operator/payments/approve", post(operator::approve))
        .route("/lang", post(lang::switch));
    if selling {
        pages = pages
            .route("/subscription/orders", post(orders::place))
            .route("/subscription/orders/{*id}", get(orders::show));
    }
    let pages = pages.layer(middleware::from_fn(csrf::ensure_token));
    let files = Router::new()
        .route("/assets/{*file}", get(assets::serve))
        .route("/robots.txt", get(assets::robots));
    // What CFM fetches carries no cookie and gets none back. Every address
    // under /sub/ is answered there, in plain text when it fails, so that
    // no error page sends a token on as the referrer.
    let links = Router::new().route("/sub/{*token}", get(cfm::fetch));
    // What machines call: Stripe with its deliveries, Mooring for the
    // entitlements. No cookie either way, and plain text when they fail.
    let machines = if selling {
        Router::new()
            .route("/stripe/webhook", post(machines::deliver))
            .route("/api/mooring/entitlements", get(machines::pull))
    } else {
        Router::new()
    };
    pages
        .merge(files)
        .merge(links)
        .merge(machines)
        .fallback(public::not_found)
        // Only the home page may run this site's scripts.
        .layer(middleware::from_fn(|request, next| {
            headers::add(Page::Home.path(), request, next)
        }))
        .with_state(state)
}

/// Serves the portal on `listener` until `shutdown` completes, then
/// finishes the requests in flight.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

#[cfg(test)]
mod tests;
