//! Subscription links: at most one per bound account, stored by the digest
//! of its token.
//!
//! A link exists only while its account is bound: the table refers to the
//! binding, so unbinding the account or deleting it removes the link. A
//! link is made only for an account with a profile, in the same transaction
//! that checks for it. A new link replaces the old one, and changing the
//! password removes it
//! (`sessions.rs`), so a link made by whoever was signed in stops working
//! once the owner changes the password. Making and removing a link act for
//! the account of a session that is still live in the same transaction, not
//! for an account read earlier.

use portal_core::auth::TokenDigest;
use portal_core::profile::CfmProfile;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::profiles::stored_profile;
use crate::rows::{corrupt, time_from_sql, time_to_sql};
use crate::sessions::session_account_id;
use crate::{AccountId, Store, StoreError};

/// Why a link was not made.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// No live session has the digest: it ended (logout, password change,
    /// deletion of the account) after the request was let in.
    #[error("the session has ended")]
    SignedOut,
    /// The account is not bound to a Mooring user, so there is nothing a
    /// link could serve.
    #[error("the account is not bound to a Mooring user")]
    Unbound,
    /// The account's Mooring user has no profile, so a link would fetch
    /// nothing.
    #[error("the account's Mooring user has no CFM profile")]
    NoProfile,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for LinkError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// Why a link was not removed.
#[derive(Debug, thiserror::Error)]
pub enum RevokeError {
    /// No live session has the digest.
    #[error("the session has ended")]
    SignedOut,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for RevokeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// How an account gets its CFM profile, as the subscription page shows it:
/// read in one statement, so the profile and the link agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfmDelivery {
    /// The account has no profile, and so no link.
    NoProfile,
    /// The account has a profile.
    Profile {
        /// When Mooring generated the bundle it came in.
        generated_at: UnixTime,
        /// When the account's link was made, if it has one.
        link_created_at: Option<UnixTime>,
    },
}

/// What a link serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedProfile {
    /// No link has the digest.
    Unknown,
    /// The link's account has no profile: the last bundle did not list its
    /// Mooring user.
    NoProfile,
    /// The profile of the link's account.
    Profile(CfmProfile),
}

impl Store {
    /// Makes a link whose token has `link` for the account signed in with
    /// the session whose token has `session`, replacing any link it had.
    /// The account must be bound and its Mooring user have a profile.
    pub fn replace_subscription_link(
        &mut self,
        session: &TokenDigest,
        link: &TokenDigest,
        now: UnixTime,
    ) -> Result<(), LinkError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Err(LinkError::SignedOut);
        };
        let bound: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM bindings WHERE account_id = ?1)",
            [account.0],
            |row| row.get(0),
        )?;
        if !bound {
            return Err(LinkError::Unbound);
        }
        let profiled: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM bindings b
                 JOIN cfm_profiles p ON p.mooring_user = b.mooring_user
                 WHERE b.account_id = ?1)",
            [account.0],
            |row| row.get(0),
        )?;
        if !profiled {
            return Err(LinkError::NoProfile);
        }
        tx.execute(
            "INSERT INTO subscription_links (account_id, token_digest, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (account_id) DO UPDATE SET
                 token_digest = excluded.token_digest, created_at = excluded.created_at",
            params![account.0, link.as_bytes(), time_to_sql(now)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Removes the link of the account signed in with the session whose
    /// token has `session`; whether it had one.
    pub fn revoke_subscription_link(
        &mut self,
        session: &TokenDigest,
        now: UnixTime,
    ) -> Result<bool, RevokeError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Err(RevokeError::SignedOut);
        };
        let removed = tx.execute(
            "DELETE FROM subscription_links WHERE account_id = ?1",
            [account.0],
        )?;
        tx.commit()?;
        Ok(removed > 0)
    }

    /// How `account` gets its profile.
    pub fn cfm_delivery(&self, account: AccountId) -> Result<CfmDelivery, StoreError> {
        let found: Option<(Option<i64>, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT pb.generated_at, l.created_at FROM bindings b
                 JOIN cfm_profiles p ON p.mooring_user = b.mooring_user
                 LEFT JOIN profile_bundle pb ON pb.id = 1
                 LEFT JOIN subscription_links l ON l.account_id = b.account_id
                 WHERE b.account_id = ?1",
                [account.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((generated_at, link_created_at)) = found else {
            return Ok(CfmDelivery::NoProfile);
        };
        // An import writes the bundle's time with its profiles.
        let generated_at = generated_at.ok_or_else(|| {
            corrupt(
                "profile bundle time",
                "a profile is stored without it".to_owned(),
            )
        })?;
        Ok(CfmDelivery::Profile {
            generated_at: time_from_sql(generated_at, "profile bundle time")?,
            link_created_at: link_created_at
                .map(|secs| time_from_sql(secs, "link creation time"))
                .transpose()?,
        })
    }

    /// When the link of `account` was made, if it has one, whether or not
    /// the account has a profile: for tests of the invariants.
    #[cfg(test)]
    pub(crate) fn subscription_link_created(
        &self,
        account: AccountId,
    ) -> Result<Option<UnixTime>, StoreError> {
        let created: Option<i64> = self
            .conn
            .query_row(
                "SELECT created_at FROM subscription_links WHERE account_id = ?1",
                [account.0],
                |row| row.get(0),
            )
            .optional()?;
        created
            .map(|secs| time_from_sql(secs, "link creation time"))
            .transpose()
    }

    /// What the link whose token has `digest` serves.
    pub fn linked_profile(&self, digest: &TokenDigest) -> Result<LinkedProfile, StoreError> {
        let found: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT p.document FROM subscription_links l
                 JOIN bindings b ON b.account_id = l.account_id
                 LEFT JOIN cfm_profiles p ON p.mooring_user = b.mooring_user
                 WHERE l.token_digest = ?1",
                [digest.as_bytes()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match found {
            None => LinkedProfile::Unknown,
            Some(None) => LinkedProfile::NoProfile,
            Some(Some(document)) => LinkedProfile::Profile(stored_profile(&document)?),
        })
    }
}
