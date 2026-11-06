//! CFM profiles, one per bound Mooring user, imported from bundles.
//!
//! A bundle is the complete set of profiles Mooring may deliver, so an
//! import replaces every stored profile with the bundle's profiles of bound
//! users: a user Mooring stops listing (revoked, say, or not yet agreed to
//! the current user agreement) stops being served. A bundle from the
//! future, or older than the last one imported, is refused whole.

use std::collections::BTreeSet;

use portal_core::account::AccountCode;
use portal_core::profile::{CfmProfile, ProfileBundle};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::rows::{code_from_sql, corrupt, time_from_sql, time_to_sql, user_from_sql};
use crate::subscriptions::MAX_CLOCK_SKEW_SECS;
use crate::{AccountId, Store, StoreError};

/// What a profile import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileImportReport {
    /// The bound users whose profiles were stored, in id order.
    pub stored: Vec<MooringUserId>,
    /// How many users of the bundle no account is bound to; their profiles
    /// were not kept.
    pub unbound: usize,
    /// Bound accounts whose Mooring user the bundle does not list, with that
    /// user: they now have no profile, and their links serve nothing.
    pub missing: Vec<(AccountCode, MooringUserId)>,
}

/// Why a bundle was not imported.
#[derive(Debug, thiserror::Error)]
pub enum ProfileImportError {
    /// Generated later than this host's clock allows.
    #[error(
        "the bundle was generated at {generated_at}, more than {MAX_CLOCK_SKEW_SECS} s after \
         this host's clock ({now}); check both clocks"
    )]
    FromTheFuture {
        /// When the bundle says it was generated.
        generated_at: UnixTime,
        /// This host's time.
        now: UnixTime,
    },
    /// Older than the last bundle imported.
    #[error(
        "the last bundle imported was generated at {stored}, after this one ({incoming}); \
         import a newer bundle"
    )]
    Older {
        /// When the last imported bundle was generated.
        stored: UnixTime,
        /// When the refused bundle was generated.
        incoming: UnixTime,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ProfileImportError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// A bound account's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProfile {
    /// The document CFM imports.
    pub profile: CfmProfile,
    /// When Mooring generated the bundle it came in.
    pub generated_at: UnixTime,
}

impl Store {
    /// Replaces the stored profiles with those of the bound users in
    /// `bundle`.
    pub fn import_profiles(
        &mut self,
        bundle: &ProfileBundle,
        now: UnixTime,
    ) -> Result<ProfileImportReport, ProfileImportError> {
        let latest_allowed = now
            .checked_add_secs(MAX_CLOCK_SKEW_SECS)
            .unwrap_or(UnixTime::MAX);
        if bundle.generated_at() > latest_allowed {
            return Err(ProfileImportError::FromTheFuture {
                generated_at: bundle.generated_at(),
                now,
            });
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let last: Option<i64> = tx
            .query_row(
                "SELECT generated_at FROM profile_bundle WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(last) = last {
            let stored = time_from_sql(last, "profile bundle time")?;
            if stored > bundle.generated_at() {
                return Err(ProfileImportError::Older {
                    stored,
                    incoming: bundle.generated_at(),
                });
            }
        }
        tx.execute("DELETE FROM cfm_profiles", [])?;
        let mut stored = Vec::new();
        let mut unbound = 0;
        for (user, profile) in bundle.profiles() {
            let bound: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM bindings WHERE mooring_user = ?1)",
                [user.as_str()],
                |row| row.get(0),
            )?;
            if !bound {
                unbound += 1;
                continue;
            }
            tx.execute(
                "INSERT INTO cfm_profiles (mooring_user, document) VALUES (?1, ?2)",
                params![user.as_str(), profile.as_str()],
            )?;
            stored.push(user.clone());
        }
        tx.execute(
            "INSERT INTO profile_bundle (id, generated_at) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET generated_at = excluded.generated_at",
            [time_to_sql(bundle.generated_at())],
        )?;
        let listed: BTreeSet<&MooringUserId> = bundle.profiles().map(|(user, _)| user).collect();
        let mut statement = tx.prepare(
            "SELECT a.code, b.mooring_user FROM bindings b JOIN accounts a ON a.id = b.account_id
             ORDER BY b.mooring_user",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut missing = Vec::new();
        for row in rows {
            let (code, user) = row?;
            let user = user_from_sql(&user)?;
            if !listed.contains(&user) {
                missing.push((code_from_sql(&code)?, user));
            }
        }
        drop(statement);
        tx.commit()?;
        Ok(ProfileImportReport {
            stored,
            unbound,
            missing,
        })
    }

    /// The profile of `account`'s Mooring user, if the account is bound and
    /// the last bundle listed the user.
    pub fn cfm_profile(&self, account: AccountId) -> Result<Option<StoredProfile>, StoreError> {
        let found: Option<(String, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT p.document, pb.generated_at FROM bindings b
                 JOIN cfm_profiles p ON p.mooring_user = b.mooring_user
                 LEFT JOIN profile_bundle pb ON pb.id = 1
                 WHERE b.account_id = ?1",
                [account.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((document, generated_at)) = found else {
            return Ok(None);
        };
        // An import writes the bundle's time with its profiles.
        let generated_at = generated_at.ok_or_else(|| {
            corrupt(
                "profile bundle time",
                "a profile is stored without it".to_owned(),
            )
        })?;
        Ok(Some(StoredProfile {
            profile: stored_profile(&document)?,
            generated_at: time_from_sql(generated_at, "profile bundle time")?,
        }))
    }
}

/// A stored document read back, checked by the rules an import applies.
pub(crate) fn stored_profile(document: &str) -> Result<CfmProfile, StoreError> {
    CfmProfile::parse(document).map_err(|error| corrupt("CFM profile", error.to_string()))
}
