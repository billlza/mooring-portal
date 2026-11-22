//! Translations of what users write to the operator
//! (`portal_core::translation`).
//!
//! A translator on the host examines each message a user writes, once
//! (`docs/translation.md`): it finds the language the message is written
//! in, and writes the message in the languages of the pages it is not
//! written in. What it found and what it wrote are the message's: they
//! are deleted with it, and so with its conversation and its account. The
//! operator's own messages are not examined.
//!
//! The messages that wait for it are handed over the newest first
//! ([`Waiting`]): the operator reads what was just written, and a message
//! the translator keeps failing on is then asked about after the newer
//! ones, never before them.
//!
//! Every translation has a number, larger for each later translation
//! ([`TranslationId`]), so that a page kept open asks only for those made
//! after the last it shows. A number is never used twice: a translation
//! is deleted with its message, which may be after later ones were made,
//! and a number used again would be one a page had passed. The page is
//! given a conversation's messages and their translations as of one
//! moment ([`Store::conversation_translated`]): a translation is made
//! after its message, and a page given the two apart could be given a
//! translation, and pass its number, before the message it belongs under.

use std::collections::BTreeMap;

use portal_core::account::AccountCode;
use portal_core::feedback::MessageBody;
use portal_core::time::UnixTime;
use portal_core::translation::{Language, PageLanguage, TranslatedText};
use rusqlite::{Connection, TransactionBehavior, params};

use crate::bindings::account_by_code;
use crate::feedback::{id_from_sql, messages_of};
use crate::rows::{corrupt, time_to_sql};
use crate::{AccountId, FeedbackId, FeedbackMessage, Store, StoreError};

/// A translation's number, or a place among translations: every
/// translation made after it has a larger number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TranslationId(u64);

impl TranslationId {
    /// Before every translation.
    pub const START: Self = Self(0);

    /// The place after translation number `number`, as a page wrote it
    /// back.
    #[must_use]
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Which of the messages that wait for the translator are asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting {
    /// Those numbered above this.
    pub newer_than: FeedbackId,
    /// Those numbered below this; however they are numbered, if `None`.
    pub older_than: Option<FeedbackId>,
    /// Those sent at this time or after it.
    pub since: UnixTime,
}

impl Waiting {
    /// Every message sent at `since` or after it.
    #[must_use]
    pub const fn since(since: UnixTime) -> Self {
        Self {
            newer_than: FeedbackId::START,
            older_than: None,
            since,
        }
    }
}

/// A user's message that the translator has not examined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untranslated {
    /// Its number.
    pub id: FeedbackId,
    /// Its text.
    pub body: MessageBody,
}

/// What the translator made of a message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Examined {
    /// The language it found the message written in; `None` where it
    /// could not tell.
    pub language: Option<Language>,
    /// The message in languages of the pages: one text for each, at most.
    pub translations: BTreeMap<PageLanguage, TranslatedText>,
}

/// A translation of a message, as the operator's page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    /// Its number.
    pub id: TranslationId,
    /// The number of the message it translates.
    pub message: FeedbackId,
    /// Its text.
    pub text: TranslatedText,
}

/// Messages of a conversation and translations of its messages, as the
/// operator's page of it reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedConversation {
    /// The messages, oldest first.
    pub messages: Vec<FeedbackMessage>,
    /// The translations, in the order they were made.
    pub translations: Vec<Translation>,
}

