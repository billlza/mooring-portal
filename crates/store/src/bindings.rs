//! Bindings of portal accounts to Mooring users, one to one.
//!
//! Only the operator binds, by account code (see
//! `portal_core::account::AccountCode` for why not by email). Unbinding
//! deletes the user's subscription record with the binding.

use portal_core::account::AccountCode;
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::rows::{code_from_sql, corrupt, time_from_sql, time_to_sql, user_from_sql};
use crate::{AccountId, Store, StoreError, UnknownAccount};

/// The Mooring user an account is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The Mooring user.
    pub user: MooringUserId,
    /// When the operator bound it.
    pub bound_at: UnixTime,
}

/// Why a binding was not made.
#[derive(Debug, thiserror::Error)]
pub enum BindError {
    /// No account has the code.
    #[error(transparent)]
    UnknownAccount(#[from] UnknownAccount),
    /// The account is bound already.
    #[error("account {account} is already bound to Mooring user {user}; unbind it first")]
    AccountBound {
        /// The account.
        account: AccountCode,
        /// Its Mooring user.
        user: MooringUserId,
    },
    /// The Mooring user is bound to another account already.
    #[error("Mooring user {user} is already bound to account {account}; unbind that account first")]
    UserBound {
        /// The Mooring user.
        user: MooringUserId,
        /// The account it is bound to.
        account: AccountCode,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for BindError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// Why an account was not unbound.
#[derive(Debug, thiserror::Error)]
pub enum UnbindError {
    /// No account has the code.
    #[error(transparent)]
    UnknownAccount(#[from] UnknownAccount),
    /// The account is not bound.
    #[error("account {0} is not bound to a Mooring user")]
    NotBound(AccountCode),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for UnbindError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

impl Store {
    /// Binds the account with `code` to `user`.
    pub fn bind(
        &mut self,
        code: &AccountCode,
        user: &MooringUserId,
        now: UnixTime,
    ) -> Result<(), BindError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(i64, Option<String>)> = tx
            .query_row(
                "SELECT a.id, b.mooring_user FROM accounts a
                 LEFT JOIN bindings b ON b.account_id = a.id WHERE a.code = ?1",
                [code.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((account, bound)) = found else {
            return Err(UnknownAccount(*code).into());
        };
        if let Some(bound) = bound {
            return Err(BindError::AccountBound {
                account: *code,
                user: user_from_sql(&bound)?,
            });
        }
        let holder: Option<String> = tx
            .query_row(
                "SELECT a.code FROM accounts a JOIN bindings b ON b.account_id = a.id
                 WHERE b.mooring_user = ?1",
                [user.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(holder) = holder {
            return Err(BindError::UserBound {
                user: user.clone(),
                account: code_from_sql(&holder)?,
            });
        }
        tx.execute(
            "INSERT INTO bindings (account_id, mooring_user, bound_at) VALUES (?1, ?2, ?3)",
            params![account, user.as_str(), time_to_sql(now)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Unbinds the account with `code`, returning the Mooring user it was
    /// bound to; the user's subscription record goes with the binding.
    pub fn unbind(&mut self, code: &AccountCode) -> Result<MooringUserId, UnbindError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(i64, Option<String>)> = tx
            .query_row(
                "SELECT a.id, b.mooring_user FROM accounts a
                 LEFT JOIN bindings b ON b.account_id = a.id WHERE a.code = ?1",
                [code.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((account, bound)) = found else {
            return Err(UnknownAccount(*code).into());
        };
        let Some(bound) = bound else {
            return Err(UnbindError::NotBound(*code));
        };
        let user = user_from_sql(&bound)?;
        tx.execute("DELETE FROM bindings WHERE account_id = ?1", [account])?;
        tx.commit()?;
        Ok(user)
    }

    /// The binding of `account`, if it has one.
    pub fn binding(&self, account: AccountId) -> Result<Option<Binding>, StoreError> {
        let found: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT mooring_user, bound_at FROM bindings WHERE account_id = ?1",
                [account.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        found
            .map(|(user, bound_at)| binding_row(&user, bound_at))
            .transpose()
    }
}

/// A binding row read back.
pub(crate) fn binding_row(user: &str, bound_at: i64) -> Result<Binding, StoreError> {
    Ok(Binding {
        user: user_from_sql(user)?,
        bound_at: time_from_sql(bound_at, "binding time")?,
    })
}

/// A binding read from a left join: both columns or neither.
pub(crate) fn binding_from_sql(
    user: Option<String>,
    bound_at: Option<i64>,
) -> Result<Option<Binding>, StoreError> {
    match (user, bound_at) {
        (Some(user), Some(bound_at)) => binding_row(&user, bound_at).map(Some),
        (None, None) => Ok(None),
        _ => Err(corrupt(
            "binding",
            "a joined binding has a user without a time, or a time without a user".to_owned(),
        )),
    }
}
