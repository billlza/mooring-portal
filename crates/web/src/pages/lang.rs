//! Switching the language.

use axum::http::header::SET_COOKIE;
use axum::response::Response;
use serde::Deserialize;

use super::{Page, see_other};
use crate::error::AppError;
use crate::i18n::Lang;
use crate::security::cookies::{self, SameSite};
use crate::security::csrf::VerifiedForm;

/// How long the choice is remembered.
const REMEMBER_SECS: u64 = 365 * 86_400;

#[derive(Debug, Deserialize)]
pub(crate) struct LangForm {
    to: Lang,
    back: Page,
}

/// `POST /lang`: remembers the language and returns to the page it was
/// chosen on. The page is named, never given as an address, so the switch
/// cannot redirect anywhere else.
pub(crate) async fn switch(
    VerifiedForm(form): VerifiedForm<LangForm>,
) -> Result<Response, AppError> {
    let mut response = see_other(form.back.path());
    response.headers_mut().append(
        SET_COOKIE,
        cookies::set(
            cookies::LANG,
            form.to.code(),
            SameSite::Lax,
            Some(REMEMBER_SECS),
        )?,
    );
    Ok(response)
}