impl Store {
    /// The users' messages that the translator has not examined, those
    /// `which` asks for: the newest first, `limit` at most.
    pub fn untranslated(
        &self,
        which: &Waiting,
        limit: u32,
    ) -> Result<Vec<Untranslated>, StoreError> {
        // As for a conversation's messages: there is none above a number
        // larger than any row's, and every row is below such a number. A
        // row numbered below 1, which SQLite never numbers so, is read
        // whatever is asked for, so that it is reported rather than left
        // out.
        let newer_than = i64::try_from(which.newer_than.get()).unwrap_or(i64::MAX);
        let older_than = which.older_than.and_then(|id| i64::try_from(id.get()).ok());
        let rows: Vec<(i64, String)> = self
            .conn
            .prepare(
                "SELECT f.id, f.body FROM feedback f
                 WHERE f.from_operator = 0 AND (f.id > ?1 OR f.id < 1)
                     AND (?2 IS NULL OR f.id < ?2) AND f.sent_at >= ?3 AND NOT EXISTS
                     (SELECT 1 FROM feedback_examined e WHERE e.feedback_id = f.id)
                 ORDER BY f.id DESC LIMIT ?4",
            )?
            .query_map(
                params![newer_than, older_than, time_to_sql(which.since), limit],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, body)| {
                Ok(Untranslated {
                    id: id_from_sql(id)?,
                    body: MessageBody::parse(&body)
                        .map_err(|error| corrupt("feedback message", error.to_string()))?,
                })
            })
            .collect()
    }

    /// Records what the translator made of `message`, as of `now`, if the
    /// message still waits for it: whether it did.
    ///
    /// The message is found by its number and its text together. By the
    /// time its translation is made, the number alone could be another
    /// message's: when the latest message of all is deleted with its
    /// account, the next message written takes its number. A message
    /// that is gone, or was examined meanwhile, is left as it is.
    pub fn record_examined(
        &mut self,
        message: &Untranslated,
        examined: &Examined,
        now: UnixTime,
    ) -> Result<bool, StoreError> {
        // No row has a number above `i64::MAX`.
        let Ok(id) = i64::try_from(message.id.get()) else {
            return Ok(false);
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let waits: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM feedback f
                 WHERE f.id = ?1 AND f.from_operator = 0 AND f.body = ?2 AND NOT EXISTS
                     (SELECT 1 FROM feedback_examined e WHERE e.feedback_id = f.id))",
            params![id, message.body.as_str()],
            |row| row.get(0),
        )?;
        if !waits {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO feedback_examined (feedback_id, language, examined_at)
             VALUES (?1, ?2, ?3)",
            params![
                id,
                examined.language.as_ref().map(Language::as_str),
                time_to_sql(now)
            ],
        )?;
        for (language, text) in &examined.translations {
            tx.execute(
                "INSERT INTO feedback_translations (feedback_id, language, body, made_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, language.code(), text.as_str(), time_to_sql(now)],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// The messages after `after` of the conversation of the account with
    /// `code`, and the translations into `language` of the conversation's
    /// messages, those numbered after `translated`; `None` if no account
    /// has the code.
    ///
    /// Both are of one moment: each translation among them is of a
    /// message among them, or of one numbered `after` or before it. A
    /// page that follows the conversation by the two numbers has
    /// therefore been given the message of every translation it is given.
    pub fn conversation_translated(
        &mut self,
        code: &AccountCode,
        after: FeedbackId,
        language: PageLanguage,
        translated: TranslationId,
    ) -> Result<Option<TranslatedConversation>, StoreError> {
        // It writes nothing: what it reads is the database as its first
        // read finds it, whatever is written meanwhile.
        let tx = self.conn.transaction()?;
        let conversation = match account_by_code(&tx, code)? {
            Some((account, _)) => Some(TranslatedConversation {
                messages: messages_of(&tx, account, after)?,
                translations: translations_of(&tx, account, language, translated)?,
            }),
            None => None,
        };
        tx.commit()?;
        Ok(conversation)
    }
}

/// The translations into `language` of the messages of `account`, those
/// numbered after `after`, in the order they were made.
fn translations_of(
    conn: &Connection,
    account: AccountId,
    language: PageLanguage,
    after: TranslationId,
) -> Result<Vec<Translation>, StoreError> {
    // As for a conversation's messages: there is none after a number
    // larger than any row's, and a row numbered below 1 is read whatever
    // is asked for, so that it is reported rather than left out.
    let after = i64::try_from(after.0).unwrap_or(i64::MAX);
    let rows: Vec<(i64, i64, String)> = conn
        .prepare(
            "SELECT t.id, t.feedback_id, t.body FROM feedback_translations t
             JOIN feedback f ON f.id = t.feedback_id
             WHERE f.account_id = ?1 AND t.language = ?2 AND (t.id > ?3 OR t.id < 1)
             ORDER BY t.id",
        )?
        .query_map(params![account.0, language.code(), after], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(id, message, body)| {
            Ok(Translation {
                id: translation_id_from_sql(id)?,
                message: id_from_sql(message)?,
                text: TranslatedText::parse(&body)
                    .map_err(|error| corrupt("translation", error.to_string()))?,
            })
        })
        .collect()
}

/// A translation's number read back: a row's id, which SQLite makes
/// positive.
fn translation_id_from_sql(id: i64) -> Result<TranslationId, StoreError> {
    u64::try_from(id)
        .ok()
        .filter(|&id| id > 0)
        .map(TranslationId)
        .ok_or_else(|| corrupt("translation number", format!("{id} is not positive")))
}
