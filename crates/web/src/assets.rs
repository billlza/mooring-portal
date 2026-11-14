//! Static assets: embedded in the binary, served under content-hashed
//! names that browsers may cache for a year.
//!
//! Nothing is fetched from another host: no web fonts, scripts or images
//! from CDNs, which are slow or blocked in mainland China. The hero image
//! is drawn by [`hero`] at startup.
//!
//! The one script is the home page's live scene, a handful of ES modules
//! that import each other by relative address. They are therefore served
//! from one directory, named by the content of all of them. The scene is an
//! ornament: every page works without it (`security::headers` lets only the
//! home page run it).

mod hero;

use std::collections::HashMap;
use std::fmt;

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderValue;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};

pub(crate) use hero::VIEW;

use crate::error::AppError;
use crate::state::AppState;

/// The stylesheet at a fixed address, for the self-contained error pages.
pub(crate) const STABLE_STYLESHEET: &str = "/assets/site.css";

/// The stylesheet, kept in one source file per part of the site and served
/// as one file.
const STYLESHEET: &str = concat!(
    include_str!("../assets/css/base.css"),
    include_str!("../assets/css/home.css"),
    include_str!("../assets/css/pages.css"),
    include_str!("../assets/css/account.css"),
    include_str!("../assets/css/rental.css"),
    include_str!("../assets/css/responsive.css"),
);
const MARK: &str = include_str!("../assets/mark.svg");

/// The module the home page loads; it imports the others.
const ENTRY: &str = "hero.mjs";
/// The live scene's modules, by file name: every file in `assets/js`.
/// Their tests are in `assets/js/test` and are not served.
const MODULES: [(&str, &str); 8] = [
    (ENTRY, include_str!("../assets/js/hero.mjs")),
    ("measure.mjs", include_str!("../assets/js/measure.mjs")),
    ("scene.mjs", include_str!("../assets/js/scene.mjs")),
    ("shader.mjs", include_str!("../assets/js/shader.mjs")),
    (
        "shader-shore.mjs",
        include_str!("../assets/js/shader-shore.mjs"),
    ),
    (
        "shader-sky.mjs",
        include_str!("../assets/js/shader-sky.mjs"),
    ),
    (
        "shader-water.mjs",
        include_str!("../assets/js/shader-water.mjs"),
    ),
    ("stage.mjs", include_str!("../assets/js/stage.mjs")),
];

const CSS: &str = "text/css; charset=utf-8";
const SVG: &str = "image/svg+xml";
const SCRIPT: &str = "text/javascript; charset=utf-8";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const SHORT: &str = "public, max-age=300";

/// The hashed addresses pages link to.
#[derive(Debug, Clone)]
pub(crate) struct AssetPaths {
    pub(crate) css: String,
    pub(crate) mark: String,
    /// The hero image.
    pub(crate) hero: String,
    /// The live scene's entry module.
    pub(crate) script: String,
    /// The modules the entry imports, which a page may fetch ahead of it.
    pub(crate) imported: Vec<String>,
}

#[derive(Clone)]
struct Asset {
    body: Bytes,
    content_type: &'static str,
    cache: &'static str,
}

/// Every asset, by the part of its address after `/assets/`.
pub(crate) struct Assets {
    files: HashMap<String, Asset>,
    paths: AssetPaths,
}

impl fmt::Debug for Assets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Assets")
            .field("paths", &self.paths)
            .finish()
    }
}

impl Assets {
    /// Draws the hero image and names every asset by its content.
    pub(crate) fn build() -> Self {
        let mut files = HashMap::new();
        let mut add = |stem: &str, ext: &str, body: Bytes, content_type| {
            let name = format!("{stem}.{:016x}.{ext}", fnv1a(&body));
            let path = format!("/assets/{name}");
            files.insert(
                name,
                Asset {
                    body,
                    content_type,
                    cache: IMMUTABLE,
                },
            );
            path
        };
        let css = add(
            "site",
            "css",
            Bytes::from_static(STYLESHEET.as_bytes()),
            CSS,
        );
        let mark = add("mark", "svg", Bytes::from_static(MARK.as_bytes()), SVG);
        let hero = add("hero", "svg", Bytes::from(hero::draw()), SVG);
        let directory = script_directory(&MODULES);
        for (name, source) in MODULES {
            files.insert(
                format!("{directory}/{name}"),
                Asset {
                    body: Bytes::from_static(source.as_bytes()),
                    content_type: SCRIPT,
                    cache: IMMUTABLE,
                },
            );
        }
        files.insert(
            "site.css".to_owned(),
            Asset {
                body: Bytes::from_static(STYLESHEET.as_bytes()),
                content_type: CSS,
                cache: SHORT,
            },
        );
        Self {
            files,
            paths: AssetPaths {
                css,
                mark,
                hero,
                script: format!("/assets/{directory}/{ENTRY}"),
                imported: MODULES
                    .iter()
                    .filter(|(name, _)| *name != ENTRY)
                    .map(|(name, _)| format!("/assets/{directory}/{name}"))
                    .collect(),
            },
        }
    }

    pub(crate) const fn paths(&self) -> &AssetPaths {
        &self.paths
    }
}

/// The directory `modules` are served from: named by all their names and
/// contents, so that a change to any one of them gives every one a new
/// address, and a module never imports a stale copy of another.
fn script_directory(modules: &[(&str, &str)]) -> String {
    let mut all = Vec::new();
    for (name, source) in modules {
        // A zero byte ends each part, so that where one ends matters.
        for part in [name, source] {
            all.extend_from_slice(part.as_bytes());
            all.push(0);
        }
    }
    format!("js.{:016x}", fnv1a(&all))
}

/// The source of the live scene's module `name`, for tests of what the
/// page and the script agree on.
#[cfg(test)]
pub(crate) fn module(name: &str) -> &'static str {
    MODULES
        .iter()
        .find(|(module, _)| *module == name)
        .map(|(_, source)| *source)
        .unwrap()
}

/// `GET /assets/{*file}`. A path that is not text names no asset, like
/// any other that names none.
pub(crate) async fn serve(
    State(state): State<AppState>,
    file: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let asset = file
        .ok()
        .and_then(|Path(file)| state.assets().files.get(&file).cloned())
        .ok_or(AppError::NotFound)?;
    Ok((
        [
            (CONTENT_TYPE, HeaderValue::from_static(asset.content_type)),
            (CACHE_CONTROL, HeaderValue::from_static(asset.cache)),
        ],
        asset.body,
    )
        .into_response())
}

/// `GET /robots.txt`: the portal serves a handful of invited people and
/// asks search engines not to list it.
pub(crate) async fn robots() -> impl IntoResponse {
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        "User-agent: *\nDisallow: /\n",
    )
}

/// FNV-1a, 64 bits: names assets by content, so a changed file gets a new
/// address. It detects change; it is not a security check.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests;
