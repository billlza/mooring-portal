//! Feedback: each account's conversation with the operator
//! (`portal_core::feedback`).
//!
//! A signed-in user writes to the operator from the site, for the account
//! of a session still live in the same transaction; the operator answers
//! from the operator pages, and the user reads the answer on the site. The
//! conversation is the account's: deleting or banning the account deletes
//! it. A user sends at most [`MAX_SENT_PER_DAY`] messages in any 24 hours
//! and a conversation keeps its latest [`MAX_KEPT`] messages, so the table
//! stays bounded however much anyone writes.

use portal_core::account::{AccountCode, Email};
use portal_core::auth::TokenDigest;
use portal_core::feedback::MessageBody;
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::bindings::account_by_code;
use crate::rows::{
    code_from_sql, corrupt, count_from_sql, email_from_sql, time_from_sql, time_to_sql,
};
use crate::sessions::session_account_id;
use crate::{AccountId, Store, StoreError, UnknownAccount};

/// The most messages a user sends in any 24 hours.
pub const MAX_SENT_PER_DAY: u32 = 20;
/// The most messages a conversation keeps: the latest.
pub const MAX_KEPT: u32 = 200;

/// A day, in seconds.
const DAY_SECS: i64 = 24 * 60 * 60;

/// A message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackMessage {
    /// Whether the operator wrote it, rather than the account's owner.
    pub from_operator: bool,
    /// Its text.
    pub body: MessageBody,
    /// When it was sent.
    pub sent_at: UnixTime,
}

/// A conversation, as the operator's list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    /// The account.
    pub account: AccountCode,
    /// Its login email.
    pub email: Email,
    /// When its latest message was sent.
    pub last_sent_at: UnixTime,
    /// Whether its latest message is the user's: it waits for a reply.
    pub awaiting_reply: bool,
    /// How many messages it keeps.
    pub messages: u64,
}

/// Why a user's message was not sent.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// No live session has the digest.
    #[error("the session has ended")]
    SignedOut,
    /// The account sent [`MAX_SENT_PER_DAY`] messages in the last 24 hours.
    #[error("the account sent {MAX_SENT_PER_DAY} messages in the last 24 hours")]
    TooMany,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for SendError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// Why the operator's reply was not sent.
#[derive(Debug, thiserror::Error)]
pub enum ReplyError {
    /// No account has the code.
    #[error(transparent)]
    UnknownAccount(#[from] UnknownAccount),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ReplyError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

impl Store {
    /// Sends `body` to the operator from the account signed in with the
    /// session whose token has `session`.
    pub fn send_feedback(
        &mut self,
        session: &TokenDigest,
        body: &MessageBody,
        now: UnixTime,
    ) -> Result<(), SendError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Err(SendError::SignedOut);
        };
        let sent: i64 = tx.query_row(
            "SELECT count(*) FROM feedback
             WHERE account_id = ?1 AND from_operator = 0 AND sent_at > ?2",
            params![account.0, time_to_sql(now).saturating_sub(DAY_SECS)],
            |row| row.get(0),
        )?;
        if sent >= i64::from(MAX_SENT_PER_DAY) {
            return Err(SendError::TooMany);
        }
        add_message(&tx, account, false, body, now)?;
        tx.commit()?;
        Ok(())
    }

    /// The conversation of the account signed in with the unexpired
    /// session whose token has `session`, oldest first, as its owner reads
    /// it: the operator's messages count as seen from `now`. `None` if
    /// there is no such session. The session is read in the same
    /// transaction, as every write for a signed-in account reads it.
    pub fn feedback_for_session(
        &mut self,
        session: &TokenDigest,
        now: UnixTime,
    ) -> Result<Option<Vec<FeedbackMessage>>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE feedback SET seen_at = ?2
             WHERE account_id = ?1 AND from_operator = 1 AND seen_at IS NULL",
            params![account.0, time_to_sql(now)],
        )?;
        let messages = messages_of(&tx, account)?;
        tx.commit()?;
        Ok(Some(messages))
    }

    /// The conversation of the account with `code`, oldest first, as the
    /// operator reads it; `None` if no account has the code.
    pub fn conversation_with(
        &self,
        code: &AccountCode,
    ) -> Result<Option<Vec<FeedbackMessage>>, StoreError> {
        let account: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM accounts WHERE code = ?1",
                [code.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        account
            .map(|account| messages_of(&self.conn, AccountId(account)))
            .transpose()
    }

    /// Every conversation: those waiting for a reply first, then the
    /// latest first.
    pub fn conversations(&self) -> Result<Vec<Conversation>, StoreError> {
        let rows: Vec<(String, String, i64, bool, i64)> = self
            .conn
            .prepare(
                "SELECT a.code, a.email, last.sent_at, last.from_operator = 0,
                     (SELECT count(*) FROM feedback n WHERE n.account_id = a.id)
                 FROM accounts a
                 JOIN feedback last ON last.id =
                     (SELECT max(f.id) FROM feedback f WHERE f.account_id = a.id)
                 ORDER BY last.from_operator, last.id DESC",
            )?
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(code, email, last_sent_at, awaiting_reply, messages)| {
                Ok(Conversation {
                    account: code_from_sql(&code)?,
                    email: email_from_sql(&email)?,
                    last_sent_at: time_from_sql(last_sent_at, "feedback time")?,
                    awaiting_reply,
                    messages: count_from_sql(messages, "feedback count")?,
                })
            })
            .collect()
    }
}

/// Sends the operator's `body` to the account with `code` on `tx`, as of
/// `now`: the operator pages' reply ([`crate::OperatorActions::reply`]).
pub(crate) fn reply_on(
    tx: &Transaction<'_>,
    code: &AccountCode,
    body: &MessageBody,
    now: UnixTime,
) -> Result<(), ReplyError> {
    let Some((account, _)) = account_by_code(tx, code)? else {
        return Err(UnknownAccount(*code).into());
    };
    add_message(tx, account, true, body, now)?;
    Ok(())
}

/// Adds a message to the conversation of `account` on `tx`, and lets the
/// conversation keep only its latest [`MAX_KEPT`] messages.
fn add_message(
    tx: &Transaction<'_>,
    account: AccountId,
    from_operator: bool,
    body: &MessageBody,
    now: UnixTime,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO feedback (account_id, from_operator, body, sent_at) VALUES (?1, ?2, ?3, ?4)",
        params![account.0, from_operator, body.as_str(), time_to_sql(now)],
    )?;
    tx.execute(
        "DELETE FROM feedback WHERE account_id = ?1 AND id NOT IN (
             SELECT id FROM feedback WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![account.0, MAX_KEPT],
    )?;
    Ok(())
}

/// The conversation of `account`, oldest first, read on `conn`.
fn messages_of(conn: &Connection, account: AccountId) -> Result<Vec<FeedbackMessage>, StoreError> {
    let rows: Vec<(bool, String, i64)> = conn
        .prepare(
            "SELECT from_operator, body, sent_at FROM feedback
             WHERE account_id = ?1 ORDER BY id",
        )?
        .query_map([account.0], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(from_operator, body, sent_at)| {
            Ok(FeedbackMessage {
                from_operator,
                body: MessageBody::parse(&body)
                    .map_err(|error| corrupt("feedback message", error.to_string()))?,
                sent_at: time_from_sql(sent_at, "feedback time")?,
            })
        })
        .collect()
}
