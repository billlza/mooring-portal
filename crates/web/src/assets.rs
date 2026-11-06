//! Static assets: embedded in the binary, served under content-hashed
//! names that browsers may cache for a year.
//!
//! Nothing is fetched from another host: no web fonts, scripts or images
//! from CDNs, which are slow or blocked in mainland China. The hero image
//! is drawn by [`hero`] at startup.

mod hero;

use std::collections::HashMap;
use std::fmt;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderValue;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};

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
    include_str!("../assets/css/responsive.css"),
);
const MARK: &str = include_str!("../assets/mark.svg");

const CSS: &str = "text/css; charset=utf-8";
const SVG: &str = "image/svg+xml";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const SHORT: &str = "public, max-age=300";

/// The hashed addresses pages link to.
#[derive(Debug, Clone)]
pub(crate) struct AssetPaths {
    pub(crate) css: String,
    pub(crate) mark: String,
    pub(crate) hero: String,
}

#[derive(Clone)]
struct Asset {
    body: Bytes,
    content_type: &'static str,
    cache: &'static str,
}

/// Every asset, by file name.
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
        let hero = add("hero", "svg", Bytes::from(hero::svg()), SVG);
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
            paths: AssetPaths { css, mark, hero },
        }
    }

    pub(crate) const fn paths(&self) -> &AssetPaths {
        &self.paths
    }
}

/// `GET /assets/{file}`.
pub(crate) async fn serve(
    State(state): State<AppState>,
    Path(file): Path<String>,
) -> Result<Response, AppError> {
    let asset = state
        .assets()
        .files
        .get(&file)
        .cloned()
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
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_its_reference_values() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn assets_are_named_by_their_content() {
        let assets = Assets::build();
        let paths = assets.paths();
        for path in [&paths.css, &paths.mark, &paths.hero] {
            let name = path.strip_prefix("/assets/").unwrap();
            assert!(assets.files.contains_key(name), "{path}");
        }
        assert_eq!(
            paths.css,
            format!("/assets/site.{:016x}.css", fnv1a(STYLESHEET.as_bytes()))
        );
        assert!(assets.files.contains_key("site.css"));
        assert!(format!("{assets:?}").starts_with("Assets { paths"));
    }
}
