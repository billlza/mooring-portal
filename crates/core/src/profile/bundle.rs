//! Profile bundles: decoding and the cross-entry rules.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::value::RawValue;

use super::document::CfmProfile;
use super::error::ProfileError;
use super::{FORMAT, MAX_BUNDLE_BYTES, VERSION};
use crate::snapshot::MooringUserId;
use crate::time::UnixTime;

/// Just the keys that say what a document is, read before anything else
/// so that another format or version is named as such.
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}

/// The version-1 bundle as written. Every key is required and no other key
/// is accepted; `format` and `version` were checked by [`Header`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleRepr<'a> {
    #[serde(rename = "format")]
    _format: IgnoredAny,
    #[serde(rename = "version")]
    _version: IgnoredAny,
    generated_at: UnixTime,
    #[serde(borrow)]
    profiles: Vec<EntryRepr<'a>>,
}

/// One user's entry. The document is taken as the exact text Mooring
/// wrote, so the portal serves what Mooring exported, byte for byte.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryRepr<'a> {
    user: MooringUserId,
    #[serde(borrow)]
    document: &'a RawValue,
}

/// A bundle that satisfies every rule of the contract: at least one
/// profile, no user twice, and every document a sing-box node list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBundle {
    generated_at: UnixTime,
    profiles: BTreeMap<MooringUserId, CfmProfile>,
}

impl ProfileBundle {
    /// A bundle of `profiles`, checked against the contract's rules.
    pub fn new(
        generated_at: UnixTime,
        profiles: impl IntoIterator<Item = (MooringUserId, CfmProfile)>,
    ) -> Result<Self, ProfileError> {
        let mut checked = BTreeMap::new();
        for (user, profile) in profiles {
            if checked.contains_key(&user) {
                return Err(ProfileError::DuplicateUser { user });
            }
            checked.insert(user, profile);
        }
        if checked.is_empty() {
            return Err(ProfileError::NoProfiles);
        }
        Ok(Self {
            generated_at,
            profiles: checked,
        })
    }

    /// Decodes and checks a bundle of at most [`MAX_BUNDLE_BYTES`].
    pub fn parse(bundle: &[u8]) -> Result<Self, ProfileError> {
        if bundle.len() > MAX_BUNDLE_BYTES {
            return Err(ProfileError::TooLarge { size: bundle.len() });
        }
        let header: Header = serde_json::from_slice(bundle).map_err(ProfileError::Json)?;
        if header.format != FORMAT {
            return Err(ProfileError::WrongFormat {
                found: header.format,
            });
        }
        if header.version != VERSION {
            return Err(ProfileError::UnsupportedVersion {
                found: header.version,
            });
        }
        let repr: BundleRepr<'_> = serde_json::from_slice(bundle).map_err(ProfileError::Json)?;
        let mut profiles = Vec::with_capacity(repr.profiles.len());
        for entry in repr.profiles {
            match CfmProfile::parse(entry.document.get()) {
                Ok(profile) => profiles.push((entry.user, profile)),
                Err(reason) => {
                    return Err(ProfileError::Document {
                        user: entry.user,
                        reason,
                    });
                }
            }
        }
        Self::new(repr.generated_at, profiles)
    }

    /// When Mooring generated the bundle.
    #[must_use]
    pub const fn generated_at(&self) -> UnixTime {
        self.generated_at
    }

    /// The profiles, in user id order.
    pub fn profiles(&self) -> impl Iterator<Item = (&MooringUserId, &CfmProfile)> {
        self.profiles.iter()
    }
}
