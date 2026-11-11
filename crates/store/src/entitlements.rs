//! The journal Mooring pulls, and the token it pulls with.
//!
//! Mooring presents a bearer token, of which the portal keeps the digest,
//! and a cursor: the number of the last entry it has acted on. The portal
//! remembers the furthest cursor presented, which is how it knows what
//! Mooring has: an account's orders may be deleted only once Mooring has
//! them (`accounts.rs`). A cursor beyond the last number the journal ever
//! gave out can only come from a journal that is not this one (a database
//! restored from an older copy, say), and is refused rather than answered
//! with nothing, which Mooring would take for "nothing new".
//!
//! Whether the entries were paid with real money is the database's to say
//! (`payment_mode.rs`), in the same transaction that reads them.

use portal_core::auth::TokenDigest;
use portal_core::payment::feed::{Entry, EntryKind};
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::orders::terms_row;
use crate::payment_mode::mode;
use crate::rows::{
    corrupt, count_from_sql, count_to_sql, order_from_sql, time_from_sql, time_to_sql,
    user_from_sql,
};
use crate::{Store, StoreError};

/// Entries after a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pulled {
    /// The entries, by number.
    pub entries: Vec<Entry>,
    /// Whether the journal holds entries after these.
    pub more: bool,
    /// Whether the journal's orders were paid with real money.
    pub livemode: bool,
}

/// How things stand between the journal and Mooring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullState {
    /// When the token Mooring pulls with was made, if there is one.
    pub token_created_at: Option<UnixTime>,
    /// The furthest cursor Mooring has presented: it has every entry up
    /// to this number.
    pub pulled_through: u64,
    /// When Mooring last pulled.
    pub pulled_at: Option<UnixTime>,
    /// The last number the journal has given out.
    pub last_seq: u64,
}

/// Why a pull was not answered.
#[derive(Debug, thiserror::Error)]
pub enum PullError {
    /// No token is set, or the one presented is not it.
    #[error("the token is not the one Mooring pulls with")]
    Unauthorized,
    /// The database has entered no payment mode: it has never been served
    /// with payments, so nothing in it is Mooring's to pull.
    #[error("the database has never been served with payments")]
    NoMode,
    /// The cursor is beyond the last number the journal has given out.
    #[error(
        "the cursor {after} is beyond the journal's last entry, {last_seq}: this is not the \
         journal the cursor was read from"
    )]
    Ahead {
        /// The cursor presented.
        after: u64,
        /// The last number the journal has given out.
        last_seq: u64,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for PullError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// The last number the journal has given out; deleting entries does not
/// lower it.
fn last_seq(conn: &rusqlite::Connection) -> Result<u64, StoreError> {
    let last: Option<i64> = conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'entitlements'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    count_from_sql(last.unwrap_or(0), "journal's last number")
}

/// Whether the token whose digest is `token` is the one Mooring pulls
/// with.
fn holds_token(conn: &rusqlite::Connection, token: &TokenDigest) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pull_access WHERE id = 1 AND token_digest = ?1)",
        [token.as_bytes()],
        |row| row.get(0),
    )?)
}

impl Store {
    /// Whether the token whose digest is `token` is the one Mooring pulls
    /// with: for a request that cannot be answered, to tell a stranger's
    /// from one of Mooring's that is malformed.
    pub fn is_pull_token(&self, token: &TokenDigest) -> Result<bool, StoreError> {
        holds_token(&self.conn, token)
    }

