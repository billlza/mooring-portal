//! The portal snapshot contract, version 1.
//!
//! Mooring exports, per user, only what the portal shows: the user's
//! status, the billing cycle's served bytes, the quota and the expiry. No
//! name, alias, label, credential, node, address or IP crosses. The
//! normative description is `docs/contract/portal-snapshot-v1.md`; the
//! golden documents under `contract/fixtures/v1/` pin the encoding.
//!
//! The portal never derives a figure Mooring did not send: a usage figure
//! Mooring could not establish travels as `null` and is shown as
//! unavailable, never as zero.

mod document;
mod error;
mod quota;
mod record;
mod user_id;

pub use document::{Snapshot, UserSnapshot};
pub use error::SnapshotError;
pub use quota::QuotaLevel;
pub use record::{
    BillingCycle, EmptyCycle, Quota, QuotaAction, SuspendReason, UserRecord, UserStatus,
    WarnPercent, WarnPercentError,
};
pub use user_id::{MooringUserId, MooringUserIdError};

/// The value of the document's `format` key.
pub const FORMAT: &str = "mooring-portal-snapshot";
/// The contract version this build reads and writes.
pub const VERSION: u32 = 1;
/// The largest document accepted, in bytes. A snapshot of the pilot's
/// five users is about two kilobytes.
pub const MAX_DOCUMENT_BYTES: usize = 1 << 20;
