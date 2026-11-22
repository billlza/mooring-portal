//! `translate`: has the host's translator examine what users wrote to the
//! operator.
//!
//! The portal runs no translation model. A service of the host does,
//! behind a socket in the file system (`docs/translation.md`), and it is
//! handed a message's text and nothing else: not whose the message is,
//! nor the database, and it has no network. This command asks it, for
//! each message of a user's that waits, which language the message is
//! written in and how it reads in the languages of the pages, and records
//! the answers. Besides the database, the socket is all it reaches.
//!
//! Each question is a connection: a line that says what is asked and how
//! many bytes of text follow, then the text. The answer is a line, `ok`
//! with the number of bytes that follow it, and then what was asked for,
//! in so many bytes; or the line `none`, where the translator has nothing
//! to say: it cannot tell the language, or has no model for it. The
//! translator then closes the connection. Anything else is a failure, and
//! so is silence past [`PATIENCE`]. A message whose question failed waits
//! for the next run.
//!
//! Both sides say how long their text is, and neither tells it by where
//! the other stops writing. An answer cut short when the translator is
//! stopped half-way is then not taken for a whole one. And the question
//! does not end by closing this side's writing half: on macOS that end
//! was seen not to reach a reader already waiting for it (2026-11-22,
//! both ends waiting for the other), which left the tests of this module
//! to wait out [`PATIENCE`] now and then.
//!
//! The newest message is asked about first, and then what was written
//! while the run went on ([`Queue`]): the operator reads what was just
//! written, and a message the translator fails on every time is asked
//! about after the newer ones, not before them. A message that has waited
//! [`RECENT_SECS`] is asked about no more, and stays as written.

use std::collections::BTreeMap;
use std::io::{self, Read as _, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use portal_core::time::{Clock, SECS_PER_DAY, UnixTime};
use portal_core::translation::{
    Language, MAX_TRANSLATION_CHARS, PageLanguage, TranslatedText, TranslatedTextError,
};
use portal_store::{Examined, FeedbackId, Store, StoreError, Untranslated, Waiting};

use crate::{CommandError, TranslatorError};

/// How long the translator is waited for: to take a question, and to
/// answer it. A model takes a few seconds to load and to write a long
/// message; one that takes minutes is stuck.
pub(crate) const PATIENCE: Duration = Duration::from_secs(120);

/// How many messages are read from the database at a time.
const BATCH: u32 = 50;

/// How long a message waits for the translator at most. One that has
/// waited so long was written while there was no translator, or is one
/// the translator fails on every time: it is asked about no more.
pub(crate) const RECENT_SECS: u64 = 30 * SECS_PER_DAY;

/// After this many questions fail in one run, the run stops: a translator
/// that fails so is broken, and the messages after would fail as well.
pub(crate) const MAX_FAILURES: u32 = 3;

/// The most bytes of an answer that are read: its first line, and the
/// longest translation in the widest characters with as much white space
/// around it again. A longer answer holds no translation short enough to
/// keep.
const MAX_ANSWER_BYTES: u64 = 8 * MAX_TRANSLATION_CHARS as u64 + 32;

/// The translator behind its socket.
#[derive(Debug)]
pub(crate) struct Translator<'a> {
    /// The socket.
    pub(crate) socket: &'a Path,
    /// How long it is waited for.
    pub(crate) patience: Duration,
}

/// What the translator said to a question.
enum Answer {
    /// What was asked for.
    Said(String),
    /// It has nothing to say.
    Nothing,
}

/// What became of a translation that was asked for.
enum Translated {
    /// The translation.
    Text(TranslatedText),
    /// The translator has no model for it.
    Nothing,
    /// What it wrote cannot be shown.
    Refused,
}

