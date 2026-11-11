//! The public pages: home, download, how it works and help.

use askama::Template;
use axum::extract::State;
use axum::response::Response;

use super::{Chrome, Page, PageContext, render};
use crate::assets::{AssetPaths, VIEW};
use crate::error::AppError;
use crate::state::AppState;
use crate::view::ReleaseView;

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    chrome: Chrome,
    hero: String,
    scene: Scene,
}

/// What the home page tells its live scene's script: where its modules
/// are, and, as the text of the canvas's `data-view`, the size of the
/// picture it draws, in plain decimal numbers.
struct Scene {
    script: String,
    imported: Vec<String>,
    view: String,
}

impl Scene {
    fn of(paths: &AssetPaths) -> Self {
        Self {
            script: paths.script.clone(),
            imported: paths.imported.clone(),
            view: format!("{} {}", VIEW.0, VIEW.1),
        }
    }
}

#[derive(Template)]
#[template(path = "download.html")]
struct DownloadPage {
    chrome: Chrome,
    release: ReleaseView,
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

/// `GET /download`: the CFM release the host last imported. A release that
/// cannot be read is an error, never "none yet".
pub(crate) async fn download(
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let now = state.now()?;
    let offered = state
        .database()
        .run(|store| store.offered_release())
        .await?
        .map_err(AppError::internal)?;
    render(&DownloadPage {
        release: ReleaseView::of(offered, now, ctx.t()),
        chrome: ctx.chrome(Page::Download),
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
