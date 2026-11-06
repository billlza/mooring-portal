//! CFM profiles, one per bound Mooring user, imported from bundles.
//!
//! The operator names every bound account's Mooring user when exporting a
//! bundle, and Mooring delivers a profile to each named user or withholds
//! the user (paused, revoked, or not yet agreed to the current user
//! agreement). An import replaces every stored profile with the bundle's
//! profiles of bound users. A withheld user stops being served, and the
//! account's subscription link ends with the profile, so that delivering
//! to the user again does not bring an old link back.
//!
//! A bundle that does not name a bound account's user at all is refused
//! whole: that is a slip in the operator's list, not a decision to stop
//! serving the account, so it must not end the account's link. A bundle
//! from the future, or older than the last one imported, is refused whole
//! too.

use portal_core::profile::{CfmProfile, ProfileBundle, WithheldReason};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::bindings::{BoundAccount, bound_accounts};
use crate::rows::{corrupt, time_from_sql, time_to_sql};
use crate::subscriptions::MAX_CLOCK_SKEW_SECS;
use crate::{AccountId, Store, StoreError};

/// What a profile import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileImportReport {
    /// The bound users whose profiles were stored, in id order.
    pub stored: Vec<MooringUserId>,
    /// Bound accounts whose Mooring user the bundle withholds, in user id
    /// order.
    pub withheld: Vec<Withheld>,
    /// Users the bundle delivers a profile to that no account is bound to,
    /// in id order: their profiles were not kept, but their credentials
    /// passed through this host.
    pub unbound_profiles: Vec<MooringUserId>,
    /// Users the bundle withholds that no account is bound to, in id order.
    pub unbound_withheld: Vec<MooringUserId>,
}

/// A bound account whose Mooring user a bundle withholds: it has no
/// profile now, and no subscription link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withheld {
    /// The account and its Mooring user.
    pub account: BoundAccount,
    /// Why Mooring withholds the user.
    pub reason: WithheldReason,
    /// It had a link, which the import ended for good.
    pub link_revoked: bool,
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
    /// The bundle neither delivers to nor withholds the Mooring users of
    /// some bound accounts: most likely the operator left them out of the
    /// export. Nothing was imported.
    #[error(
        "the bundle does not name the Mooring user of {}; nothing was imported. Export the \
         bundle again naming every bound account's user. Unbind an account only if it is not \
         to be served any more: unbinding deletes its subscription link",
        list(.accounts)
    )]
    LeavesOut {
        /// The accounts left out, in user id order; never empty.
        accounts: Vec<BoundAccount>,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `accounts`, comma-separated.
fn list(accounts: &[BoundAccount]) -> String {
    let named: Vec<String> = accounts.iter().map(BoundAccount::to_string).collect();
    named.join(", ")
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
    /// `bundle`, which must name every bound account's Mooring user.
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
        let bound = bound_accounts(&tx)?;
        let left_out: Vec<BoundAccount> = bound
            .values()
            .filter(|(_, account)| !bundle.names(&account.user))
            .map(|(_, account)| account.clone())
            .collect();
        if !left_out.is_empty() {
            return Err(ProfileImportError::LeavesOut { accounts: left_out });
        }
        tx.execute("DELETE FROM cfm_profiles", [])?;
        let mut stored = Vec::new();
        let mut unbound_profiles = Vec::new();
        for (user, profile) in bundle.profiles() {
            if !bound.contains_key(user) {
                unbound_profiles.push(user.clone());
                continue;
            }
            tx.execute(
                "INSERT INTO cfm_profiles (mooring_user, document) VALUES (?1, ?2)",
                params![user.as_str(), profile.as_str()],
            )?;
            stored.push(user.clone());
        }
        let mut withheld = Vec::new();
        let mut unbound_withheld = Vec::new();
        for (user, reason) in bundle.withheld() {
            let Some((id, account)) = bound.get(user) else {
                unbound_withheld.push(user.clone());
                continue;
            };
            let links = tx.execute(
                "DELETE FROM subscription_links WHERE account_id = ?1",
                [id.0],
            )?;
            withheld.push(Withheld {
                account: account.clone(),
                reason,
                link_revoked: links > 0,
            });
        }
        tx.execute(
            "INSERT INTO profile_bundle (id, generated_at) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET generated_at = excluded.generated_at",
            [time_to_sql(bundle.generated_at())],
        )?;
        tx.commit()?;
        Ok(ProfileImportReport {
            stored,
            withheld,
            unbound_profiles,
            unbound_withheld,
        })
    }

    /// The profile of `account`'s Mooring user, if the account is bound and
    /// the last bundle delivered a profile to the user.
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
