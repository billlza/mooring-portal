//! Login sessions, stored by the digest of their token.
//!
//! An account keeps at most [`MAX_SESSIONS_PER_ACCOUNT`] sessions (the
//! oldest go first), and every new session sweeps out expired ones, so the
//! table stays bounded without a background job.

use portal_core::auth::{StoredPasswordHash, TokenDigest};
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::accounts::Account;
use crate::rows::time_to_sql;
use crate::{AccountId, Store, StoreError};

/// The most sessions one account keeps.
pub const MAX_SESSIONS_PER_ACCOUNT: u32 = 10;

/// What changing a password did besides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordChange {
    /// The account's subscription link was removed.
    pub link_revoked: bool,
}

/// A session to record.
#[derive(Debug, Clone, Copy)]
pub struct NewSession {
    /// The digest of the session token the browser holds.
    pub digest: TokenDigest,
    /// The account logged in.
    pub account: AccountId,
    /// When the session starts.
    pub created_at: UnixTime,
    /// When it stops being valid.
    pub expires_at: UnixTime,
}

impl Store {
    /// Records `session`, dropping expired sessions and the account's
    /// oldest beyond [`MAX_SESSIONS_PER_ACCOUNT`].
    pub fn start_session(&mut self, session: &NewSession) -> Result<(), StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    /// The account whose unexpired session has `digest`.
    pub fn session_account(
        &self,
        digest: &TokenDigest,
        now: UnixTime,
    ) -> Result<Option<Account>, StoreError> {
        match session_account_id(&self.conn, digest, now)? {
            Some(account) => self.account_row(account),
            None => Ok(None),
        }
    }

    /// Ends the session with `digest`, if it exists.
    pub fn end_session(&mut self, digest: &TokenDigest) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM sessions WHERE token_digest = ?1",
            [digest.as_bytes()],
        )?;
        Ok(())
    }

    /// Sets `account`'s password hash, replaces all of its sessions with
    /// `replacement`, so that every other browser is logged out, and
    /// removes its subscription link, which whoever else was signed in
    /// could have made.
    pub fn change_password(
        &mut self,
        hash: &StoredPasswordHash,
        replacement: &NewSession,
    ) -> Result<PasswordChange, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = replacement.account.0;
        let changed = tx.execute(
            "UPDATE accounts SET password_hash = ?1, password_changed_at = ?2 WHERE id = ?3",
            params![hash.as_str(), time_to_sql(replacement.created_at), account],
        )?;
        if changed == 0 {
            return Err(StoreError::Missing { what: "account" });
        }
        tx.execute("DELETE FROM sessions WHERE account_id = ?1", [account])?;
        let links = tx.execute(
            "DELETE FROM subscription_links WHERE account_id = ?1",
            [account],
        )?;
        insert_session(&tx, replacement)?;
        tx.commit()?;
        Ok(PasswordChange {
            link_revoked: links > 0,
        })
    }
}

/// The account whose unexpired session has `digest`, read on `conn`. A
/// write that acts for a signed-in account reads it in its own
/// transaction: a request's session is read when its headers arrive, and
/// the session may have ended (logout, password change, deletion of the
/// account) before its body has.
pub(crate) fn session_account_id(
    conn: &Connection,
    digest: &TokenDigest,
    now: UnixTime,
) -> Result<Option<AccountId>, StoreError> {
    let account: Option<i64> = conn
        .query_row(
            "SELECT account_id FROM sessions WHERE token_digest = ?1 AND expires_at > ?2",
            params![digest.as_bytes(), time_to_sql(now)],
            |row| row.get(0),
        )
        .optional()?;
    Ok(account.map(AccountId))
}

fn insert_session(tx: &Transaction<'_>, session: &NewSession) -> Result<(), StoreError> {
    tx.execute(
        "DELETE FROM sessions WHERE expires_at <= ?1",
        [time_to_sql(session.created_at)],
    )?;
    tx.execute(
        "INSERT INTO sessions (token_digest, account_id, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            session.digest.as_bytes(),
            session.account.0,
            time_to_sql(session.created_at),
            time_to_sql(session.expires_at),
        ],
    )?;
    tx.execute(
        "DELETE FROM sessions WHERE account_id = ?1 AND rowid NOT IN (
             SELECT rowid FROM sessions WHERE account_id = ?1
             ORDER BY created_at DESC, rowid DESC LIMIT ?2)",
        params![session.account.0, MAX_SESSIONS_PER_ACCOUNT],
    )?;
    Ok(())
}
