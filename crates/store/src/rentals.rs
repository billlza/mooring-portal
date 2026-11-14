//! Mac rental bookings (`docs/rentals.md`): what a signed-in account asks
//! the operator for.
//!
//! A booking is made for the account of a live session, read in the
//! writing transaction, on the terms the offer had then. An account holds
//! one booking per machine. Booking the machine again on the same terms
//! leaves the booking as it was; on other terms it replaces it under a new
//! number, so the operator never closes, by the number they listed, a
//! booking that changed since. A booking goes when its owner withdraws it,
//! when the operator closes it, and with its account.

use portal_core::account::{AccountCode, Email};
use portal_core::auth::TokenDigest;
use portal_core::rental::{BookingTerms, ChipName, MachineId, Period};
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension as _, Row, Transaction, TransactionBehavior, params};

use crate::rows::{
    code_from_sql, corrupt, count_from_sql, count_to_sql, email_from_sql, time_from_sql,
    time_to_sql,
};
use crate::sessions::session_account_id;
use crate::{AccountId, Store, StoreError};

/// A booking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Booking {
    /// Its number, which the operator closes it by.
    pub id: u64,
    /// What it asks for.
    pub terms: BookingTerms,
    /// When it was made.
    pub placed_at: UnixTime,
}

/// A booking with the account that made it, as the operator lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorBooking {
    /// The booking.
    pub booking: Booking,
    /// Its account's code.
    pub account: AccountCode,
    /// Its account's email address, where the operator writes to settle
    /// it.
    pub email: Email,
}

/// Why a booking was not made or withdrawn.
#[derive(Debug, thiserror::Error)]
pub enum BookingError {
    /// No live session has the digest.
    #[error("the session has ended")]
    SignedOut,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for BookingError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// The columns [`booking_row`] reads, of `mac_bookings b`.
const BOOKING_COLUMNS: &str = "b.id, b.machine, b.chip, b.memory_gb, b.storage_gb, b.period,
     b.price, b.first_price, b.placed_at";
/// How many columns [`BOOKING_COLUMNS`] names: what follows them in a
/// query starts here.
const BOOKING_WIDTH: usize = 9;

/// A booking's row as SQLite gives it.
struct BookingRow {
    id: i64,
    machine: String,
    chip: String,
    memory_gb: i64,
    storage_gb: i64,
    period: String,
    price: i64,
    first_price: i64,
    placed_at: i64,
}

/// Reads the columns of [`BOOKING_COLUMNS`].
fn booking_row(row: &Row<'_>) -> rusqlite::Result<BookingRow> {
    Ok(BookingRow {
        id: row.get(0)?,
        machine: row.get(1)?,
        chip: row.get(2)?,
        memory_gb: row.get(3)?,
        storage_gb: row.get(4)?,
        period: row.get(5)?,
        price: row.get(6)?,
        first_price: row.get(7)?,
        placed_at: row.get(8)?,
    })
}

impl BookingRow {
    /// The booking, checked.
    fn checked(self) -> Result<Booking, StoreError> {
        let invalid = |what: &'static str, text: &str| corrupt(what, format!("{text:?}"));
        let gigabytes = |count: i64, what: &'static str| {
            u16::try_from(count).map_err(|_| invalid(what, &count.to_string()))
        };
        Ok(Booking {
            id: count_from_sql(self.id, "booking's number")?,
            terms: BookingTerms {
                machine: MachineId::parse(&self.machine)
                    .ok_or_else(|| invalid("booking's machine", &self.machine))?,
                chip: ChipName::parse(&self.chip)
                    .ok_or_else(|| invalid("booking's chip", &self.chip))?,
                memory_gb: gigabytes(self.memory_gb, "booking's memory")?,
                storage_gb: gigabytes(self.storage_gb, "booking's storage")?,
                period: Period::parse(&self.period)
                    .ok_or_else(|| invalid("booking's period", &self.period))?,
                price: count_from_sql(self.price, "booking's price")?,
                first_price: count_from_sql(self.first_price, "booking's first price")?,
            },
            placed_at: time_from_sql(self.placed_at, "booking's time")?,
        })
    }
}

/// The booking `account` holds of `machine`, read on `conn`.
fn held(
    conn: &Connection,
    account: AccountId,
    machine: &MachineId,
) -> Result<Option<Booking>, StoreError> {
    conn.query_row(
        &format!(
            "SELECT {BOOKING_COLUMNS} FROM mac_bookings b
             WHERE b.account_id = ?1 AND b.machine = ?2"
        ),
        params![account.0, machine.as_str()],
        booking_row,
    )
    .optional()?
    .map(BookingRow::checked)
    .transpose()
}

