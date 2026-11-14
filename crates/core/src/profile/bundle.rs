//! Profile bundles: decoding and the cross-entry rules.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::value::RawValue;

use super::document::CfmProfile;
use super::error::{FoundFormat, Malformation, ProfileError};
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

/// The version-2 bundle as written. Every key is required and no other key
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
    withheld: Vec<WithheldRepr>,
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

/// One withheld user.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WithheldRepr {
    user: MooringUserId,
    reason: WithheldReason,
}

/// Why Mooring withholds a user the operator named. The portal pauses the
/// subscription link of a user withheld for a passing reason and ends it
/// for a final one ([`WithheldReason::is_final`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithheldReason {
    /// Suspended, for whatever reason: the nodes serve the user nothing,
    /// but Mooring keeps the credentials.
    Suspended,
    /// Being revoked or revoked: the credentials are gone for good.
    Revoked,
    /// Served, but not yet agreed to the current user agreement, so
    /// Mooring exports nothing to the user.
    AgreementNotAccepted,
}

impl WithheldReason {
    /// Whether the user will never be served again. Only revocation is
    /// final: a suspension ends (a new billing cycle, a renewal, the
    /// operator's word), and an agreement can still be agreed to.
    #[must_use]
    pub const fn is_final(self) -> bool {
        match self {
            Self::Revoked => true,
            Self::Suspended | Self::AgreementNotAccepted => false,
        }
    }
}

impl fmt::Display for WithheldReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
            Self::AgreementNotAccepted => "not agreed to the current user agreement",
        })
    }
}

/// A bundle that satisfies every rule of the contract: it names at least
/// one user and no user twice, and every document is a sing-box node list.
///
/// A bundle names each user the operator asked Mooring for: it delivers a
/// profile to the user, or withholds the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBundle {
    generated_at: UnixTime,
    profiles: BTreeMap<MooringUserId, CfmProfile>,
    withheld: BTreeMap<MooringUserId, WithheldReason>,
}

impl ProfileBundle {
    /// A bundle delivering `profiles` and withholding `withheld`, checked
    /// against the contract's rules.
    pub fn new(
        generated_at: UnixTime,
        profiles: impl IntoIterator<Item = (MooringUserId, CfmProfile)>,
        withheld: impl IntoIterator<Item = (MooringUserId, WithheldReason)>,
    ) -> Result<Self, ProfileError> {
        let mut delivered = BTreeMap::new();
        for (user, profile) in profiles {
            if delivered.contains_key(&user) {
                return Err(ProfileError::DuplicateUser { user });
            }
            delivered.insert(user, profile);
        }
        let mut kept_back = BTreeMap::new();
        for (user, reason) in withheld {
            if delivered.contains_key(&user) || kept_back.contains_key(&user) {
                return Err(ProfileError::DuplicateUser { user });
            }
            kept_back.insert(user, reason);
        }
        if delivered.is_empty() && kept_back.is_empty() {
            return Err(ProfileError::NamesNobody);
        }
        Ok(Self {
            generated_at,
            profiles: delivered,
            withheld: kept_back,
        })
    }

    /// Decodes and checks a bundle of at most [`MAX_BUNDLE_BYTES`].
    pub fn parse(bundle: &[u8]) -> Result<Self, ProfileError> {
        if bundle.len() > MAX_BUNDLE_BYTES {
            return Err(ProfileError::TooLarge { size: bundle.len() });
        }
        let header: Header = serde_json::from_slice(bundle).map_err(malformed)?;
        if header.format != FORMAT {
            return Err(ProfileError::WrongFormat {
                found: FoundFormat::of(&header.format),
            });
        }
        if header.version != VERSION {
            return Err(ProfileError::UnsupportedVersion {
                found: header.version,
            });
        }
        let repr: BundleRepr<'_> = serde_json::from_slice(bundle).map_err(malformed)?;
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
        let withheld = repr
            .withheld
            .into_iter()
            .map(|entry| (entry.user, entry.reason));
        Self::new(repr.generated_at, profiles, withheld)
    }

    /// When Mooring generated the bundle.
    #[must_use]
    pub const fn generated_at(&self) -> UnixTime {
        self.generated_at
    }

    /// The profiles Mooring delivers, in user id order.
    pub fn profiles(&self) -> impl Iterator<Item = (&MooringUserId, &CfmProfile)> {
        self.profiles.iter()
    }

    /// The users Mooring withholds, with why, in id order: the operator
    /// named them, but Mooring delivers nothing to them now.
    pub fn withheld(&self) -> impl Iterator<Item = (&MooringUserId, WithheldReason)> {
        self.withheld.iter().map(|(user, reason)| (user, *reason))
    }

    /// Whether the bundle names `user`, delivering a profile or
    /// withholding the user.
    #[must_use]
    pub fn names(&self, user: &MooringUserId) -> bool {
        self.profiles.contains_key(user) || self.withheld.contains_key(user)
    }

    /// How many users the bundle names, delivered or withheld.
    #[must_use]
    pub fn named_count(&self) -> usize {
        self.profiles.len() + self.withheld.len()
    }

    /// The user the bundle names, and what it does with them, if it names
    /// exactly one: a bundle Mooring exported for one user.
    #[must_use]
    pub fn sole_user(&self) -> Option<(&MooringUserId, Named<'_>)> {
        if self.named_count() != 1 {
            return None;
        }
        let delivered = self
            .profiles
            .iter()
            .map(|(user, profile)| (user, Named::Delivered(profile)));
        let withheld = self
            .withheld
            .iter()
            .map(|(user, reason)| (user, Named::Withheld(*reason)));
        delivered.chain(withheld).next()
    }
}

/// What a bundle does with a user it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named<'a> {
    /// Mooring delivers this profile to the user.
    Delivered(&'a CfmProfile),
    /// Mooring withholds the user, for this reason.
    Withheld(WithheldReason),
}

/// A decoding failure, without the decoder's message.
fn malformed(error: serde_json::Error) -> ProfileError {
    ProfileError::Malformed {
        kind: Malformation::of(&error),
        line: error.line(),
        column: error.column(),
    }
}
