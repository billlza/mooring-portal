//! The password policy, and wrappers that keep passwords out of logs.
//!
//! The policy follows NIST SP 800-63B and OWASP ASVS: a length floor and
//! ceiling, no composition rules, and no password equal to the login name.
//! Passwords are taken verbatim (no trimming or normalisation), so what the
//! user typed is what is hashed.

use std::fmt;

use super::Email;

/// The fewest characters (Unicode scalar values) a new password may have.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// The most characters a new password may have.
pub const MAX_PASSWORD_CHARS: usize = 128;

/// Why a new password was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    /// Fewer than [`MIN_PASSWORD_CHARS`] characters.
    #[error("the password has fewer than {MIN_PASSWORD_CHARS} characters")]
    TooShort,
    /// More than [`MAX_PASSWORD_CHARS`] characters.
    #[error("the password has more than {MAX_PASSWORD_CHARS} characters")]
    TooLong,
    /// A control character, such as a line break, which no keyboard types
    /// into a password field.
    #[error("the password contains a control character")]
    ControlCharacter,
    /// The email address, or the part of it before `@`, in any case.
    #[error("the password is the email address")]
    MatchesEmail,
}

/// A password that satisfies the policy, ready to hash. Its `Debug` output
/// is redacted.
#[derive(Clone)]
pub struct NewPassword(String);

impl NewPassword {
    /// Checks `text` against the policy for the account `email`.
    pub fn parse(text: String, email: &Email) -> Result<Self, PasswordError> {
        if text.chars().any(char::is_control) {
            return Err(PasswordError::ControlCharacter);
        }
        let chars = text.chars().count();
        if chars < MIN_PASSWORD_CHARS {
            return Err(PasswordError::TooShort);
        }
        if chars > MAX_PASSWORD_CHARS {
            return Err(PasswordError::TooLong);
        }
        let lowered = text.to_lowercase();
        if lowered == email.as_str() || lowered == email.local_part() {
            return Err(PasswordError::MatchesEmail);
        }
        Ok(Self(text))
    }

    /// The password, for the hasher only.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NewPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NewPassword(<redacted>)")
    }
}

/// A password typed to log in or to confirm a change, checked only against
/// a stored hash (never against the policy, which may have changed since
/// the password was set). Its `Debug` output is redacted.
#[derive(Clone)]
pub struct PasswordAttempt(String);

impl PasswordAttempt {
    /// Wraps the typed text verbatim.
    #[must_use]
    pub const fn new(text: String) -> Self {
        Self(text)
    }

    /// The attempt, for the hasher only.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PasswordAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordAttempt(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email() -> Email {
        Email::parse("lilei@qq.com").unwrap()
    }

    #[test]
    fn length_is_counted_in_characters() {
        assert_eq!(
            NewPassword::parse("a".repeat(11), &email()).unwrap_err(),
            PasswordError::TooShort
        );
        assert!(NewPassword::parse("a".repeat(12), &email()).is_ok());
        // Twelve CJK characters are twelve characters, though 36 bytes.
        assert!(NewPassword::parse("夜里停泊的船亮着的那盏灯".to_owned(), &email()).is_ok());
        assert_eq!(
            NewPassword::parse("夜里停泊的船亮着的那盏".to_owned(), &email()).unwrap_err(),
            PasswordError::TooShort
        );
        assert!(NewPassword::parse("b".repeat(128), &email()).is_ok());
        assert_eq!(
            NewPassword::parse("b".repeat(129), &email()).unwrap_err(),
            PasswordError::TooLong
        );
    }

    #[test]
    fn spaces_are_kept_and_control_characters_refused() {
        let spaced = NewPassword::parse("  correct horse  ".to_owned(), &email()).unwrap();
        assert_eq!(spaced.expose(), "  correct horse  ");
        for bad in [
            "correct horse\nbattery",
            "correct\thorse battery",
            "correct horse\u{7f}battery",
        ] {
            assert_eq!(
                NewPassword::parse(bad.to_owned(), &email()).unwrap_err(),
                PasswordError::ControlCharacter,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_email_is_not_a_password() {
        let long = Email::parse("li.lei.portal.user@qq.com").unwrap();
        for bad in [
            "li.lei.portal.user@qq.com",
            "LI.LEI.PORTAL.USER@QQ.COM",
            "li.lei.portal.user",
        ] {
            assert_eq!(
                NewPassword::parse(bad.to_owned(), &long).unwrap_err(),
                PasswordError::MatchesEmail,
                "{bad}"
            );
        }
        assert!(NewPassword::parse("li.lei.portal.user!".to_owned(), &long).is_ok());
    }

    #[test]
    fn debug_output_is_redacted() {
        let password = NewPassword::parse("correct horse battery".to_owned(), &email()).unwrap();
        assert_eq!(format!("{password:?}"), "NewPassword(<redacted>)");
        let attempt = PasswordAttempt::new("hunter2hunter2".to_owned());
        assert_eq!(format!("{attempt:?}"), "PasswordAttempt(<redacted>)");
        assert_eq!(attempt.expose(), "hunter2hunter2");
    }
}
