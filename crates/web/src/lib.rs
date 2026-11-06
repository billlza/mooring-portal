//! The HTTP front end of the Mooring portal.
//!
//! Server-rendered pages without JavaScript: the public pages (home,
//! download, how it works, help), registration, login and logout, and the
//! signed-in account and subscription pages. Every state-changing request
//! is a form protected against CSRF; every response carries security
//! headers; no asset comes from another host.

mod assets;
mod brand;
mod config;
mod db;
mod error;
mod format;
mod i18n;
mod pages;
mod release;
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

pub use config::{ClientIpSource, OriginError, PublicOrigin, SESSION_TTL_SECS, WebConfig};
pub use state::{AppState, StartupError};

use pages::{account, auth, lang, public, subscription};
use security::{csrf, headers};

/// The portal's routes.
pub fn router(state: AppState) -> Router {
    let pages = Router::new()
        .route("/", get(public::home))
        .route("/download", get(public::download))
        .route("/plans", get(public::plans))
        .route("/help", get(public::help))
        .route("/login", get(auth::login_page).post(auth::login))
        .route("/register", get(auth::register_page).post(auth::register))
        .route("/logout", post(auth::logout))
        .route("/account", get(account::account))
        .route(
            "/account/password",
            get(account::password_page).post(account::change_password),
        )
        .route("/subscription", get(subscription::subscription))
        .route("/lang", post(lang::switch))
        .layer(middleware::from_fn(csrf::ensure_token));
    let files = Router::new()
        .route("/assets/{file}", get(assets::serve))
        .route("/robots.txt", get(assets::robots));
    pages
        .merge(files)
        .fallback(public::not_found)
        .layer(middleware::from_fn(headers::add))
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
