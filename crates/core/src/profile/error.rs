//! Why a profile bundle was refused.
//!
//! No refusal carries text from the file: a bundle holds credentials, and
//! a refusal is printed to the operator's terminal, or to a service's log
//! when an import is scheduled. The decoder's own messages quote the value
//! they reject, so only the kind and the position of a problem are kept.

use std::fmt;

use serde_json::error::Category;

use super::document::DocumentError;
use super::{FORMAT, MAX_BUNDLE_BYTES, VERSION};
use crate::snapshot::{self, MooringUserId};

/// What is wrong with a bundle that does not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformation {
    /// It is not JSON.
    Syntax,
    /// It is JSON, but a key is missing or unknown, or a value is of the
    /// wrong type or invalid.
    Shape,
    /// It ends too early.
    Truncated,
    /// The decoder failed to read it.
    Unreadable,
}

impl Malformation {
    /// The kind of `error`.
    pub(super) fn of(error: &serde_json::Error) -> Self {
        match error.classify() {
            Category::Syntax => Self::Syntax,
            Category::Data => Self::Shape,
            Category::Eof => Self::Truncated,
            Category::Io => Self::Unreadable,
        }
    }
}

impl fmt::Display for Malformation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "it is not JSON",
            Self::Shape => {
                "a key is missing or unknown, or a value is of the wrong type or invalid"
            }
            Self::Truncated => "it ends too early",
            Self::Unreadable => "it could not be read",
        })
    }
}

/// The format a bundle of the wrong format declares, as far as the portal
/// names it: another format's text is not repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundFormat {
    /// A portal snapshot, imported with `import` instead.
    Snapshot,
    /// Anything else.
    Other,
}

impl FoundFormat {
    /// What `format` names.
    pub(super) fn of(format: &str) -> Self {
        if format == snapshot::FORMAT {
            Self::Snapshot
        } else {
            Self::Other
        }
    }
}

impl fmt::Display for FoundFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot => write!(f, "{:?}", snapshot::FORMAT),
            Self::Other => f.write_str("another format"),
        }
    }
}

/// Why a profile bundle was refused. Problems inside a user's document
/// arrive as [`ProfileError::Document`].
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// Larger than [`MAX_BUNDLE_BYTES`].
    #[error("the bundle is larger than the {MAX_BUNDLE_BYTES} bytes a profile bundle may have")]
    TooLarge {
        /// How many bytes were offered (a bounded reader stops early).
        size: usize,
    },
    /// Not JSON of the expected shape.
    #[error("the file is not a valid profile bundle: {kind}, at line {line}, column {column}")]
    Malformed {
        /// What is wrong.
        kind: Malformation,
        /// The line of the problem, from 1.
        line: usize,
        /// Its column, from 1.
        column: usize,
    },
    /// Some other JSON document.
    #[error("the file's format is {found}, not {FORMAT:?}")]
    WrongFormat {
        /// The format it declares.
        found: FoundFormat,
    },
    /// A version this build cannot read.
    #[error("the file is profile bundle version {found}; this build reads version {VERSION}")]
    UnsupportedVersion {
        /// The `version` it carries.
        found: u32,
    },
    /// No profiles.
    #[error("the bundle lists no profiles")]
    NoProfiles,
    /// The same user twice.
    #[error("user {user} is listed more than once")]
    DuplicateUser {
        /// The user.
        user: MooringUserId,
    },
    /// A user's document is not one the portal serves.
    #[error("the document of user {user} is refused: {reason}")]
    Document {
        /// The user.
        user: MooringUserId,
        /// Why.
        #[source]
        reason: DocumentError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileBundle;

    #[test]
    fn only_known_formats_are_named() {
        assert_eq!(
            FoundFormat::of("mooring-portal-snapshot"),
            FoundFormat::Snapshot
        );
        let other = FoundFormat::of("a-credential-that-must-not-leak");
        assert_eq!(other, FoundFormat::Other);
        let error = ProfileError::WrongFormat { found: other };
        assert_eq!(
            error.to_string(),
            "the file's format is another format, not \"mooring-portal-cfm-profiles\""
        );
    }

    #[test]
    fn a_bundle_that_does_not_decode_is_named_by_kind_and_place() {
        for (text, kind, line, column) in [
            ("not json", Malformation::Syntax, 1, 2),
            ("{\"format\": ", Malformation::Truncated, 1, 11),
            (
                "\"a-credential-that-must-not-leak\"",
                Malformation::Shape,
                1,
                33,
            ),
        ] {
            let error = ProfileBundle::parse(text.as_bytes()).unwrap_err();
            assert!(
                matches!(
                    error,
                    ProfileError::Malformed { kind: k, line: l, column: c }
                        if k == kind && l == line && c == column
                ),
                "{text}: {error}"
            );
            assert!(!error.to_string().contains("credential"), "{error}");
        }
        assert_eq!(
            ProfileError::Malformed {
                kind: Malformation::Shape,
                line: 3,
                column: 9
            }
            .to_string(),
            "the file is not a valid profile bundle: a key is missing or unknown, or a value is \
             of the wrong type or invalid, at line 3, column 9"
        );
        for (kind, text) in [
            (Malformation::Syntax, "it is not JSON"),
            (Malformation::Truncated, "it ends too early"),
            (Malformation::Unreadable, "it could not be read"),
        ] {
            assert_eq!(kind.to_string(), text);
        }
    }
}