impl Store {
    /// Books on `terms` for the account signed in with the session whose
    /// token has `session`. The account's booking of the same machine is
    /// left as it was if it is on the same terms, and replaced if not.
    pub fn book(
        &mut self,
        session: &TokenDigest,
        terms: &BookingTerms,
        now: UnixTime,
    ) -> Result<(), BookingError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Err(BookingError::SignedOut);
        };
        if let Some(booking) = held(&tx, account, &terms.machine)? {
            if booking.terms == *terms {
                return Ok(());
            }
            tx.execute(
                "DELETE FROM mac_bookings WHERE id = ?1",
                [count_to_sql(booking.id, "booking's number")?],
            )?;
        }
        tx.execute(
            "INSERT INTO mac_bookings (account_id, machine, chip, memory_gb, storage_gb, period,
                 price, first_price, placed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                account.0,
                terms.machine.as_str(),
                terms.chip.as_str(),
                terms.memory_gb,
                terms.storage_gb,
                terms.period.as_str(),
                count_to_sql(terms.price, "booking's price")?,
                count_to_sql(terms.first_price, "booking's first price")?,
                time_to_sql(now),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Withdraws the booking of `machine` of the account signed in with
    /// the session whose token has `session`; whether it had one.
    pub fn withdraw_booking(
        &mut self,
        session: &TokenDigest,
        machine: &MachineId,
        now: UnixTime,
    ) -> Result<bool, BookingError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Err(BookingError::SignedOut);
        };
        let removed = tx.execute(
            "DELETE FROM mac_bookings WHERE account_id = ?1 AND machine = ?2",
            params![account.0, machine.as_str()],
        )?;
        tx.commit()?;
        Ok(removed > 0)
    }

    /// The bookings of `account`, oldest first.
    pub fn bookings_of(&self, account: AccountId) -> Result<Vec<Booking>, StoreError> {
        let rows: Vec<BookingRow> = self
            .conn
            .prepare(&format!(
                "SELECT {BOOKING_COLUMNS} FROM mac_bookings b WHERE b.account_id = ?1
                 ORDER BY b.placed_at, b.id"
            ))?
            .query_map([account.0], booking_row)?
            .collect::<Result<_, _>>()?;
        rows.into_iter().map(BookingRow::checked).collect()
    }

    /// Every booking with its account, oldest first: what waits for the
    /// operator.
    pub fn bookings(&self) -> Result<Vec<OperatorBooking>, StoreError> {
        let rows: Vec<(BookingRow, String, String)> = self
            .conn
            .prepare(&format!(
                "SELECT {BOOKING_COLUMNS}, a.code, a.email
                 FROM mac_bookings b JOIN accounts a ON a.id = b.account_id
                 ORDER BY b.placed_at, b.id"
            ))?
            .query_map([], operator_row)?
            .collect::<Result<_, _>>()?;
        rows.into_iter().map(operator_booking).collect()
    }

    /// Closes booking `id` once the operator has dealt with it: deletes it,
    /// and gives what it was, if there was one. Copies of the row stay in
    /// the database files until [`Store::erase_deleted`].
    pub fn close_booking(&mut self, id: u64) -> Result<Option<OperatorBooking>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let closed = close_booking_on(&tx, id)?;
        tx.commit()?;
        Ok(closed)
    }
}

/// Closes booking `id` on `tx`, as [`Store::close_booking`] does.
pub(crate) fn close_booking_on(
    tx: &Transaction<'_>,
    id: u64,
) -> Result<Option<OperatorBooking>, StoreError> {
    // A number no row can have names no booking.
    let Ok(id) = i64::try_from(id) else {
        return Ok(None);
    };
    let found = tx
        .query_row(
            &format!(
                "SELECT {BOOKING_COLUMNS}, a.code, a.email
                 FROM mac_bookings b JOIN accounts a ON a.id = b.account_id
                 WHERE b.id = ?1"
            ),
            [id],
            operator_row,
        )
        .optional()?;
    let Some(found) = found else {
        return Ok(None);
    };
    let closed = operator_booking(found)?;
    tx.execute("DELETE FROM mac_bookings WHERE id = ?1", [id])?;
    Ok(Some(closed))
}

/// Reads [`BOOKING_COLUMNS`] and the account's code and address after
/// them.
fn operator_row(row: &Row<'_>) -> rusqlite::Result<(BookingRow, String, String)> {
    Ok((
        booking_row(row)?,
        row.get(BOOKING_WIDTH)?,
        row.get(BOOKING_WIDTH + 1)?,
    ))
}

/// What [`operator_row`] read, checked.
fn operator_booking(
    (row, code, email): (BookingRow, String, String),
) -> Result<OperatorBooking, StoreError> {
    Ok(OperatorBooking {
        booking: row.checked()?,
        account: code_from_sql(&code)?,
        email: email_from_sql(&email)?,
    })
}