impl Translator<'_> {
    /// Asks `question` of `text`.
    fn ask(&self, question: &str, text: &str) -> Result<Answer, TranslatorError> {
        let mut stream = UnixStream::connect(self.socket).map_err(TranslatorError::Unreachable)?;
        let answer = self
            .exchange(&mut stream, question, text)
            .map_err(TranslatorError::Silent)?;
        if answer.len() as u64 > MAX_ANSWER_BYTES {
            return Err(TranslatorError::TooLong);
        }
        let Some(end) = answer.iter().position(|&byte| byte == b'\n') else {
            return Err(TranslatorError::Unexpected);
        };
        let (line, said) = (&answer[..end], &answer[end + 1..]);
        if line == b"none" && said.is_empty() {
            return Ok(Answer::Nothing);
        }
        let length = line
            .strip_prefix(b"ok ")
            .and_then(written_length)
            .ok_or(TranslatorError::Unexpected)?;
        if said.len() != length {
            return Err(TranslatorError::Incomplete);
        }
        String::from_utf8(said.to_vec())
            .map(Answer::Said)
            .map_err(|_| TranslatorError::NotText)
    }

    /// Writes `question` and `text` to `stream`, and reads the answer to
    /// its end, up to one byte more than an answer may hold.
    fn exchange(&self, stream: &mut UnixStream, question: &str, text: &str) -> io::Result<Vec<u8>> {
        stream.set_read_timeout(Some(self.patience))?;
        stream.set_write_timeout(Some(self.patience))?;
        stream.write_all(format!("{question} {}\n", text.len()).as_bytes())?;
        stream.write_all(text.as_bytes())?;
        let mut answer = Vec::new();
        stream.take(MAX_ANSWER_BYTES + 1).read_to_end(&mut answer)?;
        Ok(answer)
    }

    /// The language `text` is written in, if the translator can tell.
    fn language_of(&self, text: &str) -> Result<Option<Language>, TranslatorError> {
        match self.ask("detect", text)? {
            Answer::Said(code) => Language::parse(code.trim())
                .map(Some)
                .map_err(|_| TranslatorError::Unexpected),
            Answer::Nothing => Ok(None),
        }
    }

    /// `text`, written in `from`, in `into`.
    fn translate(
        &self,
        text: &str,
        from: &Language,
        into: PageLanguage,
    ) -> Result<Translated, TranslatorError> {
        match self.ask(&format!("translate {from} {into}"), text) {
            Ok(Answer::Said(text)) => Ok(match TranslatedText::parse(&text) {
                Ok(text) => Translated::Text(text),
                Err(
                    TranslatedTextError::Blank
                    | TranslatedTextError::TooLong
                    | TranslatedTextError::ControlCharacter,
                ) => Translated::Refused,
            }),
            Ok(Answer::Nothing) => Ok(Translated::Nothing),
            // No translation that long is kept, however it ends.
            Err(TranslatorError::TooLong) => Ok(Translated::Refused),
            Err(error) => Err(error),
        }
    }

    /// What the translator makes of `message`, and how many of its
    /// translations cannot be shown.
    ///
    /// Its models go into English and out of it. So a message is put
    /// into English first, unless it is English, and from that English
    /// into every other language of the pages but its own.
    fn examine(&self, message: &Untranslated) -> Result<(Examined, u32), TranslatorError> {
        let text = message.body.as_str();
        let Some(language) = self.language_of(text)? else {
            return Ok((Examined::default(), 0));
        };
        let own = language.page();
        let mut translations = BTreeMap::new();
        let mut refused = 0;
        let english = if own == Some(PageLanguage::English) {
            Some(text.to_owned())
        } else {
            match self.translate(text, &language, PageLanguage::English)? {
                Translated::Text(english) => {
                    let words = english.as_str().to_owned();
                    translations.insert(PageLanguage::English, english);
                    Some(words)
                }
                Translated::Nothing => None,
                Translated::Refused => {
                    refused += 1;
                    None
                }
            }
        };
        if let Some(english) = english {
            let from = Language::from(PageLanguage::English);
            for into in PageLanguage::ALL {
                if into == PageLanguage::English || own == Some(into) {
                    continue;
                }
                match self.translate(&english, &from, into)? {
                    Translated::Text(text) => {
                        translations.insert(into, text);
                    }
                    Translated::Nothing => {}
                    Translated::Refused => refused += 1,
                }
            }
        }
        Ok((
            Examined {
                language: Some(language),
                translations,
            },
            refused,
        ))
    }
}

