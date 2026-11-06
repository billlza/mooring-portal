//! One user's CFM import document.

use std::fmt;

use serde::Deserialize;

use super::MAX_PROFILE_BYTES;

/// Why a document is not one the portal serves.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    /// Larger than [`MAX_PROFILE_BYTES`].
    #[error("the document has {size} bytes; at most {MAX_PROFILE_BYTES} are accepted")]
    TooLarge {
        /// Its size.
        size: usize,
    },
    /// Not a sing-box node list. Only the position of the problem is kept:
    /// the decoder's own message can quote the document, which holds
    /// credentials.
    #[error(
        "the document is not a sing-box node list (an object whose only key is `outbounds`, \
         a list of objects that each have a `type` and a `tag`); the problem is at line \
         {line}, column {column}"
    )]
    NotNodeList {
        /// The line of the problem, from 1.
        line: usize,
        /// Its column, from 1.
        column: usize,
    },
    /// The list is empty.
    #[error("the document lists no outbound")]
    NoOutbounds,
    /// An outbound's `type` or `tag` is empty.
    #[error("outbound {index} (from 0) has an empty `type` or `tag`")]
    Unnamed {
        /// Its position in the list.
        index: usize,
    },
}

/// The outline the portal checks: the root holds only `outbounds`, which
/// CFM's importer requires of a sing-box node list. Each outbound's keys
/// other than `type` and `tag` are CFM's to judge.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Outline {
    outbounds: Vec<Outbound>,
}

#[derive(Deserialize)]
struct Outbound {
    #[serde(rename = "type")]
    kind: String,
    tag: String,
}

/// A CFM import document the portal may serve, kept as the exact text
/// Mooring wrote. Its `Debug` output shows only its size: it holds
/// credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct CfmProfile(String);

impl CfmProfile {
    /// Checks the outline of `text`.
    pub fn parse(text: &str) -> Result<Self, DocumentError> {
        if text.len() > MAX_PROFILE_BYTES {
            return Err(DocumentError::TooLarge { size: text.len() });
        }
        let outline: Outline =
            serde_json::from_str(text).map_err(|error| DocumentError::NotNodeList {
                line: error.line(),
                column: error.column(),
            })?;
        if outline.outbounds.is_empty() {
            return Err(DocumentError::NoOutbounds);
        }
        if let Some(index) = outline
            .outbounds
            .iter()
            .position(|outbound| outbound.kind.is_empty() || outbound.tag.is_empty())
        {
            return Err(DocumentError::Unnamed { index });
        }
        Ok(Self(text.to_owned()))
    }

    /// The document, exactly as Mooring wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CfmProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CfmProfile({} bytes, redacted)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "a-credential-that-must-not-leak";

    #[test]
    fn a_node_list_is_kept_as_written() {
        let text = format!(
            "{{ \"outbounds\": [ {{\"type\": \"vless\", \"tag\": \"n1-reality\", \"uuid\": \
             \"{SECRET}\"}} ] }}"
        );
        let profile = CfmProfile::parse(&text).unwrap();
        assert_eq!(profile.as_str(), text);
        assert_eq!(
            format!("{profile:?}"),
            format!("CfmProfile({} bytes, redacted)", text.len())
        );
    }

    #[test]
    fn anything_but_a_named_node_list_is_refused() {
        for (text, expected) in [
            ("{\"outbounds\": []}", DocumentError::NoOutbounds),
            (
                "{\"outbounds\": [{\"type\": \"vless\", \"tag\": \"a\"}, {\"type\": \"\", \"tag\": \"b\"}]}",
                DocumentError::Unnamed { index: 1 },
            ),
            (
                "{\"outbounds\": [{\"type\": \"vless\", \"tag\": \"\"}]}",
                DocumentError::Unnamed { index: 0 },
            ),
        ] {
            assert_eq!(CfmProfile::parse(text), Err(expected), "{text}");
        }
        for text in [
            "",
            "[]",
            "\"outbounds\"",
            "{}",
            "{\"outbounds\": [], \"inbounds\": []}",
            "{\"outbounds\": [{\"tag\": \"a\"}]}",
            "{\"outbounds\": [{\"type\": \"vless\"}]}",
            "{\"outbounds\": [{\"type\": 5, \"tag\": \"a\"}]}",
            "{\"outbounds\": [{\"type\": \"vless\", \"tag\": \"a\"}]} trailing",
            "\u{feff}{\"outbounds\": [{\"type\": \"vless\", \"tag\": \"a\"}]}",
        ] {
            assert!(
                matches!(
                    CfmProfile::parse(text),
                    Err(DocumentError::NotNodeList { .. })
                ),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_refusal_names_the_place_but_never_quotes_the_document() {
        let text = format!("{{\n  \"outbounds\": [\"{SECRET}\"]\n}}");
        let error = CfmProfile::parse(&text).unwrap_err();
        assert_eq!(
            error,
            DocumentError::NotNodeList {
                line: 2,
                column: 49
            }
        );
        assert!(!error.to_string().contains(SECRET), "{error}");
        assert!(error.to_string().contains("line 2, column 49"), "{error}");
    }

    #[test]
    fn the_size_limit_is_inclusive() {
        let wrap = |padding: usize| {
            format!(
                "{{\"outbounds\": [{{\"type\": \"vless\", \"tag\": \"a\"}}]}}{}",
                " ".repeat(padding)
            )
        };
        let base = wrap(0).len();
        assert!(CfmProfile::parse(&wrap(MAX_PROFILE_BYTES - base)).is_ok());
        assert_eq!(
            CfmProfile::parse(&wrap(MAX_PROFILE_BYTES - base + 1)),
            Err(DocumentError::TooLarge {
                size: MAX_PROFILE_BYTES + 1
            })
        );
    }
}
