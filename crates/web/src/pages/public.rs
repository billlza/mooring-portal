//! The public pages: home, download, how it works and help.

use askama::Template;
use axum::response::Response;

use super::{Chrome, Page, PageContext, render};
use crate::error::AppError;
use crate::release::{MAC, MacRelease};

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    chrome: Chrome,
    hero: String,
}

#[derive(Template)]
#[template(path = "download.html")]
struct DownloadPage {
    chrome: Chrome,
    mac: &'static MacRelease,
}

#[derive(Template)]
#[template(path = "plans.html")]
struct PlansPage {
    chrome: Chrome,
}

#[derive(Template)]
#[template(path = "help.html")]
struct HelpPage {
    chrome: Chrome,
}

/// `GET /`.
pub(crate) async fn home(ctx: PageContext) -> Result<Response, AppError> {
    render(&HomePage {
        hero: ctx.hero().to_owned(),
        chrome: ctx.chrome(Page::Home),
    })
}

/// `GET /download`.
pub(crate) async fn download(ctx: PageContext) -> Result<Response, AppError> {
    render(&DownloadPage {
        chrome: ctx.chrome(Page::Download),
        mac: &MAC,
    })
}

/// `GET /plans`.
pub(crate) async fn plans(ctx: PageContext) -> Result<Response, AppError> {
    render(&PlansPage {
        chrome: ctx.chrome(Page::Plans),
    })
}

/// `GET /help`.
pub(crate) async fn help(ctx: PageContext) -> Result<Response, AppError> {
    render(&HelpPage {
        chrome: ctx.chrome(Page::Help),
    })
}

/// Any other address.
pub(crate) async fn not_found() -> AppError {
    AppError::NotFound
}
