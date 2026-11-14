//! The Mac rental bookings, and closing one once it is dealt with
//! (`docs/rentals.md`). Closing deletes the booking, so it ends by erasing
//! it from the database files, as `close-booking` on the host does.

use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use portal_store::AsOperator;
use serde::Deserialize;

use super::{Confirmed, OperatorForm, Outcome, Section, chrome, turned_away};
use crate::error::AppError;
use crate::format::beijing;
use crate::i18n::fill;
use crate::pages::{Chrome, PageContext, render_status};
use crate::state::AppState;
use crate::view::BookingView;

#[derive(Template)]
#[template(path = "operator_bookings.html")]
struct BookingsPage {
    chrome: Chrome,
    section: Section,
    outcome: Outcome,
    bookings: Vec<BookingRow>,
}

/// A booking, as the list shows it.
struct BookingRow {
    id: u64,
    account: String,
    email: String,
    booking: BookingView,
    /// When it was made, in Beijing time.
    placed: String,
}

/// The closing form: the booking's number.
#[derive(Debug, Deserialize)]
pub(crate) struct CloseForm {
    id: u64,
}

/// `GET /operator/bookings`: every booking, oldest first.
pub(crate) async fn bookings(
    _: Confirmed,
    State(state): State<AppState>,
    ctx: PageContext,
) -> Result<Response, AppError> {
    page(&state, &ctx, Outcome::default(), StatusCode::OK).await
}

/// `POST /operator/bookings/close`: closes the booking with the number
/// the form names.
pub(crate) async fn close_booking(
    State(state): State<AppState>,
    ctx: PageContext,
    OperatorForm { gate, form }: OperatorForm<CloseForm>,
) -> Result<Response, AppError> {
    let t = ctx.t();
    let id = form.id;
    let now = state.now()?;
    let closed = state
        .database()
        .run(move |store| {
            store
                .as_operator(&gate.digest, now, |pages| pages.close_booking(id))
                .map(|closed| closed.map(|closed| closed.map(|_| store.erase_deleted())))
        })
        .await?
        .map_err(AppError::internal)?;
    let closed = match closed {
        AsOperator::Done(closed) => closed,
        AsOperator::NotAllowed(not) => return Ok(turned_away(not, Section::Bookings)),
    };
    let (outcome, status) = match closed {
        Some(erased) => {
            tracing::info!("the operator closed a Mac booking on the operator pages");
            (
                Outcome::done(fill(t.operator.closed_notice, id)).erased(t, erased),
                StatusCode::OK,
            )
        }
        // Closed already (a second click), or never made.
        None => (
            Outcome::refused(t, &fill(t.operator.no_booking, id)),
            StatusCode::CONFLICT,
        ),
    };
    page(&state, &ctx, outcome, status).await
}

/// The bookings page, after `outcome`.
async fn page(
    state: &AppState,
    ctx: &PageContext,
    outcome: Outcome,
    status: StatusCode,
) -> Result<Response, AppError> {
    let bookings = state
        .database()
        .run(|store| store.bookings())
        .await?
        .map_err(AppError::internal)?;
    let t = ctx.t();
    render_status(
        status,
        &BookingsPage {
            section: Section::Bookings,
            outcome,
            bookings: bookings
                .iter()
                .map(|listed| BookingRow {
                    id: listed.booking.id,
                    account: listed.account.to_string(),
                    email: listed.email.as_str().to_owned(),
                    booking: BookingView::of(&listed.booking, t),
                    placed: beijing(listed.booking.placed_at),
                })
                .collect(),
            chrome: chrome(ctx),
        },
    )
}
