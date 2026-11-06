//! Why a profile bundle was refused.

use super::document::DocumentError;
use super::{FORMAT, MAX_BUNDLE_BYTES, VERSION};
use crate::snapshot::MooringUserId;

/// Why a profile bundle was refused. Problems with a key of the bundle
/// itself (an invalid id or date, a missing or unknown key) arrive as
/// [`ProfileError::Json`], whose message names the line and column;
/// problems inside a user's document arrive as [`ProfileError::Document`],
/// which never quotes the document.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// Larger than [`MAX_BUNDLE_BYTES`].
    #[error("the bundle is larger than the {MAX_BUNDLE_BYTES} bytes a profile bundle may have")]
    TooLarge {
        /// How many bytes were offered (a bounded reader stops early).
        size: usize,
    },
    /// Not JSON of the expected shape.
    #[error("the file is not a valid profile bundle: {0}")]
    Json(#[source] serde_json::Error),
    /// Some other JSON document.
    #[error("the file's format is {found:?}, not {FORMAT:?}")]
    WrongFormat {
        /// The `format` it carries.
        found: String,
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
