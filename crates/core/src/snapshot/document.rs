//! Snapshot documents: decoding, the cross-field rules, and encoding.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::error::SnapshotError;
use super::record::{BillingCycle, Quota, UserRecord, UserStatus};
use super::user_id::MooringUserId;
use super::{FORMAT, MAX_DOCUMENT_BYTES, VERSION};
use crate::time::UnixTime;

/// Just the keys that say what a document is, read before anything else
/// so that another format or version is named as such.
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}

/// The version-1 document as written. Every key is required (a missing
/// optional value is an explicit `null`) and no other key is accepted.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentRepr {
    format: String,
    version: u32,
    generated_at: UnixTime,
    cycle: BillingCycle,
    #[serde(deserialize_with = "Option::deserialize")]
    counted_through: Option<UnixTime>,
    users: Vec<UserRepr>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserRepr {
    user: MooringUserId,
    status: UserStatus,
    #[serde(deserialize_with = "Option::deserialize")]
    cycle_served_bytes: Option<u64>,
    reality_downlink_counted_at_close: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    quota: Option<Quota>,
    #[serde(deserialize_with = "Option::deserialize")]
    expires_at: Option<UnixTime>,
}

/// A snapshot that satisfies every rule of the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    generated_at: UnixTime,
    cycle: BillingCycle,
    counted_through: Option<UnixTime>,
    users: BTreeMap<MooringUserId, UserRecord>,
}

impl Snapshot {
    /// A snapshot of `users`, checked against the contract's rules.
    pub fn new(
        generated_at: UnixTime,
        cycle: BillingCycle,
        counted_through: Option<UnixTime>,
        users: impl IntoIterator<Item = (MooringUserId, UserRecord)>,
    ) -> Result<Self, SnapshotError> {
        if cycle.starts_at() > generated_at {
            return Err(SnapshotError::CycleAfterGeneration { generated_at });
        }
        if let Some(through) = counted_through {
            if through < cycle.starts_at() || through > cycle.ends_at() {
                return Err(SnapshotError::CountedThroughOutsideCycle { through });
            }
            if through > generated_at {
                return Err(SnapshotError::CountedThroughAfterGeneration {
                    through,
                    generated_at,
                });
            }
        }
        let mut checked = BTreeMap::new();
        for (user, record) in users {
            if counted_through.is_none() && record.cycle_served_bytes.is_some() {
                return Err(SnapshotError::UsageWithoutReport { user });
            }
            if let UserStatus::Suspended { since, .. } = record.status
                && since > generated_at
            {
                return Err(SnapshotError::SuspendedAfterGeneration { user, since });
            }
            if checked.contains_key(&user) {
                return Err(SnapshotError::DuplicateUser { user });
            }
            checked.insert(user, record);
        }
        if checked.is_empty() {
            return Err(SnapshotError::NoUsers);
        }
        Ok(Self {
            generated_at,
            cycle,
            counted_through,
            users: checked,
        })
    }

    /// Decodes and checks a document of at most [`MAX_DOCUMENT_BYTES`].
    pub fn parse(document: &[u8]) -> Result<Self, SnapshotError> {
        if document.len() > MAX_DOCUMENT_BYTES {
            return Err(SnapshotError::TooLarge {
                size: document.len(),
            });
        }
        let header: Header = serde_json::from_slice(document).map_err(SnapshotError::Json)?;
        if header.format != FORMAT {
            return Err(SnapshotError::WrongFormat {
                found: header.format,
            });
        }
        if header.version != VERSION {
            return Err(SnapshotError::UnsupportedVersion {
                found: header.version,
            });
        }
        let repr: DocumentRepr = serde_json::from_slice(document).map_err(SnapshotError::Json)?;
        Self::new(
            repr.generated_at,
            repr.cycle,
            repr.counted_through,
            repr.users.into_iter().map(|user| {
                (
                    user.user,
                    UserRecord {
                        status: user.status,
                        cycle_served_bytes: user.cycle_served_bytes,
                        reality_downlink_counted_at_close: user.reality_downlink_counted_at_close,
                        quota: user.quota,
                        expires_at: user.expires_at,
                    },
                )
            }),
        )
    }

