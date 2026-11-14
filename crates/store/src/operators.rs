//! Operators: the accounts that may use the operator pages.
//!
//! Only the operator's command on the host makes an account an operator
//! (`grant-operator`); nothing on the site can. An operator's session acts
//! on the operator pages only within [`OPERATOR_CONFIRM_SECS`] of
//! confirming its account's password, and confirming replaces the session
//! with a new one ([`Store::confirm_session`]): a copy of the old token,
//! taken however long before, opens nothing. A session that has never
//! confirmed, such as the one a password change starts, has no
//! confirmation.
//!
//! Every write the operator pages make runs through [`Store::as_operator`],
//! which checks the session again inside the write's own transaction: a
//! request's session is read when its headers arrive, and by the time its
//! body has, the session may have ended, its account may be an operator no
//! more, or its confirmation may have run out. [`OperatorActions`] holds
//! the writes the pages may make, and only that check makes one. It also
//! reads what a page shows after a write, in the write's transaction: a
//! write commits together with what its page will say of it, or not at
//! all.

use portal_core::account::AccountCode;
use portal_core::auth::TokenDigest;
use portal_core::feedback::MessageBody;
use portal_core::payment::stripe::StripeId;
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::accounts::{accounts_on, open_subscriptions};
use crate::bans::ban_on;
use crate::bindings::{account_by_code, bind_on, unbind_on};
use crate::feedback::reply_on;
use crate::rentals::close_booking_on;
use crate::rows::{stripe_id_from_sql, time_from_sql, time_to_sql};
use crate::sessions::insert_session;
use crate::{
    AccountId, AccountSummary, Ban, BanError, BindError, NewSession, OperatorBooking, ReplyError,
    Store, StoreError, UnbindError, UnknownAccount,
};

/// How long a confirmation of the password lets a session act on the
/// operator pages, in seconds: from the moment it is made, however much
/// the operator does meanwhile.
pub const OPERATOR_CONFIRM_SECS: u64 = 30 * 60;

/// The session of an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorSession {
    /// The operator's account.
    pub account: AccountId,
    /// Whether the session confirmed the account's password within
    /// [`OPERATOR_CONFIRM_SECS`].
    pub confirmed: bool,
}

/// Why a session may not act on the operator pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAllowed {
    /// No unexpired session has the token.
    SignedOut,
    /// The session's account is no operator.
    NotOperator,
    /// The session has not confirmed the password within
    /// [`OPERATOR_CONFIRM_SECS`].
    Unconfirmed,
}

/// What writes asked for on the operator pages came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsOperator<T> {
    /// The session could act, and the writes were made: what they gave.
    Done(T),
    /// The session could not act, and nothing was written.
    NotAllowed(NotAllowed),
}

impl<T> AsOperator<T> {
    /// What `then` makes of the writes' result, if they were made.
    pub fn map<U>(self, then: impl FnOnce(T) -> U) -> AsOperator<U> {
        match self {
            Self::Done(done) => AsOperator::Done(then(done)),
            Self::NotAllowed(not) => AsOperator::NotAllowed(not),
        }
    }
}

/// What unbinding an account from the operator pages did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unbound {
    /// The Mooring user the account was bound to.
    pub user: MooringUserId,
    /// The account's subscriptions that Stripe may still charge, though no
    /// Mooring user can be granted them now.
    pub subscriptions: Vec<StripeId>,
}

