//! Accounts: registration, credentials, listing and deletion.

use portal_core::account::{AccountCode, Email};
use portal_core::auth::StoredPasswordHash;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::bindings::{Binding, binding_from_sql};
use crate::rows::{code_from_sql, email_from_sql, time_from_sql, time_to_sql};
use crate::{AccountId, Store, StoreError, UnknownAccount};

/// What registration records.
#[derive(Debug)]
pub struct NewAccount {
    /// A fresh random code.
    pub code: AccountCode,
    /// The login email.
    pub email: Email,
    /// The Argon2id hash of the chosen password.
    pub password_hash: StoredPasswordHash,
    /// When the account is created.
    pub created_at: UnixTime,
}

/// An account as its owner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The row id.
    pub id: AccountId,
    /// The code the owner reads out to have the account bound.
    pub code: AccountCode,
    /// The login email.
    pub email: Email,
    /// When the account was created.
    pub created_at: UnixTime,
}

/// What a login checks a password against.
#[derive(Debug, Clone)]
pub struct Credentials {
    /// The account.
    pub account: AccountId,
    /// Its password hash.
    pub password_hash: StoredPasswordHash,
}

/// An account as the operator lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    /// The account's code.
    pub code: AccountCode,
    /// The login email.
    pub email: Email,
    /// When the account was created.
    pub created_at: UnixTime,
    /// The Mooring user it is bound to, if any.
    pub binding: Option<Binding>,
}

/// Why registration did not create an account.
#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    /// Another account has the email.
    #[error("an account with this email address already exists")]
    EmailTaken,
    /// The portal holds as many accounts as it is configured to.
    #[error("registration is closed: the portal holds its limit of {limit} accounts")]
    Full {
        /// The configured limit.
        limit: u32,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for RegisterError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

impl Store {
    /// Creates `account` unless its email is taken or the portal already
    /// holds `limit` accounts.
    pub fn register(&mut self, account: NewAccount, limit: u32) -> Result<Account, RegisterError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE email = ?1)",
            [account.email.as_str()],
            |row| row.get(0),
        )?;
        if taken {
            return Err(RegisterError::EmailTaken);
        }
        let held: i64 = tx.query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))?;
        if held >= i64::from(limit) {
            return Err(RegisterError::Full { limit });
        }
        tx.execute(
            "INSERT INTO accounts (code, email, password_hash, created_at, password_changed_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                account.code.to_string(),
                account.email.as_str(),
                account.password_hash.as_str(),
                time_to_sql(account.created_at),
            ],
        )?;
        let id = AccountId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(Account {
            id,
            code: account.code,
            email: account.email,
            created_at: account.created_at,
        })
    }

    /// The credentials of the account with `email`, if there is one.
    pub fn credentials(&self, email: &Email) -> Result<Option<Credentials>, StoreError> {
        let found = self
            .conn
            .query_row(
                "SELECT id, password_hash FROM accounts WHERE email = ?1",
                [email.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(found.map(|(id, hash)| Credentials {
            account: AccountId(id),
            password_hash: StoredPasswordHash::from_stored(hash),
        }))
    }

    /// The credentials of `account` (`Missing` if it was deleted).
    pub fn credentials_of(&self, account: AccountId) -> Result<Credentials, StoreError> {
        let hash: String = self
            .conn
            .query_row(
                "SELECT password_hash FROM accounts WHERE id = ?1",
                [account.0],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::Missing { what: "account" })?;
        Ok(Credentials {
            account,
            password_hash: StoredPasswordHash::from_stored(hash),
        })
    }

    /// Every account, oldest first, with its binding.
    pub fn accounts(&self) -> Result<Vec<AccountSummary>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT a.code, a.email, a.created_at, b.mooring_user, b.bound_at
             FROM accounts a LEFT JOIN bindings b ON b.account_id = a.id
             ORDER BY a.created_at, a.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        let mut accounts = Vec::new();
        for row in rows {
            let (code, email, created_at, user, bound_at) = row?;
            accounts.push(AccountSummary {
                code: code_from_sql(&code)?,
                email: email_from_sql(&email)?,
                created_at: time_from_sql(created_at, "account creation time")?,
                binding: binding_from_sql(user, bound_at)?,
            });
        }
        Ok(accounts)
    }

    /// Deletes the account with `code`, with its sessions, binding and
    /// subscription record.
    pub fn delete_account(&mut self, code: &AccountCode) -> Result<(), DeleteError> {
        let deleted = self
            .conn
            .execute("DELETE FROM accounts WHERE code = ?1", [code.to_string()])?;
        if deleted == 0 {
            return Err(DeleteError::Unknown(UnknownAccount(*code)));
        }
        Ok(())
    }

    /// The account with row id `account`, if it exists.
    pub(crate) fn account_row(&self, account: AccountId) -> Result<Option<Account>, StoreError> {
        let found = self
            .conn
            .query_row(
                "SELECT code, email, created_at FROM accounts WHERE id = ?1",
                [account.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        found
            .map(|(code, email, created_at)| {
                Ok(Account {
                    id: account,
                    code: code_from_sql(&code)?,
                    email: email_from_sql(&email)?,
                    created_at: time_from_sql(created_at, "account creation time")?,
                })
            })
            .transpose()
    }
}

/// Why an account was not deleted.
#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    /// No account has the code.
    #[error(transparent)]
    Unknown(#[from] UnknownAccount),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for DeleteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}
