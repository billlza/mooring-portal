//! Bans: the operator's explicit act against an account caught abusing
//! the service. Mooring's revocation does not say why a user went, so a
//! ban is never read from a bundle.
//!
//! A ban deletes the account with everything it holds, so its owner can no
//! longer log in and its subscription link ends for good, and it keeps the
//! Argon2id digest of the account's email address, so that the address
//! cannot register again. Only the address the portal already knows is used, so
//! a banned person who registers with another address gets through; what
//! keeps them from being served is that only the operator binds accounts
//! to Mooring users.

use portal_core::account::AccountCode;
use portal_core::auth::{BanDigest, HashError};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::rows::{email_from_sql, time_to_sql, user_from_sql};
use crate::{Store, StoreError, UnknownAccount};

/// What a ban removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ban {
    /// The Mooring user the account was bound to, if any.
    pub user: Option<MooringUserId>,
    /// The account had a subscription link, which ended with it.
    pub link_ended: bool,
}

/// Why an account was not banned.
#[derive(Debug, thiserror::Error)]
pub enum BanError {
    /// No account has the code.
    #[error(transparent)]
    Unknown(#[from] UnknownAccount),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The address could not be hashed for the list.
    #[error(transparent)]
    Hash(#[from] HashError),
}

impl From<rusqlite::Error> for BanError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

impl Store {
    /// Bans the account with `code`: deletes it with its sessions,
    /// binding, subscription record, CFM profile and subscription link, and
    /// keeps its email address from registering again. Copies of the rows
    /// stay in the database files until [`Store::erase_deleted`].
    pub fn ban(&mut self, code: &AccountCode, now: UnixTime) -> Result<Ban, BanError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(i64, String, Option<String>, bool)> = tx
            .query_row(
                "SELECT a.id, a.email, b.mooring_user,
                        EXISTS (SELECT 1 FROM subscription_links l WHERE l.account_id = a.id)
                 FROM accounts a LEFT JOIN bindings b ON b.account_id = a.id
                 WHERE a.code = ?1",
                [code.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((id, email, user, link_ended)) = found else {
            return Err(UnknownAccount(*code).into());
        };
        let email = email_from_sql(&email)?;
        let user = user.map(|user| user_from_sql(&user)).transpose()?;
        // Slow, and inside the write lock; an operator's command waits for
        // it once, and the server's writes wait as long.
        let digest = BanDigest::of(&email)?;
        tx.execute(
            "INSERT INTO banned_emails (email_digest, banned_at) VALUES (?1, ?2)",
            params![digest.as_bytes(), time_to_sql(now)],
        )?;
        tx.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(Ban { user, link_ended })
    }

    /// Lets the address whose ban digest is `digest` register again;
    /// whether it was banned.
    pub fn unban(&mut self, digest: &BanDigest) -> Result<bool, StoreError> {
        let removed = self.conn.execute(
            "DELETE FROM banned_emails WHERE email_digest = ?1",
            [digest.as_bytes()],
        )?;
        Ok(removed > 0)
    }
}
