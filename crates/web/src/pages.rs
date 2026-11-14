//! The pages, and what every page shares.

pub(crate) mod account;
pub(crate) mod auth;
pub(crate) mod cfm;
pub(crate) mod feedback;
pub(crate) mod lang;
pub(crate) mod operator;
pub(crate) mod orders;
pub(crate) mod public;
pub(crate) mod rental;
pub(crate) mod subscription;

use askama::Template;
use axum::extract::{FromRequestParts, Query};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Redirect, Response};
use portal_core::feedback::MAX_MESSAGE_CHARS;
use portal_store::{Account, Signals};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::brand::{Brand, brand};
use crate::error::AppError;
use crate::i18n::{Lang, Strings, fill};
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
    Mac,
    Help,
    Login,
    Register,
    Account,
    Password,
    Subscription,
    Feedback,
    Operator,
}

impl Page {
    /// The page's address.
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Download => "/download",
            Self::Plans => "/plans",
            Self::Mac => "/mac",
            Self::Help => "/help",
            Self::Login => "/login",
            Self::Register => "/register",
            Self::Account => "/account",
            Self::Password => "/account/password",
            Self::Subscription => "/subscription",
            Self::Feedback => "/feedback",
            Self::Operator => "/operator",
        }
    }

    /// The page's name in forms.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Download => "download",
            Self::Plans => "plans",
            Self::Mac => "mac",
            Self::Help => "help",
            Self::Login => "login",
            Self::Register => "register",
            Self::Account => "account",
            Self::Password => "password",
            Self::Subscription => "subscription",
            Self::Feedback => "feedback",
            Self::Operator => "operator",
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
    /// What the signed-in account is shown besides the page's content.
    pub(crate) signals: Signals,
    pub(crate) page: Page,
    pub(crate) css: String,
    pub(crate) mark: String,
    /// The drawn picture of the home page, which every other page opens on.
    pub(crate) hero: String,
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

    /// Whether the page offers the feedback box: to a signed-in account
    /// that is no operator, on every page but the conversation itself.
    /// The operator answers feedback rather than sends it.
    pub(crate) fn feedback_box(&self) -> bool {
        self.signed_in
            && !self.signals.operator
            && !matches!(self.page, Page::Feedback | Page::Operator)
    }

    /// The most characters a message to or from the operator may hold.
    pub(crate) const fn max_message_chars(&self) -> usize {
        MAX_MESSAGE_CHARS
    }

    /// The hint under a message's field: how long it may be.
    pub(crate) fn message_limit(&self) -> String {
        fill(self.t.feedback.limit, MAX_MESSAGE_CHARS)
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
    pub(crate) signals: Signals,
    csrf: String,
    css: String,
    mark: String,
    hero: String,
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
        let Viewer(viewer) = Viewer::from_request_parts(parts, state).await?;
        let (account, signals) = viewer.map_or((None, Signals::default()), |(account, signals)| {
            (Some(account), signals)
        });
        let paths = state.assets().paths();
        Ok(Self {
            lang,
            account,
            signals,
            csrf,
            css: paths.css.clone(),
            mark: paths.mark.clone(),
            hero: paths.hero.clone(),
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
            signals: self.signals,
            page,
            css: self.css.clone(),
            mark: self.mark.clone(),
            hero: self.hero.clone(),
            language_switch: true,
        }
    }
}

/// A page's query string. One that does not parse is refused with this
/// site's own error page: the framework's refusal would quote the request
/// back to whoever sent it.
pub(crate) struct PageQuery<T>(pub(crate) T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for PageQuery<T> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(query)| Self(query))
            .map_err(|_| AppError::BadRequest("the query string does not parse"))
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
            (Page::Mac, "/mac", "mac"),
            (Page::Help, "/help", "help"),
            (Page::Login, "/login", "login"),
            (Page::Register, "/register", "register"),
            (Page::Account, "/account", "account"),
            (Page::Password, "/account/password", "password"),
            (Page::Subscription, "/subscription", "subscription"),
            (Page::Feedback, "/feedback", "feedback"),
            (Page::Operator, "/operator", "operator"),
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
