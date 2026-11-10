//! What is wrong with a JSON document that does not decode.
//!
//! The decoder's own messages quote the value they reject. The portal's
//! documents hold credentials, or come from the network, and a refusal is
//! printed to the operator's terminal or to a service's log; so only the
//! kind of a problem is kept.

use std::fmt;

use serde_json::error::Category;

/// What is wrong with a document that does not decode.
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
    pub(crate) fn of(error: &serde_json::Error) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_problem_is_told_apart_and_named() {
        let kind =
            |text: &str| Malformation::of(&serde_json::from_str::<Vec<u8>>(text).unwrap_err());
        assert_eq!(kind("[1,"), Malformation::Truncated);
        assert_eq!(kind("[1 2]"), Malformation::Syntax);
        assert_eq!(kind("[\"a\"]"), Malformation::Shape);
        assert_eq!(Malformation::Syntax.to_string(), "it is not JSON");
        assert_eq!(Malformation::Truncated.to_string(), "it ends too early");
        assert_eq!(Malformation::Unreadable.to_string(), "it could not be read");
        assert!(
            Malformation::Shape
                .to_string()
                .starts_with("a key is missing")
        );
    }
}