/// Why an account's operator rights did not change.
#[derive(Debug, thiserror::Error)]
pub enum OperatorError {
    /// No account has the code.
    #[error(transparent)]
    UnknownAccount(#[from] UnknownAccount),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for OperatorError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// Whether a confirmation made at `confirmed_at` lets a session act at
/// `now`. One later than `now`, which only a clock set back leaves, counts
/// for nothing.
fn confirmed_lately(confirmed_at: Option<UnixTime>, now: UnixTime) -> bool {
    confirmed_at.is_some_and(|at| at <= now && now.as_secs() - at.as_secs() < OPERATOR_CONFIRM_SECS)
}

/// The unexpired session with `digest` at `now`, as `conn` sees it: whose
/// it is, whether its account is an operator, and whether it confirmed
/// the password lately; `None` if there is no such session.
fn standing(
    conn: &Connection,
    digest: &TokenDigest,
    now: UnixTime,
) -> Result<Option<(AccountId, bool, bool)>, StoreError> {
    let found: Option<(i64, bool, Option<i64>)> = conn
        .query_row(
            "SELECT s.account_id,
                    EXISTS (SELECT 1 FROM operators o WHERE o.account_id = s.account_id),
                    s.confirmed_at
             FROM sessions s WHERE s.token_digest = ?1 AND s.expires_at > ?2",
            params![digest.as_bytes(), time_to_sql(now)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    found
        .map(|(account, operator, confirmed_at)| {
            let confirmed_at = confirmed_at
                .map(|secs| time_from_sql(secs, "session confirmation time"))
                .transpose()?;
            Ok((
                AccountId(account),
                operator,
                confirmed_lately(confirmed_at, now),
            ))
        })
        .transpose()
}

impl Store {
    /// Makes the account with `code` an operator; whether it was not one
    /// already.
    pub fn grant_operator(
        &mut self,
        code: &AccountCode,
        now: UnixTime,
    ) -> Result<bool, OperatorError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((account, _)) = account_by_code(&tx, code)? else {
            return Err(UnknownAccount(*code).into());
        };
        let granted = tx.execute(
            "INSERT INTO operators (account_id, granted_at) VALUES (?1, ?2)
             ON CONFLICT (account_id) DO NOTHING",
            params![account.0, time_to_sql(now)],
        )?;
        tx.commit()?;
        Ok(granted > 0)
    }

    /// Makes the account with `code` an operator no more; whether it was
    /// one. Its sessions' confirmations go too, so that granting it again
    /// asks for the password again.
    pub fn revoke_operator(&mut self, code: &AccountCode) -> Result<bool, OperatorError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((account, _)) = account_by_code(&tx, code)? else {
            return Err(UnknownAccount(*code).into());
        };
        let revoked = tx.execute("DELETE FROM operators WHERE account_id = ?1", [account.0])?;
        tx.execute(
            "UPDATE sessions SET confirmed_at = NULL WHERE account_id = ?1",
            [account.0],
        )?;
        tx.commit()?;
        Ok(revoked > 0)
    }

    /// The operator signed in with the unexpired session whose token has
    /// `digest`; `None` if there is no such session, or its account is no
    /// operator.
    pub fn operator_session(
        &self,
        digest: &TokenDigest,
        now: UnixTime,
    ) -> Result<Option<OperatorSession>, StoreError> {
        Ok(standing(&self.conn, digest, now)?
            .filter(|&(_, operator, _)| operator)
            .map(|(account, _, confirmed)| OperatorSession { account, confirmed }))
    }

    /// Replaces the unexpired session with `digest`, an operator's, with
    /// `replacement`, a session of the same account that has confirmed
    /// the password as it starts. Whether there was such a session; if
    /// there was not, nothing changes.
    pub fn confirm_session(
        &mut self,
        digest: &TokenDigest,
        replacement: &NewSession,
    ) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = replacement.created_at;
        let Some((account, true, _)) = standing(&tx, digest, now)? else {
            return Ok(false);
        };
        if account != replacement.account {
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM sessions WHERE token_digest = ?1",
            [digest.as_bytes()],
        )?;
        insert_session(&tx, replacement)?;
        tx.execute(
            "UPDATE sessions SET confirmed_at = ?2 WHERE token_digest = ?1",
            params![replacement.digest.as_bytes(), time_to_sql(now)],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Makes `writes` for the session with `digest` at `now`, in one
    /// immediate transaction that first checks the session may act on
    /// the operator pages. What `writes` did is committed if it succeeds;
    /// nothing is, if it fails or the session may not act.
    pub fn as_operator<T, E>(
        &mut self,
        digest: &TokenDigest,
        now: UnixTime,
        writes: impl FnOnce(&OperatorActions<'_>) -> Result<T, E>,
    ) -> Result<AsOperator<T>, E>
    where
        E: From<StoreError>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)?;
        let not = match standing(&tx, digest, now)? {
            None => Some(NotAllowed::SignedOut),
            Some((_, false, _)) => Some(NotAllowed::NotOperator),
            Some((_, true, false)) => Some(NotAllowed::Unconfirmed),
            Some((_, true, true)) => None,
        };
        if let Some(not) = not {
            return Ok(AsOperator::NotAllowed(not));
        }
        let done = writes(&OperatorActions { tx: &tx, now })?;
        tx.commit().map_err(StoreError::from)?;
        Ok(AsOperator::Done(done))
    }
}

/// The writes of the operator pages, on a transaction begun by
/// [`Store::as_operator`] for a session that may make them.
#[derive(Debug)]
pub struct OperatorActions<'a> {
    tx: &'a Transaction<'a>,
    now: UnixTime,
}

impl OperatorActions<'_> {
    /// Binds the account with `code` to `user`, as [`Store::bind`] does.
    pub fn bind(&self, code: &AccountCode, user: &MooringUserId) -> Result<(), BindError> {
        bind_on(self.tx, code, user, self.now)
    }

    /// Unbinds the account with `code`, as [`Store::unbind`] does, and
    /// says which of its subscriptions Stripe may still charge.
    pub fn unbind(&self, code: &AccountCode) -> Result<Unbound, UnbindError> {
        let (account, user) = unbind_on(self.tx, code)?;
        let subscriptions = open_subscriptions(self.tx, account.0)?
            .iter()
            .map(|subscription| stripe_id_from_sql(subscription))
            .collect::<Result<_, _>>()?;
        Ok(Unbound {
            user,
            subscriptions,
        })
    }

    /// Bans the account with `code` without cards, as [`Store::ban`]
    /// does; `None`, and nothing done, for an operator's account: only the
    /// host takes away what only the host gives.
    pub fn ban(&self, code: &AccountCode) -> Result<Option<Ban>, BanError> {
        let operator: bool = self.tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM operators o JOIN accounts a ON a.id = o.account_id
                            WHERE a.code = ?1)",
            [code.to_string()],
            |row| row.get(0),
        )?;
        if operator {
            return Ok(None);
        }
        ban_on(self.tx, code, &[], self.now).map(Some)
    }

    /// Closes booking `id`, as [`Store::close_booking`] does.
    pub fn close_booking(&self, id: u64) -> Result<Option<OperatorBooking>, StoreError> {
        close_booking_on(self.tx, id)
    }

    /// Sends the operator's `body` to the account with `code`.
    pub fn reply(&self, code: &AccountCode, body: &MessageBody) -> Result<(), ReplyError> {
        reply_on(self.tx, code, body, self.now)
    }

    /// Every account, as [`Store::accounts`] lists them, with what this
    /// transaction has written so far.
    pub fn accounts(&self) -> Result<Vec<AccountSummary>, StoreError> {
        accounts_on(self.tx)
    }
}