    /// Makes the token whose digest is `token` the one Mooring pulls
    /// with, in place of any before it. What Mooring has pulled stands.
    pub fn set_pull_token(&mut self, token: &TokenDigest, now: UnixTime) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO pull_access (id, token_digest, token_created_at) VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET token_digest = excluded.token_digest,
                 token_created_at = excluded.token_created_at",
            params![token.as_bytes(), time_to_sql(now)],
        )?;
        Ok(())
    }

    /// Removes the token Mooring pulls with; whether there was one.
    pub fn revoke_pull_token(&mut self) -> Result<bool, StoreError> {
        let removed = self.conn.execute(
            "UPDATE pull_access SET token_digest = NULL, token_created_at = NULL
             WHERE id = 1 AND token_digest IS NOT NULL",
            [],
        )?;
        Ok(removed > 0)
    }

    /// How things stand between the journal and Mooring.
    pub fn pull_state(&self) -> Result<PullState, StoreError> {
        let row: Option<(Option<i64>, i64, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT token_created_at, pulled_through, pulled_at FROM pull_access
                 WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (token_created_at, pulled_through, pulled_at) = row.unwrap_or((None, 0, None));
        Ok(PullState {
            token_created_at: token_created_at
                .map(|secs| time_from_sql(secs, "pull token's time"))
                .transpose()?,
            pulled_through: count_from_sql(pulled_through, "pull cursor")?,
            pulled_at: pulled_at
                .map(|secs| time_from_sql(secs, "last pull's time"))
                .transpose()?,
            last_seq: last_seq(&self.conn)?,
        })
    }

    /// The first `limit` entries after `after`, for the holder of the
    /// token whose digest is `token`; and notes that Mooring has every
    /// entry up to `after`.
    pub fn pull_entitlements(
        &mut self,
        token: &TokenDigest,
        after: u64,
        limit: u32,
        now: UnixTime,
    ) -> Result<Pulled, PullError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !holds_token(&tx, token)? {
            return Err(PullError::Unauthorized);
        }
        let Some(livemode) = mode(&tx)? else {
            return Err(PullError::NoMode);
        };
        let last_seq = last_seq(&tx)?;
        if after > last_seq {
            return Err(PullError::Ahead { after, last_seq });
        }
        let after_sql = count_to_sql(after, "pull cursor")?;
        tx.execute(
            "UPDATE pull_access SET pulled_through = max(pulled_through, ?1), pulled_at = ?2
             WHERE id = 1",
            params![after_sql, time_to_sql(now)],
        )?;
        // One more than asked for tells whether there are more.
        let wanted = i64::from(limit) + 1;
        let rows: Vec<EntryRow> = tx
            .prepare(
                "SELECT e.seq, e.kind, e.amount, e.in_full, e.recorded_at, o.id, o.mooring_user,
                        o.paid_at, o.renewal_of, o.plan, o.plan_kind, o.days, o.traffic_bytes,
                        o.amount, o.currency, o.payment_link_id
                 FROM entitlements e JOIN orders o ON o.id = e.order_id
                 WHERE e.seq > ?1 ORDER BY e.seq LIMIT ?2",
            )?
            .query_map(params![after_sql, wanted], |row| {
                Ok(EntryRow {
                    seq: row.get(0)?,
                    kind: row.get(1)?,
                    amount: row.get(2)?,
                    in_full: row.get(3)?,
                    recorded_at: row.get(4)?,
                    order: row.get(5)?,
                    user: row.get(6)?,
                    paid_at: row.get(7)?,
                    renewal_of: row.get(8)?,
                    terms: terms_row(row, 9)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        tx.commit()?;
        let limit = limit as usize;
        let more = rows.len() > limit;
        let entries = rows
            .into_iter()
            .take(limit)
            .map(EntryRow::checked)
            .collect::<Result<_, _>>()?;
        Ok(Pulled {
            entries,
            more,
            livemode,
        })
    }
}

/// An entry's row as SQLite gives it.
struct EntryRow {
    seq: i64,
    kind: String,
    amount: Option<i64>,
    in_full: Option<bool>,
    recorded_at: i64,
    order: String,
    user: String,
    paid_at: Option<i64>,
    renewal_of: Option<String>,
    terms: crate::orders::TermsRow,
}

impl EntryRow {
    fn checked(self) -> Result<Entry, StoreError> {
        let amount = self
            .amount
            .map(|amount| count_from_sql(amount, "journal entry's amount"))
            .transpose()?;
        let kind = match (self.kind.as_str(), amount, self.in_full) {
            ("grant", None, None) => EntryKind::Grant,
            ("refund", Some(amount_refunded), Some(in_full)) => EntryKind::Refund {
                amount_refunded,
                in_full,
            },
            ("dispute", Some(amount_disputed), None) => EntryKind::Dispute { amount_disputed },
            (kind, ..) => return Err(corrupt("journal entry's kind", format!("{kind:?}"))),
        };
        // Only a paid order is ever entered.
        let paid_at = self
            .paid_at
            .ok_or_else(|| corrupt("journal entry", "its order has no payment time".to_owned()))?;
        Ok(Entry {
            seq: count_from_sql(self.seq, "journal entry's number")?,
            kind,
            order: order_from_sql(&self.order)?,
            user: user_from_sql(&self.user)?,
            terms: self.terms.checked()?,
            paid_at: time_from_sql(paid_at, "order's payment time")?,
            renewal_of: self.renewal_of.as_deref().map(order_from_sql).transpose()?,
            recorded_at: time_from_sql(self.recorded_at, "journal entry's time")?,
        })
    }
}
