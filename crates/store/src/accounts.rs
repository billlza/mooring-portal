//! Accounts: registration, credentials, listing and deletion.

use portal_core::account::{AccountCode, Email};
use portal_core::auth::{DigestedEmail, StoredPasswordHash};
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};

use crate::bindings::{Binding, binding_from_sql};
use crate::orders::{ORDER_TABLES, RENEWAL_TABLES, RENEWS, UNSETTLED};
use crate::rows::{code_from_sql, email_from_sql, time_from_sql, time_to_sql};
use crate::{AccountId, Store, StoreError, UnknownAccount};

/// What registration records.
#[derive(Debug)]
pub struct NewAccount {
    /// A fresh random code.
    pub code: AccountCode,
    /// The login email, with the ban list's digest of it, computed off the
    /// database's thread because it is slow; registration is refused when
    /// the list holds the digest.
    pub email: DigestedEmail,
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
    /// The address belonged to a banned account.
    #[error("this email address is banned from registering")]
    Banned,
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
        let banned: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM banned_emails WHERE email_digest = ?1)",
            [account.email.ban_digest().as_bytes()],
            |row| row.get(0),
        )?;
        if banned {
            return Err(RegisterError::Banned);
        }
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE email = ?1)",
            [account.email.email().as_str()],
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
                account.email.email().as_str(),
                account.password_hash.as_str(),
                time_to_sql(account.created_at),
            ],
        )?;
        let id = AccountId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(Account {
            id,
            code: account.code,
            email: account.email.into_email(),
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

    /// Deletes the account with `code`, with its sessions, binding,
    /// subscription record, CFM profile, subscription link and orders, and
    /// with what Stripe said of those orders' payments. Copies of the rows
    /// stay in the database files until [`Store::erase_deleted`].
    ///
    /// Refused while money is in the air: while Stripe may still charge a
    /// subscription of the account, which no one could then stop from
    /// here; and while an order of it is paid but Mooring does not have
    /// it yet, or waits for the operator without its payment having gone
    /// back (refunded in full, or disputed).
    pub fn delete_account(&mut self, code: &AccountCode) -> Result<(), DeleteError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account: Option<i64> = tx
            .query_row(
                "SELECT id FROM accounts WHERE code = ?1",
                [code.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(account) = account else {
            return Err(DeleteError::Unknown(UnknownAccount(*code)));
        };
        let subscriptions = open_subscriptions(&tx, account)?;
        if !subscriptions.is_empty() {
            return Err(DeleteError::Renewing {
                account: *code,
                subscriptions: subscriptions.join(", "),
            });
        }
        let orders: Vec<String> = tx
            .prepare(&format!(
                "SELECT o.id FROM {ORDER_TABLES} WHERE o.account_id = ?1 AND {UNSETTLED}
                 ORDER BY o.created_at, o.id"
            ))?
            .query_map([account], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        if !orders.is_empty() {
            return Err(DeleteError::Unsettled {
                account: *code,
                orders: orders.join(", "),
            });
        }
        forget_payments(&tx, account)?;
        tx.execute("DELETE FROM accounts WHERE id = ?1", [account])?;
        tx.commit()?;
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

/// The subscriptions of the account with row id `account` that Stripe
/// may charge again, in id order.
pub(crate) fn open_subscriptions(
    conn: &Connection,
    account: i64,
) -> Result<Vec<String>, StoreError> {
    Ok(conn
        .prepare(&format!(
            "SELECT DISTINCT o.subscription FROM {RENEWAL_TABLES}
             WHERE o.account_id = ?1 AND {RENEWS}
             ORDER BY o.subscription"
        ))?
        .query_map([account], |row| row.get(0))?
        .collect::<Result<_, _>>()?)
}

/// Forgets what Stripe said of the payments, invoices and subscriptions
/// of the orders of the account with row id `account`: the orders are
/// about to go. A stray payment can independently name one of these
/// facts; in particular, it needs to remember a subscription has ended.
pub(crate) fn forget_payments(conn: &Connection, account: i64) -> Result<(), StoreError> {
    for (table, key) in [
        ("stripe_payments", "payment_intent"),
        ("stripe_invoice_payments", "invoice"),
        ("stripe_subscriptions", "subscription"),
    ] {
        conn.execute(
            &format!(
                "DELETE FROM {table} WHERE {key} IN
                     (SELECT {key} FROM orders WHERE account_id = ?1 AND {key} IS NOT NULL)
                   AND {key} NOT IN
                     (SELECT {key} FROM stray_payments WHERE {key} IS NOT NULL)"
            ),
            [account],
        )?;
    }
    Ok(())
}

/// Why an account was not deleted.
#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    /// No account has the code.
    #[error(transparent)]
    Unknown(#[from] UnknownAccount),
    /// Stripe may still charge a subscription of the account.
    #[error(
        "account {account} has a subscription that Stripe may charge again ({subscriptions}): \
         cancel it at Stripe, and delete the account once Stripe has told the portal that it \
         ended (`orders` shows when)"
    )]
    Renewing {
        /// The account.
        account: AccountCode,
        /// Its subscriptions that have not ended, comma-separated.
        subscriptions: String,
    },
    /// An order of the account is paid and not yet with Mooring, or waits
    /// for the operator.
    #[error(
        "account {account} has orders that are not settled ({orders}): a paid order is settled \
         once Mooring has pulled it, one in review by a full refund at Stripe or by \
         `approve-order` (`orders` shows where each stands)"
    )]
    Unsettled {
        /// The account.
        account: AccountCode,
        /// The orders, comma-separated.
        orders: String,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for DeleteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}