    /// The document, pretty-printed with the users in id order.
    pub fn to_json_pretty(&self) -> Result<String, SnapshotError> {
        serde_json::to_string_pretty(&self.repr(self.users.iter())).map_err(SnapshotError::Encode)
    }

    /// When Mooring generated the snapshot.
    #[must_use]
    pub const fn generated_at(&self) -> UnixTime {
        self.generated_at
    }

    /// The billing cycle the usage figures belong to.
    #[must_use]
    pub const fn cycle(&self) -> BillingCycle {
        self.cycle
    }

    /// The end (exclusive) of the counted usage; `None` when Mooring had
    /// no usage report.
    #[must_use]
    pub const fn counted_through(&self) -> Option<UnixTime> {
        self.counted_through
    }

    /// The users, in id order.
    pub fn users(&self) -> impl Iterator<Item = (&MooringUserId, &UserRecord)> {
        self.users.iter()
    }

    /// Each user's part of the snapshot, in id order.
    pub fn per_user(&self) -> impl Iterator<Item = UserSnapshot> + '_ {
        self.users.iter().map(|(user, record)| UserSnapshot {
            generated_at: self.generated_at,
            cycle: self.cycle,
            counted_through: self.counted_through,
            user: user.clone(),
            record: *record,
        })
    }

    fn repr<'a>(
        &self,
        users: impl Iterator<Item = (&'a MooringUserId, &'a UserRecord)>,
    ) -> DocumentRepr {
        DocumentRepr {
            format: FORMAT.to_owned(),
            version: VERSION,
            generated_at: self.generated_at,
            cycle: self.cycle,
            counted_through: self.counted_through,
            users: users
                .map(|(user, record)| UserRepr {
                    user: user.clone(),
                    status: record.status,
                    cycle_served_bytes: record.cycle_served_bytes,
                    reality_downlink_counted_at_close: record.reality_downlink_counted_at_close,
                    quota: record.quota,
                    expires_at: record.expires_at,
                })
                .collect(),
        }
    }
}

/// One user's part of a snapshot, with the snapshot-wide values it is read
/// against. The portal stores it as a one-user version-1 document, so a
/// stored record is checked by the same rules as an imported one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSnapshot {
    generated_at: UnixTime,
    cycle: BillingCycle,
    counted_through: Option<UnixTime>,
    user: MooringUserId,
    record: UserRecord,
}

impl UserSnapshot {
    /// Decodes a stored record: a document holding exactly one user.
    pub fn from_document(document: &str) -> Result<Self, SnapshotError> {
        let snapshot = Snapshot::parse(document.as_bytes())?;
        let found = snapshot.users.len();
        let mut users = snapshot.per_user();
        match (users.next(), users.next()) {
            (Some(only), None) => Ok(only),
            _ => Err(SnapshotError::NotOneUser { found }),
        }
    }

    /// The compact one-user document stored for this user.
    pub fn to_document(&self) -> Result<String, SnapshotError> {
        let snapshot = Snapshot {
            generated_at: self.generated_at,
            cycle: self.cycle,
            counted_through: self.counted_through,
            users: BTreeMap::new(),
        };
        serde_json::to_string(&snapshot.repr(std::iter::once((&self.user, &self.record))))
            .map_err(SnapshotError::Encode)
    }

    /// When Mooring generated the snapshot.
    #[must_use]
    pub const fn generated_at(&self) -> UnixTime {
        self.generated_at
    }

    /// The billing cycle the usage figure belongs to.
    #[must_use]
    pub const fn cycle(&self) -> BillingCycle {
        self.cycle
    }

    /// The end (exclusive) of the counted usage, if Mooring had a report.
    #[must_use]
    pub const fn counted_through(&self) -> Option<UnixTime> {
        self.counted_through
    }

    /// The user.
    #[must_use]
    pub const fn user(&self) -> &MooringUserId {
        &self.user
    }

    /// What the snapshot says about the user.
    #[must_use]
    pub const fn record(&self) -> &UserRecord {
        &self.record
    }
}

#[cfg(test)]
mod tests;
