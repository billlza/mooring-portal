//! The CFM profile bundle contract, version 1.
//!
//! For each user it may deliver to, Mooring exports the CFM import document
//! `mooring-export` writes in its default format: sing-box JSON whose only
//! key is `outbounds` (Mooring's `docs/cfm-contract.md`). The portal hands
//! each user their own document, as a download and through their
//! subscription link, and does nothing else with it: it checks the
//! document's outline and never derives, logs or rewrites it.
//!
//! Unlike the snapshot, these documents hold credentials and node
//! addresses (the VLESS UUID, the REALITY key and short id, the Hysteria2
//! passwords), so they travel in a document of their own and
//! [`CfmProfile`] never prints them. The normative description is
//! `docs/contract/cfm-profiles-v1.md`; the synthetic documents under
//! `contract/fixtures/cfm-profiles-v1/` pin the encoding.

mod bundle;
mod document;
mod error;

pub use bundle::ProfileBundle;
pub use document::{CfmProfile, DocumentError};
pub use error::ProfileError;

/// The value of the bundle's `format` key.
pub const FORMAT: &str = "mooring-portal-cfm-profiles";
/// The contract version this build reads.
pub const VERSION: u32 = 1;
/// The largest bundle accepted, in bytes.
pub const MAX_BUNDLE_BYTES: usize = 1 << 20;
/// The largest single document accepted, in bytes. A pilot user's document
/// is a few kilobytes; CFM 0.5.1 itself accepts up to 4 MiB.
pub const MAX_PROFILE_BYTES: usize = 64 * 1024;
