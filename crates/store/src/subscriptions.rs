//! Imported snapshot records, one per bound Mooring user.
//!
//! An import keeps only the records of users bound to an account: the
//! portal has no use for anyone else's figures, so it does not hold them.
//! It refuses a snapshot from the future (beyond a small clock skew) and
//! one older than a record it already holds, as a whole: either every
//! record of the snapshot is stored or none is.

use std::collections::BTreeSet;

use portal_core::snapshot::{MooringUserId, Snapshot, SnapshotError, UserSnapshot};
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::bindings::{Binding, BoundAccount, binding_row, bound_accounts};
use crate::rows::corrupt;
use crate::{AccountId, Store, StoreError};

/// How far a snapshot's generation time may lie ahead of this host's
/// clock: the operator's Mac and the portal's host keep their own time.
pub const MAX_CLOCK_SKEW_SECS: u64 = 300;

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// The bound users whose records were stored, in id order.
    pub stored: Vec<MooringUserId>,
    /// How many users of the snapshot no account is bound to; their
    /// records were not kept.
    pub unbound: usize,
    /// Bound accounts whose Mooring user the snapshot does not list, in user
    /// id order: their pages keep showing their previous record, if any.
    pub missing: Vec<BoundAccount>,
}

/// Why a snapshot was not imported.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Generated later than this host's clock allows.
    #[error(
        "the snapshot was generated at {generated_at}, more than {MAX_CLOCK_SKEW_SECS} s after \
         this host's clock ({now}); check both clocks"
    )]
    FromTheFuture {
        /// When the snapshot says it was generated.
        generated_at: UnixTime,
        /// This host's time.
        now: UnixTime,
    },
    /// Older than a record already stored.
    #[error(
        "the stored record of Mooring user {user} was generated at {stored}, after this \
         snapshot ({incoming}); import a newer snapshot"
    )]
    Older {
        /// The user.
        user: MooringUserId,
        /// When the stored record was generated.
        stored: UnixTime,
        /// When the refused snapshot was generated.
        incoming: UnixTime,
    },
    /// A record could not be encoded.
    #[error(transparent)]
    Encode(#[from] SnapshotError),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ImportError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// An account's subscription, as the subscription page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subscription {
    /// The operator has not bound the account to a Mooring user.
    Unbound,
    /// The account is bound.
    Bound {
        /// To whom, since when.
        binding: Binding,
        /// The latest imported record; `None` until an import lists the user.
        record: Option<UserSnapshot>,
    },
}

impl Store {
    /// Stores the records of the bound users in `snapshot`.
    pub fn import(
        &mut self,
        snapshot: &Snapshot,
        now: UnixTime,
    ) -> Result<ImportReport, ImportError> {
        let latest_allowed = now
            .checked_add_secs(MAX_CLOCK_SKEW_SECS)
            .unwrap_or(UnixTime::MAX);
        if snapshot.generated_at() > latest_allowed {
            return Err(ImportError::FromTheFuture {
                generated_at: snapshot.generated_at(),
                now,
            });
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let bound = bound_accounts(&tx)?;
        let mut stored = Vec::new();
        let mut unbound = 0;
        for part in snapshot.per_user() {
            if !bound.contains_key(part.user()) {
                unbound += 1;
                continue;
            }
            let existing: Option<String> = tx
                .query_row(
                    "SELECT document FROM subscription_records WHERE mooring_user = ?1",
                    [part.user().as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                let existing = stored_record(&existing)?;
                if existing.generated_at() > part.generated_at() {
                    return Err(ImportError::Older {
                        user: part.user().clone(),
                        stored: existing.generated_at(),
                        incoming: part.generated_at(),
                    });
                }
            }
            tx.execute(
                "INSERT INTO subscription_records (mooring_user, document) VALUES (?1, ?2)
                 ON CONFLICT (mooring_user) DO UPDATE SET document = excluded.document",
                params![part.user().as_str(), part.to_document()?],
            )?;
            stored.push(part.user().clone());
        }
        let listed: BTreeSet<&MooringUserId> = snapshot.users().map(|(user, _)| user).collect();
        let missing = bound
            .into_values()
            .map(|(_, account)| account)
            .filter(|account| !listed.contains(&account.user))
            .collect();
        tx.commit()?;
        Ok(ImportReport {
            stored,
            unbound,
            missing,
        })
    }

    /// The subscription of `account`, read in one statement.
    pub fn subscription(&self, account: AccountId) -> Result<Subscription, StoreError> {
        let found: Option<(String, i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT b.mooring_user, b.bound_at, r.document FROM bindings b
                 LEFT JOIN subscription_records r ON r.mooring_user = b.mooring_user
                 WHERE b.account_id = ?1",
                [account.0],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((user, bound_at, document)) = found else {
            return Ok(Subscription::Unbound);
        };
        let binding = binding_row(&user, bound_at)?;
        let record = document.as_deref().map(stored_record).transpose()?;
        if let Some(record) = &record
            && record.user() != &binding.user
        {
            return Err(corrupt(
                "subscription record",
                format!(
                    "the record of {} is filed under {}",
                    record.user(),
                    binding.user
                ),
            ));
        }
        Ok(Subscription::Bound { binding, record })
    }
}

fn stored_record(document: &str) -> Result<UserSnapshot, StoreError> {
    UserSnapshot::from_document(document)
        .map_err(|error| corrupt("subscription record", error.to_string()))
}
