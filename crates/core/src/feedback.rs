//! Feedback: what a signed-in user writes to the operator from the site,
//! and what the operator writes back. Each account has one conversation
//! with the operator, read on the site: the site sends no email.
//!
//! A message is plain text. The site shows it escaped, as it shows
//! everything a user wrote, and keeps it free of control characters other
//! than line breaks and tabs, so that it reads the same wherever it is
//! shown. The controls of the direction text runs in count among them:
//! with them, a message could show its words in another order than the
//! one they were written in.

use std::fmt;

/// The most characters a message may hold. A form carries each byte of a
/// character outside ASCII as three, so the longest message in the widest
/// characters, four bytes each, still fits in the form the site accepts.
pub const MAX_MESSAGE_CHARS: usize = 1000;

/// A message's text: not blank, at most [`MAX_MESSAGE_CHARS`] characters,
/// with its line breaks written `\n` and no other control character but a
/// tab, nor any of Unicode's controls of the direction of text
/// (`Bidi_Control`). Leading and trailing white space is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBody(String);

/// Why a message's text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MessageBodyError {
    /// Nothing but white space.
    #[error("the message is empty")]
    Blank,
    /// More than [`MAX_MESSAGE_CHARS`] characters.
    #[error("the message is longer than {MAX_MESSAGE_CHARS} characters")]
    TooLong,
    /// A control character other than a line break or a tab, or a
    /// control of the direction of text.
    #[error("the message holds a control character")]
    ControlCharacter,
}

/// Whether `c` is one of Unicode's controls of the direction of text, the
/// characters with the property `Bidi_Control`: the marks, embeddings,
/// overrides and isolates that reorder what a reader sees.
const fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// `text` as the site keeps what was written: every line break `\n`, as
/// browsers send `\r\n`, and no white space around it.
pub(crate) fn tidy(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

/// Whether `text` holds a control character other than a line break or a
/// tab, or a control of the direction of text: what no text the site
/// shows may hold.
pub(crate) fn holds_control(text: &str) -> bool {
    text.chars()
        .any(|c| (c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c))
}

impl MessageBody {
    /// The message `text` writes. Browsers send a form's line breaks as
    /// `\r\n`; every line break is kept as `\n`.
    ///
    /// # Errors
    ///
    /// [`MessageBodyError`] if the text is blank, too long, or holds a
    /// control character other than a line break or a tab, or a control
    /// of the direction of text.
    pub fn parse(text: &str) -> Result<Self, MessageBodyError> {
        let text = tidy(text);
        if text.is_empty() {
            return Err(MessageBodyError::Blank);
        }
        if text.chars().count() > MAX_MESSAGE_CHARS {
            return Err(MessageBodyError::TooLong);
        }
        if holds_control(&text) {
            return Err(MessageBodyError::ControlCharacter);
        }
        Ok(Self(text))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_keeps_its_lines_without_the_space_around_them() {
        let body =
            MessageBody::parse("  订阅链接打不开\r\n\r\n换了节点也不行\r第三行\t完\n ").unwrap();
        assert_eq!(
            body.as_str(),
            "订阅链接打不开\n\n换了节点也不行\n第三行\t完"
        );
        assert_eq!(body.to_string(), body.as_str());
    }

    #[test]
    fn a_blank_long_or_controlled_message_is_refused() {
        for blank in ["", "  ", "\r\n\t"] {
            assert_eq!(MessageBody::parse(blank), Err(MessageBodyError::Blank));
        }
        let longest = "字".repeat(MAX_MESSAGE_CHARS);
        assert_eq!(MessageBody::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            MessageBody::parse(&format!("{longest}字")),
            Err(MessageBodyError::TooLong)
        );
        // Characters are counted, not bytes, and only after the space
        // around the text is dropped.
        assert!(MessageBody::parse(&format!(" {longest} ")).is_ok());
        // The controls of direction too: with "\u{202e}", "evil.exe" could
        // show as an image's name.
        for controlled in [
            "a\u{1b}[31mb",
            "a\u{0}b",
            "a\u{7f}b",
            "a\u{85}b",
            "a\u{61c}b",
            "a\u{200e}b",
            "a\u{200f}b",
            "a\u{202a}b",
            "photo\u{202e}gpj.exe",
            "a\u{2066}b",
            "a\u{2069}b",
        ] {
            assert_eq!(
                MessageBody::parse(controlled),
                Err(MessageBodyError::ControlCharacter),
                "{controlled:?}"
            );
        }
        // What only joins or separates characters stays: an emoji's joiner,
        // a space that does not break, a character of another script.
        for kept in [
            "\u{1f469}\u{200d}\u{1f4bb}",
            "a\u{a0}b",
            "a\u{200b}b",
            "\u{5e9}\u{5dc}\u{5d5}\u{5dd}",
            "a\u{2029}b",
        ] {
            assert_eq!(MessageBody::parse(kept).unwrap().as_str(), kept, "{kept:?}");
        }
        assert_eq!(
            MessageBodyError::TooLong.to_string(),
            "the message is longer than 1000 characters"
        );
    }
}
