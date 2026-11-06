//! Subscription links: at most one per bound account, stored by the digest
//! of its token.
//!
//! A link exists only while its account is bound: the table refers to the
//! binding, so unbinding the account or deleting it removes the link. A
//! link is made only for an account with a profile, in the same transaction
//! that checks for it. A new link replaces the old one, and changing the
//! password removes it
//! (`sessions.rs`), so a link made by whoever was signed in stops working
//! once the owner changes the password.

use portal_core::auth::TokenDigest;
use portal_core::profile::CfmProfile;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::profiles::stored_profile;
use crate::rows::{time_from_sql, time_to_sql};
use crate::{AccountId, Store, StoreError};

/// Why a link was not made.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
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
    /// Makes the link of `account`, whose token has `digest`, replacing any
    /// it had. The account must be bound and its Mooring user have a
    /// profile.
    pub fn replace_subscription_link(
        &mut self,
        account: AccountId,
        digest: &TokenDigest,
        now: UnixTime,
    ) -> Result<(), LinkError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
            params![account.0, digest.as_bytes(), time_to_sql(now)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Removes the link of `account`; whether it had one.
    pub fn revoke_subscription_link(&mut self, account: AccountId) -> Result<bool, StoreError> {
        let removed = self.conn.execute(
            "DELETE FROM subscription_links WHERE account_id = ?1",
            [account.0],
        )?;
        Ok(removed > 0)
    }

    /// When the link of `account` was made, if it has one.
    pub fn subscription_link_created(
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