/// The number of bytes `digits` says follow, written in digits alone.
fn written_length(digits: &[u8]) -> Option<usize> {
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// The messages that wait, in the order a run asks about them: the
/// newest first, down to the oldest, and then those written meanwhile,
/// the same way. Each is handed over once in a run.
struct Queue {
    /// Which of them come next.
    next: Waiting,
    /// The newest handed over since the newest was last asked for.
    newest: Option<FeedbackId>,
}

impl Queue {
    /// The messages sent at `since` or after it.
    const fn new(since: UnixTime) -> Self {
        Self {
            next: Waiting::since(since),
            newest: None,
        }
    }

    /// The next of them, [`BATCH`] at most; none when no more wait.
    fn next(&mut self, store: &Store) -> Result<Vec<Untranslated>, StoreError> {
        loop {
            let waiting = store.untranslated(&self.next, BATCH)?;
            if let (Some(newest), Some(oldest)) = (waiting.first(), waiting.last()) {
                self.newest.get_or_insert(newest.id);
                self.next.older_than = Some(oldest.id);
                return Ok(waiting);
            }
            // Down to the oldest. What was written since the newest was
            // asked for is above every message handed over so far.
            let Some(newest) = self.newest.take() else {
                return Ok(waiting);
            };
            self.next.newer_than = newest;
            self.next.older_than = None;
        }
    }
}

/// What a run did.
#[derive(Debug, Default)]
struct Tally {
    /// Messages examined and recorded.
    examined: u64,
    /// Translations recorded.
    translations: u64,
    /// Translations that cannot be shown, and were not kept.
    refused: u64,
    /// Messages deleted while they were examined, or examined by another
    /// run meanwhile.
    gone: u64,
    /// Messages whose question failed.
    failed: u32,
}

/// `translate`: has the translator behind `socket` examine every message
/// of a user's that waits for it.
pub(crate) fn translate(
    store: &mut Store,
    clock: &dyn Clock,
    socket: &Path,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    run(
        store,
        clock,
        &Translator {
            socket,
            patience: PATIENCE,
        },
        out,
    )
}

/// [`translate`], by `translator`.
pub(crate) fn run(
    store: &mut Store,
    clock: &dyn Clock,
    translator: &Translator<'_>,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let mut tally = Tally::default();
    let mut first_failure = None;
    let since = UnixTime::from_secs(clock.now()?.as_secs().saturating_sub(RECENT_SECS))?;
    // Each message is asked about once in a run: one that fails is
    // passed, and waits for the next run.
    let mut queue = Queue::new(since);
    'run: loop {
        let waiting = queue.next(store)?;
        if waiting.is_empty() {
            break;
        }
        for message in &waiting {
            match translator.examine(message) {
                Ok((examined, refused)) => {
                    if store.record_examined(message, &examined, clock.now()?)? {
                        tally.examined += 1;
                        tally.translations += examined.translations.len() as u64;
                        tally.refused += u64::from(refused);
                    } else {
                        tally.gone += 1;
                    }
                }
                Err(error) => {
                    tally.failed += 1;
                    // With no translator to reach, no message after this
                    // one fares better.
                    let hopeless = matches!(error, TranslatorError::Unreachable(_))
                        || tally.failed == MAX_FAILURES;
                    first_failure.get_or_insert(error);
                    if hopeless {
                        break 'run;
                    }
                }
            }
        }
    }
    report(&tally, out)?;
    match first_failure {
        None => Ok(()),
        Some(source) => Err(CommandError::Translator {
            socket: translator.socket.to_owned(),
            failed: tally.failed,
            source,
        }),
    }
}

/// Says what a run did. Nothing of any message is said: the host keeps
/// what this prints longer than the portal keeps a message.
fn report(tally: &Tally, out: &mut dyn Write) -> io::Result<()> {
    if tally.examined == 0 && tally.gone == 0 && tally.failed == 0 {
        return writeln!(out, "no message waits for the translator");
    }
    write!(
        out,
        "examined {} message(s) and wrote {} translation(s)",
        tally.examined, tally.translations
    )?;
    if tally.refused > 0 {
        write!(
            out,
            "; {} translation(s) could not be shown and were not kept",
            tally.refused
        )?;
    }
    if tally.gone > 0 {
        write!(
            out,
            "; {} message(s) had been deleted or examined meanwhile",
            tally.gone
        )?;
    }
    writeln!(out)
}
