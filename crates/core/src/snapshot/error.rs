//! Why a snapshot document was refused.

use super::user_id::MooringUserId;
use super::{FORMAT, MAX_DOCUMENT_BYTES, VERSION};
use crate::time::UnixTime;

/// Why a snapshot document was refused. Field-level problems (an invalid
/// id, date, percentage or key) arrive as [`SnapshotError::Json`], whose
/// message names the line and column.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// Larger than [`MAX_DOCUMENT_BYTES`].
    #[error("the document is larger than the {MAX_DOCUMENT_BYTES} bytes a snapshot may have")]
    TooLarge {
        /// How many bytes were offered (a bounded reader stops early).
        size: usize,
    },
    /// Not JSON of the expected shape.
    #[error("the document is not a valid snapshot: {0}")]
    Json(#[source] serde_json::Error),
    /// Some other JSON document.
    #[error("the document's format is {found:?}, not {FORMAT:?}")]
    WrongFormat {
        /// The `format` it carries.
        found: String,
    },
    /// A version this build cannot read.
    #[error("the document is snapshot version {found}; this build reads version {VERSION}")]
    UnsupportedVersion {
        /// The `version` it carries.
        found: u32,
    },
    /// The billing cycle starts after the snapshot was generated.
    #[error("the billing cycle starts after the snapshot was generated at {generated_at}")]
    CycleAfterGeneration {
        /// When the snapshot was generated.
        generated_at: UnixTime,
    },
    /// Usage is counted through an instant outside the billing cycle.
    #[error("usage is counted through {through}, outside the billing cycle")]
    CountedThroughOutsideCycle {
        /// The instant.
        through: UnixTime,
    },
    /// Usage is counted through an instant after the snapshot was generated.
    #[error(
        "usage is counted through {through}, after the snapshot was generated at {generated_at}"
    )]
    CountedThroughAfterGeneration {
        /// The instant.
        through: UnixTime,
        /// When the snapshot was generated.
        generated_at: UnixTime,
    },
    /// No users.
    #[error("the snapshot lists no users")]
    NoUsers,
    /// The same user twice.
    #[error("user {user} is listed more than once")]
    DuplicateUser {
        /// The user.
        user: MooringUserId,
    },
    /// A usage figure without a usage report to come from.
    #[error(
        "user {user} has a cycle usage figure, but the snapshot has no usage report (counted_through is null)"
    )]
    UsageWithoutReport {
        /// The user.
        user: MooringUserId,
    },
    /// A suspension after the snapshot was generated.
    #[error("user {user} was suspended at {since}, after the snapshot was generated")]
    SuspendedAfterGeneration {
        /// The user.
        user: MooringUserId,
        /// When.
        since: UnixTime,
    },
    /// A stored record must hold exactly one user.
    #[error("a stored record must hold exactly one user, not {found}")]
    NotOneUser {
        /// How many it holds.
        found: usize,
    },
    /// Encoding a document failed.
    #[error("encoding the snapshot failed: {0}")]
    Encode(#[source] serde_json::Error),
}
