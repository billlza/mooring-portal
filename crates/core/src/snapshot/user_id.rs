//! Mooring user ids as the contract carries them.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The RFC 4648 base32 alphabet in lowercase, as Mooring writes ids.
const BASE32_LOWER: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
/// Characters in an id.
const LEN: usize = 10;

/// A Mooring user id: 10 lowercase RFC 4648 base32 characters (Mooring's
/// `UserId`).
///
/// It is the key the operator binds a portal account to. The contract does
/// not use the user's public alias: Mooring's user agreement (revision 1,
/// clause 4) promises that the alias never leaves the operator's computer,
/// whereas the id already appears in the servers' records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MooringUserId(String);

/// Text that is not a Mooring user id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{text:?} is not a Mooring user id (10 lowercase base32 characters)")]
pub struct MooringUserIdError {
    /// The refused text.
    pub text: String,
}

impl MooringUserId {
    /// Validates `text` as an id.
    pub fn parse(text: &str) -> Result<Self, MooringUserIdError> {
        let valid = text.len() == LEN && text.bytes().all(|byte| BASE32_LOWER.contains(&byte));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(MooringUserIdError {
                text: text.to_owned(),
            })
        }
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MooringUserId {
    type Error = MooringUserIdError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<MooringUserId> for String {
    fn from(id: MooringUserId) -> Self {
        id.0
    }
}

impl fmt::Display for MooringUserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_ten_lowercase_base32_characters() {
        let id = MooringUserId::parse("abcdefgh23").unwrap();
        assert_eq!(id.as_str(), "abcdefgh23");
        assert_eq!(id.to_string(), "abcdefgh23");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"abcdefgh23\"");
        assert_eq!(String::from(id.clone()), "abcdefgh23");
        for bad in [
            "",
            "abcdefghi",
            "abcdefghijk",
            "ABCDEFGHIJ",
            "abcdefgh01",
            "abcdefgh8z",
            "abcd-efghi",
            "k7qd-m2xa",
            "abcdefghé",
        ] {
            assert_eq!(
                MooringUserId::parse(bad),
                Err(MooringUserIdError {
                    text: bad.to_owned()
                }),
                "{bad:?}"
            );
            assert!(serde_json::from_str::<MooringUserId>(&format!("{bad:?}")).is_err());
        }
    }
}
