//! The operator's commands on Mac rental bookings (`docs/rentals.md`):
//! what waits, and closing a booking once it is dealt with.

use std::io::Write;

use portal_core::payment::Currency;
use portal_core::rental::BookingTerms;
use portal_store::Store;

use crate::CommandError;
use crate::operator::erase_then_report;
use crate::payments::amount;

/// Lists every booking, oldest first, with the account that made it and
/// the address to write to its owner at.
pub(crate) fn bookings(store: &Store, out: &mut dyn Write) -> Result<(), CommandError> {
    let bookings = store.bookings()?;
    writeln!(
        out,
        "{:<6}  {:<20}  {:<14}  {:<18}  {:<7}  {:<11}  {:<11}  EMAIL",
        "NUMBER", "BOOKED", "ACCOUNT", "MACHINE", "PERIOD", "PRICE", "FIRST"
    )?;
    for listed in &bookings {
        let terms = &listed.booking.terms;
        writeln!(
            out,
            "{:<6}  {:<20}  {:<14}  {:<18}  {:<7}  {:<11}  {:<11}  {}",
            listed.booking.id,
            listed.booking.placed_at.to_string(),
            listed.account.to_string(),
            machine(terms),
            terms.period.as_str(),
            amount(terms.price, Currency::Gbp),
            amount(terms.first_price, Currency::Gbp),
            listed.email.as_str()
        )?;
    }
    writeln!(out, "{} booking(s)", bookings.len())?;
    if !bookings.is_empty() {
        writeln!(
            out,
            "once one is dealt with (its machine is ordered, or its owner has said no), close it \
             with `close-booking --id <number>`"
        )?;
    }
    Ok(())
}

/// Closes a booking that is dealt with: deletes it, and erases it from
/// the database files.
pub(crate) fn close_booking(
    store: &mut Store,
    id: u64,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let Some(closed) = store.close_booking(id)? else {
        return Err(CommandError::NoBooking(id));
    };
    erase_then_report(store, out, |out| {
        writeln!(
            out,
            "closed booking #{id}: {} by the {}, for account {}",
            machine(&closed.booking.terms),
            closed.booking.terms.period.as_str(),
            closed.account
        )
    })
}

/// The machine a booking asks for: `M6 24GB/512GB`.
fn machine(terms: &BookingTerms) -> String {
    format!(
        "{} {}GB/{}GB",
        terms.chip, terms.memory_gb, terms.storage_gb
    )
}
