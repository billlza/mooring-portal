//! Which money a database holds: Stripe's test mode, or real.
//!
//! Stripe keeps two worlds apart, test mode and live, and marks every
//! event with the one it is of. The journal Mooring pulls cannot hold
//! both: Mooring is told for each document whether to grant, so an entry
//! of a rehearsal among real ones would be granted as real. A database
//! therefore holds the payments of one mode and says which. The server
//! enters that mode when it starts with a catalogue; an event of the
//! other mode is refused before anything of it is recorded
//! (`stripe_events.rs`); and what a document tells Mooring of its mode is
//! read from here (`entitlements.rs`), never from the catalogue the
//! server happens to run with.
//!
//! The mode changes only while the database holds no order and no payment
//! that is no order's. A rehearsal's are removed whole, by the operator
//! ([`Store::discard_test_payments`]); real ones never are.

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};

use crate::orders::{ORDER_TABLES, RENEWAL_TABLES, RENEWS, STRAY_NEEDS_CANCELLATION, UNSETTLED};
use crate::rows::count_from_sql;
use crate::{Store, StoreError};

/// Why the database did not enter a payment mode.
#[derive(Debug, thiserror::Error)]
pub enum ModeError {
    /// It holds orders or payments of the other mode.
    #[error("{}", held(*.livemode))]
    Held {
        /// Whether what it holds was paid with real money.
        livemode: bool,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ModeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// What [`ModeError::Held`] says.
const fn held(livemode: bool) -> &'static str {
    if livemode {
        "the database holds orders and payments made with real money, and the catalogue is for \
         Stripe's test mode: rehearse with another database"
    } else {
        "the database holds orders and payments of Stripe's test mode, and the catalogue is for \
         real payments: remove them with `discard-test-payments` first"
    }
}

/// What [`Store::discard_test_payments`] removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discarded {
    /// Orders, with their entries in the journal.
    pub orders: u64,
    /// Payments that were no order's.
    pub stray_payments: u64,
}

/// Why nothing was discarded.
#[derive(Debug, thiserror::Error)]
pub enum DiscardError {
    /// The database is one of payments made with real money.
    #[error(
        "the database is for payments made with real money, which are never discarded; nothing \
         changed"
    )]
    Live,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for DiscardError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// Whether the payments the database holds were made with real money, if
/// it has entered a mode.
pub(crate) fn mode(conn: &Connection) -> Result<Option<bool>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT livemode FROM payment_mode WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

/// How many rows `table` holds.
fn rows(conn: &Connection, table: &str) -> Result<u64, StoreError> {
    let count: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })?;
    count_from_sql(count, "count of rows")
}

/// Forgets every event Stripe delivered and all it said of payments,
/// invoices and subscriptions.
fn forget_stripe(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "DELETE FROM stripe_events;
         DELETE FROM stripe_payments;
         DELETE FROM stripe_invoice_payments;
         DELETE FROM stripe_subscriptions;
         DELETE FROM intro_paid_intents;
         DELETE FROM intro_schedule_notices;
         DELETE FROM introductory_offers;",
    )?;
    Ok(())
}

impl Store {
    /// Makes the database hold the payments of one mode: real money if
    /// `livemode`, Stripe's test mode if not. A database that holds orders
    /// or stray payments of the other mode is left as it is; one that
    /// holds none changes over, and forgets what Stripe said in the
    /// other.
    pub fn enter_payment_mode(&mut self, livemode: bool) -> Result<(), ModeError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match mode(&tx)? {
            Some(entered) if entered == livemode => return Ok(()),
            Some(entered) => {
                let holds: bool = tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM orders)
                         OR EXISTS (SELECT 1 FROM stray_payments)
                         OR EXISTS (SELECT 1 FROM introductory_offers WHERE schedule_state = 'open')",
                    [],
                    |row| row.get(0),
                )?;
                if holds {
                    return Err(ModeError::Held { livemode: entered });
                }
                forget_stripe(&tx)?;
                tx.execute(
                    "UPDATE payment_mode SET livemode = ?1 WHERE id = 1",
                    [livemode],
                )?;
            }
            None => {
                tx.execute(
                    "INSERT INTO payment_mode (id, livemode) VALUES (1, ?1)",
                    [livemode],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Whether money is in the air: Stripe may charge a subscription
    /// again, or an order is not settled. Such a database is to be served
    /// with payments, or what Stripe says of them is heard by no one and
    /// what Mooring is owed is never fetched.
    pub fn payments_outstanding(&self) -> Result<bool, StoreError> {
        Ok(self.conn.query_row(
            &format!(
                "SELECT EXISTS (SELECT 1 FROM {RENEWAL_TABLES} WHERE {RENEWS})
                     OR EXISTS (SELECT 1 FROM introductory_offers WHERE schedule_state = 'open')
                     OR EXISTS (SELECT 1 FROM stray_payments sp WHERE {STRAY_NEEDS_CANCELLATION})
                     OR EXISTS (SELECT 1 FROM {ORDER_TABLES} WHERE {UNSETTLED})"
            ),
            [],
            |row| row.get(0),
        )?)
    }

    /// Removes a rehearsal: every order with its entries in the journal,
    /// every payment that is no order's, and all that Stripe said, of a
    /// database that has taken no real money. The journal's numbers are
    /// not given out again, and the token Mooring pulls with, what Mooring
    /// has pulled and the ban list stay. Copies of the rows stay in the
    /// database files until [`Store::erase_deleted`].
    pub fn discard_test_payments(&mut self) -> Result<Discarded, DiscardError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if mode(&tx)? == Some(true) {
            return Err(DiscardError::Live);
        }
        let discarded = Discarded {
            orders: rows(&tx, "orders")?,
            stray_payments: rows(&tx, "stray_payments")?,
        };
        tx.execute_batch("DELETE FROM orders; DELETE FROM stray_payments;")?;
        forget_stripe(&tx)?;
        tx.commit()?;
        Ok(discarded)
    }
}
