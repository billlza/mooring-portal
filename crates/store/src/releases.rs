//! The CFM release the download page offers: one row, replaced whole by
//! each import of GitHub's latest release.
//!
//! The row holds nothing private, so replacing it needs no erasure. It is
//! checked again when read back, like every row.

use std::cmp::Ordering;

use portal_core::release::{CfmRelease, Version};
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::rows::{corrupt, time_from_sql, time_to_sql};
use crate::{Store, StoreError};

/// The release on offer, and when it was last found to be the latest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedRelease {
    /// The release.
    pub release: CfmRelease,
    /// When an import last found it to be the latest.
    pub checked_at: UnixTime,
}

/// What importing a release changed on the download page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChange {
    /// Nothing was on offer before.
    First,
    /// The same release is still the latest; only the time of the check
    /// moved.
    Unchanged,
    /// A newer release took the place of `from`.
    Newer {
        /// The version offered until now.
        from: Version,
    },
    /// An older release took the place of `from`: the newer one is no
    /// longer the latest, so it was withdrawn.
    Older {
        /// The version offered until now.
        from: Version,
    },
    /// The same version, published again: its disk image or its time of
    /// publication changed.
    Republished,
}

impl Store {
    /// Puts `release` on offer, found to be the latest at `now`.
    pub fn offer_release(
        &mut self,
        release: &CfmRelease,
        now: UnixTime,
    ) -> Result<ReleaseChange, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = offered(&tx)?;
        tx.execute(
            "INSERT INTO cfm_release (id, version, published_at, dmg_sha256, checked_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET version = excluded.version,
                 published_at = excluded.published_at, dmg_sha256 = excluded.dmg_sha256,
                 checked_at = excluded.checked_at",
            (
                release.version().to_string(),
                time_to_sql(release.published_at()),
                release.dmg_sha256(),
                time_to_sql(now),
            ),
        )?;
        tx.commit()?;
        Ok(match before {
            None => ReleaseChange::First,
            Some(before) if before.release == *release => ReleaseChange::Unchanged,
            Some(before) => {
                let from = before.release.version();
                match release.version().cmp(&from) {
                    Ordering::Greater => ReleaseChange::Newer { from },
                    Ordering::Less => ReleaseChange::Older { from },
                    Ordering::Equal => ReleaseChange::Republished,
                }
            }
        })
    }

    /// The release on offer, if one has been imported.
    pub fn offered_release(&self) -> Result<Option<OfferedRelease>, StoreError> {
        offered(&self.conn)
    }
}

fn offered(conn: &Connection) -> Result<Option<OfferedRelease>, StoreError> {
    let row: Option<(String, i64, String, i64)> = conn
        .query_row(
            "SELECT version, published_at, dmg_sha256, checked_at FROM cfm_release WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((version, published_at, dmg_sha256, checked_at)) = row else {
        return Ok(None);
    };
    let version =
        Version::parse(&version).map_err(|error| corrupt("release version", error.to_string()))?;
    let release = CfmRelease::from_parts(
        version,
        time_from_sql(published_at, "release publication time")?,
        &dmg_sha256,
    )
    .map_err(|error| corrupt("release disk image digest", error.to_string()))?;
    Ok(Some(OfferedRelease {
        release,
        checked_at: time_from_sql(checked_at, "release check time")?,
    }))
}
