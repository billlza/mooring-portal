//! Translations of what users write to the operator.
//!
//! The operator reads every conversation in the language of their pages:
//! where a user wrote in another, a translation stands under the message,
//! marked as a machine's. A translator on the portal's own host makes it
//! (`docs/translation.md`). The message is never changed, and its writer
//! never sees the translation.
//!
//! A translation is text a program wrote, so it is held to what a
//! message is held to: no control characters but line breaks and tabs,
//! none of the controls of the direction of text, and a bound on its
//! length.

use std::fmt;

use crate::feedback::{MAX_MESSAGE_CHARS, holds_control, tidy};

/// The most characters a translation may hold. A message of Chinese
/// grows several times as long in English, a few letters for each
/// character; six times the longest message leaves room for any, and
/// still bounds what a translator gone wrong can store.
pub const MAX_TRANSLATION_CHARS: usize = 6 * MAX_MESSAGE_CHARS;

/// A language the pages come in, which is what a message is translated
/// into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PageLanguage {
    /// Simplified Chinese.
    Chinese,
    /// English.
    English,
}

impl PageLanguage {
    /// Every language the pages come in.
    pub const ALL: [Self; 2] = [Self::Chinese, Self::English];

    /// Its code, as a detector names the language and as the store keeps
    /// it.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }

    /// The language with `code`, if the pages come in it.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }
}

impl fmt::Display for PageLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// The language a message is written in, by its ISO 639 code: two or
/// three lowercase letters, such as `ja` or `yue`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Language(String);

/// A language's code that is not two or three lowercase letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a language's code is two or three lowercase letters")]
pub struct LanguageError;

impl Language {
    /// The language with `code`.
    ///
    /// # Errors
    ///
    /// [`LanguageError`] unless `code` is two or three lowercase ASCII
    /// letters.
    pub fn parse(code: &str) -> Result<Self, LanguageError> {
        if matches!(code.len(), 2 | 3) && code.bytes().all(|byte| byte.is_ascii_lowercase()) {
            Ok(Self(code.to_owned()))
        } else {
            Err(LanguageError)
        }
    }

    /// The code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The language of the pages this is, if it is one.
    #[must_use]
    pub fn page(&self) -> Option<PageLanguage> {
        PageLanguage::from_code(&self.0)
    }
}

impl From<PageLanguage> for Language {
    fn from(language: PageLanguage) -> Self {
        Self(language.code().to_owned())
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A translation's text: not blank, at most [`MAX_TRANSLATION_CHARS`]
/// characters, with its line breaks written `\n` and no other control
/// character but a tab, nor any control of the direction of text. Leading
/// and trailing white space is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedText(String);

/// Why a translation's text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TranslatedTextError {
    /// Nothing but white space.
    #[error("the translation is empty")]
    Blank,
    /// More than [`MAX_TRANSLATION_CHARS`] characters.
    #[error("the translation is longer than {MAX_TRANSLATION_CHARS} characters")]
    TooLong,
    /// A control character other than a line break or a tab, or a
    /// control of the direction of text.
    #[error("the translation holds a control character")]
    ControlCharacter,
}

impl TranslatedText {
    /// The translation `text` writes.
    ///
    /// # Errors
    ///
    /// [`TranslatedTextError`] if the text is blank, too long, or holds a
    /// control character other than a line break or a tab, or a control
    /// of the direction of text.
    pub fn parse(text: &str) -> Result<Self, TranslatedTextError> {
        let text = tidy(text);
        if text.is_empty() {
            return Err(TranslatedTextError::Blank);
        }
        if text.chars().count() > MAX_TRANSLATION_CHARS {
            return Err(TranslatedTextError::TooLong);
        }
        if holds_control(&text) {
            return Err(TranslatedTextError::ControlCharacter);
        }
        Ok(Self(text))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TranslatedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_come_in_chinese_and_english() {
        assert_eq!(
            PageLanguage::ALL.map(PageLanguage::code),
            ["zh", "en"],
            "the store's check names the same two"
        );
        for language in PageLanguage::ALL {
            assert_eq!(PageLanguage::from_code(language.code()), Some(language));
            assert_eq!(language.to_string(), language.code());
            assert_eq!(Language::from(language).page(), Some(language));
        }
        for other in ["", "ja", "ZH", "zh-CN", "eng"] {
            assert_eq!(PageLanguage::from_code(other), None, "{other:?}");
        }
    }

    #[test]
    fn a_language_is_two_or_three_lowercase_letters() {
        for code in ["ja", "en", "zh", "yue", "ceb"] {
            let language = Language::parse(code).unwrap();
            assert_eq!(language.as_str(), code);
            assert_eq!(language.to_string(), code);
        }
        assert_eq!(Language::parse("ja").unwrap().page(), None);
        assert_eq!(
            Language::parse("zh").unwrap().page(),
            Some(PageLanguage::Chinese)
        );
        for refused in [
            "", "j", "japn", "JA", "Ja", "j1", "zh-CN", "zh_cn", " ja", "ja\n", "日本", "é",
        ] {
            assert_eq!(Language::parse(refused), Err(LanguageError), "{refused:?}");
        }
        assert_eq!(
            LanguageError.to_string(),
            "a language's code is two or three lowercase letters"
        );
    }

    #[test]
    fn a_translation_is_held_to_what_a_message_is() {
        let text = TranslatedText::parse(
            "  The link does not open.\r\n\r\nNot on another node either.\tEnd\n ",
        )
        .unwrap();
        assert_eq!(
            text.as_str(),
            "The link does not open.\n\nNot on another node either.\tEnd"
        );
        assert_eq!(text.to_string(), text.as_str());
        for blank in ["", "  ", "\r\n\t"] {
            assert_eq!(
                TranslatedText::parse(blank),
                Err(TranslatedTextError::Blank)
            );
        }
        // As long as its own bound, which is several messages long, and
        // no longer.
        let longest = "w".repeat(MAX_TRANSLATION_CHARS);
        assert_eq!(MAX_TRANSLATION_CHARS, 6 * MAX_MESSAGE_CHARS);
        assert_eq!(TranslatedText::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            TranslatedText::parse(&format!("{longest}w")),
            Err(TranslatedTextError::TooLong)
        );
        // Characters are counted, not bytes.
        assert!(TranslatedText::parse(&"字".repeat(MAX_TRANSLATION_CHARS)).is_ok());
        for controlled in ["a\u{1b}[31mb", "a\u{0}b", "a\u{202e}b", "a\u{2066}b"] {
            assert_eq!(
                TranslatedText::parse(controlled),
                Err(TranslatedTextError::ControlCharacter),
                "{controlled:?}"
            );
        }
        assert_eq!(
            TranslatedTextError::TooLong.to_string(),
            "the translation is longer than 6000 characters"
        );
        assert_eq!(
            TranslatedTextError::Blank.to_string(),
            "the translation is empty"
        );
        assert_eq!(
            TranslatedTextError::ControlCharacter.to_string(),
            "the translation holds a control character"
        );
    }
}
