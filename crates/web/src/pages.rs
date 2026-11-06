//! The pages, and what every page shares.

pub(crate) mod account;
pub(crate) mod auth;
pub(crate) mod cfm;
pub(crate) mod lang;
pub(crate) mod public;
pub(crate) mod subscription;

use askama::Template;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Redirect, Response};
use portal_store::Account;
use serde::Deserialize;

use crate::assets::AssetPaths;
use crate::brand::{Brand, brand};
use crate::error::AppError;
use crate::i18n::{Lang, Strings};
use crate::security::cookies;
use crate::security::csrf::CsrfToken;
use crate::security::session::Viewer;
use crate::state::AppState;

/// A page, as the navigation, the language switch and logins name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Page {
    Home,
    Download,
    Plans,
    Help,
    Login,
    Register,
    Account,
    Password,
    Subscription,
}

impl Page {
    /// The page's address.
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Download => "/download",
            Self::Plans => "/plans",
            Self::Help => "/help",
            Self::Login => "/login",
            Self::Register => "/register",
            Self::Account => "/account",
            Self::Password => "/account/password",
            Self::Subscription => "/subscription",
        }
    }

    /// The page's name in forms.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Download => "download",
            Self::Plans => "plans",
            Self::Help => "help",
            Self::Login => "login",
            Self::Register => "register",
            Self::Account => "account",
            Self::Password => "password",
            Self::Subscription => "subscription",
        }
    }
}

/// What every page template shows around its content.
#[derive(Debug)]
pub(crate) struct Chrome {
    pub(crate) lang: Lang,
    pub(crate) t: &'static Strings,
    pub(crate) brand: &'static Brand,
    pub(crate) csrf: String,
    pub(crate) signed_in: bool,
    pub(crate) page: Page,
    pub(crate) css: String,
    pub(crate) mark: String,
    /// Whether the header offers the language switch.
    pub(crate) language_switch: bool,
}

impl Chrome {
    /// Without the language switch: for a page that cannot be shown again,
    /// which switching would leave.
    pub(crate) const fn without_language_switch(mut self) -> Self {
        self.language_switch = false;
        self
    }

    /// `aria-current` for the navigation link to `key`.
    pub(crate) fn current(&self, key: &str) -> &'static str {
        if self.page.key() == key {
            "page"
        } else {
            "false"
        }
    }

    /// `aria-current` for the account tab `key`; changing the password
    /// belongs to the account tab.
    pub(crate) fn current_tab(&self, key: &str) -> &'static str {
        let tab = match self.page {
            Page::Password => Page::Account,
            page => page,
        };
        if tab.key() == key { "page" } else { "false" }
    }
}

/// What a page handler learns about its request: the language, the CSRF
/// token for its forms, and the signed-in account.
#[derive(Debug)]
pub(crate) struct PageContext {
    pub(crate) lang: Lang,
    pub(crate) account: Option<Account>,
    csrf: String,
    paths: AssetPaths,
}

impl FromRequestParts<AppState> for PageContext {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let lang = Lang::from_cookie(cookies::read(&parts.headers, cookies::LANG));
        let csrf = parts
            .extensions
            .get::<CsrfToken>()
            .ok_or_else(|| AppError::internal_message("the CSRF middleware did not run"))?
            .form_value();
        let Viewer(account) = Viewer::from_request_parts(parts, state).await?;
        Ok(Self {
            lang,
            account,
            csrf,
            paths: state.assets().paths().clone(),
        })
    }
}

impl PageContext {
    /// The text in this request's language.
    pub(crate) const fn t(&self) -> &'static Strings {
        self.lang.strings()
    }

    /// The surroundings of `page`.
    pub(crate) fn chrome(&self, page: Page) -> Chrome {
        Chrome {
            lang: self.lang,
            t: self.lang.strings(),
            brand: brand(self.lang),
            csrf: self.csrf.clone(),
            signed_in: self.account.is_some(),
            page,
            css: self.paths.css.clone(),
            mark: self.paths.mark.clone(),
            language_switch: true,
        }
    }

    /// The hero image's address.
    pub(crate) fn hero(&self) -> &str {
        &self.paths.hero
    }
}

/// `template` as an HTML page.
pub(crate) fn render(template: &impl Template) -> Result<Response, AppError> {
    Ok(Html(template.render()?).into_response())
}

/// `template` as an HTML page with `status`, for forms sent back with
/// errors.
pub(crate) fn render_status(
    status: StatusCode,
    template: &impl Template,
) -> Result<Response, AppError> {
    Ok((status, Html(template.render()?)).into_response())
}

/// After a form, the browser fetches `path` (303 See Other).
pub(crate) fn see_other(path: &str) -> Response {
    Redirect::to(path).into_response()
}

/// The login page, returning to `page` once logged in.
pub(crate) fn login_first(page: Page) -> Response {
    see_other(&format!("/login?next={}", page.key()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_name_their_paths() {
        for (page, path, key) in [
            (Page::Home, "/", "home"),
            (Page::Download, "/download", "download"),
            (Page::Plans, "/plans", "plans"),
            (Page::Help, "/help", "help"),
            (Page::Login, "/login", "login"),
            (Page::Register, "/register", "register"),
            (Page::Account, "/account", "account"),
            (Page::Password, "/account/password", "password"),
            (Page::Subscription, "/subscription", "subscription"),
        ] {
            assert_eq!((page.path(), page.key()), (path, key));
            let parsed: Page =
                serde_urlencoded::from_str::<Vec<(String, Page)>>(&format!("p={key}"))
                    .unwrap()
                    .remove(0)
                    .1;
            assert_eq!(parsed, page);
        }
    }
}
