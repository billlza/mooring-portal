//! The public pages: home, download, how it works and help.

use askama::Template;
use axum::extract::State;
use axum::response::Response;

use super::{Chrome, Page, PageContext, render};
use crate::assets::{AssetPaths, VIEW};
use crate::error::AppError;
use crate::i18n::Item;
use crate::state::AppState;
use crate::view::{PlanView, ReleaseView};

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    chrome: Chrome,
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
    /// What is on sale, once the portal sells anything.
    prices: Option<Prices>,
    /// How one pays, as things stand.
    payment: &'static Item,
}

/// The plans page's price list.
struct Prices {
    plans: Vec<PlanView>,
    /// The catalogue's payment pages take no real payments.
    test_mode: bool,
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

/// `GET /plans`: how the service works and, once the portal sells
/// anything, what it sells. Ordering is on the subscription page, where
/// the account is known.
pub(crate) async fn plans(
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    let t = ctx.t();
    let prices = state.payments().map(|payments| {
        let catalogue = payments.catalogue();
        Prices {
            plans: catalogue
                .plans()
                .iter()
                .filter(|plan| !plan.introductory)
                .map(|plan| PlanView::of(plan, false, t))
                .collect(),
            test_mode: !catalogue.livemode(),
        }
    });
    render(&PlansPage {
        payment: if prices.is_some() {
            &t.plans.payment_open
        } else {
            &t.plans.payment_closed
        },
        prices,
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
