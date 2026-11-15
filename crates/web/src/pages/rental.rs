//! The Mac rental page, and booking a machine on it.
//!
//! Anyone may look; booking needs an account. A booking is made for the
//! signed-in account alone, as the store finds it in the writing
//! transaction; no address or form names an account. The form names a
//! machine and a period by their ids, and the booking takes its terms
//! from the offer, never from the form. Nothing is paid here: the
//! operator settles each booking with whoever made it (`docs/rentals.md`).

use askama::Template;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use portal_core::rental::{BookingTerms, MACHINES, Machine, MachineId, Period};
use portal_store::BookingError;
use serde::Deserialize;

use super::{Chrome, Page, PageContext, PageQuery, login_first, render, see_other};
use crate::error::AppError;
use crate::security::csrf::VerifiedForm;
use crate::security::session::presented_token;
use crate::state::AppState;
use crate::view::{BookingView, MachineView, PeriodView};

crate::pages::page_templates!(MacPage);

#[derive(Template)]
#[template(path = "mac.html")]
struct MacPage {
    chrome: Chrome,
    notice: Option<&'static str>,
    machines: Vec<MachineView>,
    periods: Vec<PeriodView>,
    /// For a signed-in visitor.
    bookings: Option<Vec<BookingView>>,
}

/// A one-off message after a redirect, named by the redirect itself.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notice {
    Booked,
    Withdrawn,
    MachineGone,
}

impl Notice {
    /// Where the browser goes after a form that ends with `self`: the
    /// page, which lists the account's bookings first.
    const fn location(self) -> &'static str {
        match self {
            Self::Booked => "/mac?notice=booked",
            Self::Withdrawn => "/mac?notice=withdrawn",
            Self::MachineGone => "/mac?notice=machine_gone",
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct MacQuery {
    notice: Option<Notice>,
}

/// The booking form: a machine and a period, by their ids.
#[derive(Debug, Deserialize)]
pub(crate) struct BookingForm {
    machine: String,
    period: String,
}

/// The withdrawal form: the machine whose booking goes.
#[derive(Debug, Deserialize)]
pub(crate) struct WithdrawalForm {
    machine: String,
}

/// `GET /mac`: the machines on offer, their prices, and the signed-in
/// account's bookings.
pub(crate) async fn mac(
    State(state): State<AppState>,
    ctx: PageContext,
    PageQuery(query): PageQuery<MacQuery>,
) -> Result<Response, AppError> {
    let bookings = match &ctx.account {
        Some(account) => {
            let id = account.id;
            Some(
                state
                    .database()
                    .run(move |store| store.bookings_of(id))
                    .await?
                    .map_err(AppError::internal)?,
            )
        }
        None => None,
    };
    let t = ctx.t();
    render(&MacPage {
        notice: query.notice.map(|notice| match notice {
            Notice::Booked => t.rental.notice_booked,
            Notice::Withdrawn => t.rental.notice_withdrawn,
            Notice::MachineGone => t.rental.notice_gone,
        }),
        machines: MACHINES
            .iter()
            .map(|machine| MachineView::of(machine, t))
            .collect(),
        periods: PeriodView::all(t),
        bookings: bookings.map(|bookings| {
            bookings
                .iter()
                .map(|booking| BookingView::of(booking, t))
                .collect()
        }),
        chrome: ctx.chrome(Page::Mac),
    })
}

/// `POST /mac/bookings`: books the machine the form names for the period
/// it names, for the signed-in account.
pub(crate) async fn book(
    State(state): State<AppState>,
    headers: HeaderMap,
    VerifiedForm(form): VerifiedForm<BookingForm>,
) -> Result<Response, AppError> {
    let Some(session) = presented_token(&headers) else {
        return Ok(login_first(Page::Mac));
    };
    // The page offers only these periods; any other is not its form.
    let period = Period::parse(&form.period)
        .ok_or(AppError::BadRequest("the period is not one on offer"))?;
    // A machine may have left the offer since the page was shown.
    let Some(machine) = Machine::find(&form.machine) else {
        return Ok(see_other(Notice::MachineGone.location()));
    };
    let terms = BookingTerms::of(machine, period);
    let session = session.digest();
    let now = state.now()?;
    let booked = state
        .database()
        .run(move |store| store.book(&session, &terms, now))
        .await?;
    finish(booked, Notice::Booked)
}

/// `POST /mac/bookings/withdraw`: withdraws the signed-in account's
/// booking of the machine the form names, if it has one.
pub(crate) async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    VerifiedForm(form): VerifiedForm<WithdrawalForm>,
) -> Result<Response, AppError> {
    let Some(session) = presented_token(&headers) else {
        return Ok(login_first(Page::Mac));
    };
    let machine =
        MachineId::parse(&form.machine).ok_or(AppError::BadRequest("the machine is not an id"))?;
    let session = session.digest();
    let now = state.now()?;
    let withdrawn = state
        .database()
        .run(move |store| store.withdraw_booking(&session, &machine, now))
        .await?
        // Withdrawn already (a second click), it is gone all the same.
        .map(|_| ());
    finish(withdrawn, Notice::Withdrawn)
}

/// The page with `notice` once a form has done its work; the login page
/// if its session ended while it was on its way.
fn finish(outcome: Result<(), BookingError>, notice: Notice) -> Result<Response, AppError> {
    match outcome {
        Ok(()) => Ok(see_other(notice.location())),
        Err(BookingError::SignedOut) => Ok(login_first(Page::Mac)),
        Err(BookingError::Store(error)) => Err(AppError::internal(error)),
    }
}
