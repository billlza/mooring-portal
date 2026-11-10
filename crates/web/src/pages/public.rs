//! The public pages: home, download, how it works and help.

use askama::Template;
use axum::extract::State;
use axum::response::Response;

use super::{Chrome, Page, PageContext, render};
use crate::assets::{AssetPaths, GEOMETRY};
use crate::error::AppError;
use crate::release::{MAC, MacRelease};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    chrome: Chrome,
    hero: String,
    scene: Scene,
}

/// What the home page tells its live scene's script, as the text of the
/// canvas's `data-*` attributes: the layers to load, and where the
/// picture's fixed points are, as plain decimal numbers.
struct Scene {
    script: String,
    imported: Vec<String>,
    land: String,
    ring: String,
    view: String,
    pole: String,
    span: String,
    shore: String,
    lantern: String,
}

impl Scene {
    fn of(paths: &AssetPaths) -> Self {
        let at = GEOMETRY;
        Self {
            script: paths.script.clone(),
            imported: paths.imported.clone(),
            land: paths.land.clone(),
            ring: paths.ring.clone(),
            view: format!("{} {}", at.width, at.height),
            pole: format!("{} {}", at.pole_x, at.pole_y),
            span: at.span.to_string(),
            shore: at.shore.to_string(),
            lantern: format!("{} {}", at.lantern_x, at.lantern_y),
        }
    }
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
pub(crate) async fn home(
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let paths = state.assets().paths();
    render(&HomePage {
        hero: paths.hero.clone(),
        scene: Scene::of(paths),
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
