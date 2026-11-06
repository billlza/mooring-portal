//! The values a snapshot carries for each user, and the billing cycle.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

use crate::time::{CalendarDate, UnixTime};

/// One SIM billing cycle on the UTC calendar: from UTC midnight of
/// `start` (inclusive) to UTC midnight of `next_start` (exclusive), as
/// Mooring's `BillingCycle` defines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BillingCycleRepr", into = "BillingCycleRepr")]
pub struct BillingCycle {
    start: CalendarDate,
    next_start: CalendarDate,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BillingCycleRepr {
    start: CalendarDate,
    next_start: CalendarDate,
}

/// A billing cycle that does not end after it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the billing cycle must end after it starts, not run from {start} to {next_start}")]
pub struct EmptyCycle {
    /// The refused start.
    pub start: CalendarDate,
    /// The refused next start.
    pub next_start: CalendarDate,
}

impl BillingCycle {
    /// The cycle from `start` to `next_start` (`EmptyCycle` unless later).
    pub fn new(start: CalendarDate, next_start: CalendarDate) -> Result<Self, EmptyCycle> {
        if start < next_start {
            Ok(Self { start, next_start })
        } else {
            Err(EmptyCycle { start, next_start })
        }
    }

    /// The first day of the cycle.
    #[must_use]
    pub const fn start(self) -> CalendarDate {
        self.start
    }

    /// The first day of the next cycle.
    #[must_use]
    pub const fn next_start(self) -> CalendarDate {
        self.next_start
    }

    /// The instant the cycle starts.
    #[must_use]
    pub const fn starts_at(self) -> UnixTime {
        self.start.midnight()
    }

    /// The instant the next cycle starts.
    #[must_use]
    pub const fn ends_at(self) -> UnixTime {
        self.next_start.midnight()
    }
}

impl TryFrom<BillingCycleRepr> for BillingCycle {
    type Error = EmptyCycle;

    fn try_from(repr: BillingCycleRepr) -> Result<Self, Self::Error> {
        Self::new(repr.start, repr.next_start)
    }
}

impl From<BillingCycle> for BillingCycleRepr {
    fn from(cycle: BillingCycle) -> Self {
        Self {
            start: cycle.start,
            next_start: cycle.next_start,
        }
    }
}

/// Why a user is suspended (Mooring's `SuspendReason`, without the
/// quota cycle's date).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspendReason {
    /// The operator suspended the user.
    Operator,
    /// The user's served bytes reached their quota in a billing cycle.
    QuotaExceeded,
    /// The user's access expired.
    Expired,
    /// An abuse complaint is being reviewed.
    AbuseReview,
}

/// A user's state (Mooring's `UserStatus`; its `revoke_pending` and
/// `revoked` both travel as `revoked`, since either way the user is served
/// nowhere and never will be again).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UserStatus {
    /// The user is served.
    Active,
    /// The user is served nowhere until resumed.
    Suspended {
        /// Why.
        reason: SuspendReason,
        /// When the user was suspended.
        since: UnixTime,
    },
    /// The user's access was revoked for good.
    Revoked,
}

/// The share of a quota at which Mooring warns the operator, 1..=99
/// percent; the portal shows the user as near their quota from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct WarnPercent(u8);

/// A warning share outside 1..=99.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{percent} is not a warning share (1..=99 percent)")]
pub struct WarnPercentError {
    /// The refused share.
    pub percent: u8,
}

impl WarnPercent {
    /// Accepts 1..=99.
    pub const fn new(percent: u8) -> Result<Self, WarnPercentError> {
        if percent >= 1 && percent <= 99 {
            Ok(Self(percent))
        } else {
            Err(WarnPercentError { percent })
        }
    }

    /// The share in percent.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for WarnPercent {
    type Error = WarnPercentError;

    fn try_from(percent: u8) -> Result<Self, Self::Error> {
        Self::new(percent)
    }
}

impl From<WarnPercent> for u8 {
    fn from(percent: WarnPercent) -> Self {
        percent.0
    }
}

/// What reaching the quota does (Mooring's `QuotaAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaAction {
    /// Only the operator is told.
    Warn,
    /// The user is suspended until the next billing cycle.
    Suspend,
}

/// A user's effective quota for the cycle: their own, or the automatic
/// share Mooring computed (Mooring's `effective_quota`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    /// Served bytes (both directions, every node) allowed per cycle.
    pub cycle_served_bytes: NonZeroU64,
    /// The share at which the user counts as near the quota.
    pub warn_percent: WarnPercent,
    /// What reaching the quota does.
    pub action: QuotaAction,
}

/// What the snapshot says about one user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserRecord {
    /// The user's state.
    pub status: UserStatus,
    /// Bytes served to the user in the snapshot's cycle, through
    /// `counted_through`; `None` when Mooring could not establish the
    /// figure, which the portal shows as unavailable, never as zero.
    pub cycle_served_bytes: Option<u64>,
    /// Whether REALITY downlink bytes are counted only when a connection
    /// closes (Mooring's `UsageAccuracy`): true while the user is served,
    /// so the figure may lag.
    pub reality_downlink_counted_at_close: bool,
    /// The effective quota; `None`: the user has no quota of their own.
    pub quota: Option<Quota>,
    /// When the user's access expires; `None`: it does not.
    pub expires_at: Option<UnixTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> CalendarDate {
        CalendarDate::parse(text).unwrap()
    }

    #[test]
    fn a_cycle_must_end_after_it_starts() {
        let cycle = BillingCycle::new(date("2026-11-01"), date("2026-12-01")).unwrap();
        assert_eq!(cycle.start(), date("2026-11-01"));
        assert_eq!(cycle.next_start(), date("2026-12-01"));
        assert_eq!(cycle.starts_at(), date("2026-11-01").midnight());
        assert_eq!(cycle.ends_at(), date("2026-12-01").midnight());
        for (start, next) in [("2026-11-01", "2026-11-01"), ("2026-12-01", "2026-11-01")] {
            assert_eq!(
                BillingCycle::new(date(start), date(next)),
                Err(EmptyCycle {
                    start: date(start),
                    next_start: date(next)
                })
            );
        }
        let json = r#"{"start":"2026-11-01","next_start":"2026-12-01"}"#;
        assert_eq!(serde_json::to_string(&cycle).unwrap(), json);
        assert_eq!(serde_json::from_str::<BillingCycle>(json).unwrap(), cycle);
        assert!(
            serde_json::from_str::<BillingCycle>(
                r#"{"start":"2026-11-01","next_start":"2026-11-01"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<BillingCycle>(
                r#"{"start":"2026-11-01","next_start":"2026-12-01","days":30}"#
            )
            .is_err()
        );
    }

    #[test]
    fn statuses_encode_as_mooring_writes_them() {
        let since = UnixTime::from_secs(1_793_836_800).unwrap();
        for (status, json) in [
            (UserStatus::Active, "\"active\"".to_owned()),
            (UserStatus::Revoked, "\"revoked\"".to_owned()),
            (
                UserStatus::Suspended {
                    reason: SuspendReason::QuotaExceeded,
                    since,
                },
                r#"{"suspended":{"reason":"quota_exceeded","since":1793836800}}"#.to_owned(),
            ),
            (
                UserStatus::Suspended {
                    reason: SuspendReason::AbuseReview,
                    since,
                },
                r#"{"suspended":{"reason":"abuse_review","since":1793836800}}"#.to_owned(),
            ),
        ] {
            assert_eq!(serde_json::to_string(&status).unwrap(), json);
            assert_eq!(serde_json::from_str::<UserStatus>(&json).unwrap(), status);
        }
        for bad in [
            "\"revoke_pending\"",
            "\"suspended\"",
            r#"{"suspended":{"reason":"operator"}}"#,
            r#"{"suspended":{"reason":"holiday","since":1}}"#,
            r#"{"suspended":{"reason":"operator","since":1,"by":"me"}}"#,
            r#"{"suspended":{"reason":{"quota_exceeded":{"cycle_start":"2026-10-01"}},"since":1}}"#,
        ] {
            assert!(serde_json::from_str::<UserStatus>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn warn_percent_is_one_to_ninety_nine() {
        for percent in 0..=u8::MAX {
            assert_eq!(
                WarnPercent::new(percent).is_ok(),
                (1..=99).contains(&percent)
            );
        }
        assert_eq!(WarnPercent::new(80).unwrap().get(), 80);
        assert_eq!(u8::from(WarnPercent::new(80).unwrap()), 80);
        assert_eq!(
            WarnPercent::new(100),
            Err(WarnPercentError { percent: 100 })
        );
    }

    #[test]
    fn quotas_encode_with_every_key_required() {
        let json = r#"{"cycle_served_bytes":40000000000,"warn_percent":80,"action":"suspend"}"#;
        let quota: Quota = serde_json::from_str(json).unwrap();
        assert_eq!(quota.cycle_served_bytes.get(), 40_000_000_000);
        assert_eq!(quota.action, QuotaAction::Suspend);
        assert_eq!(serde_json::to_string(&quota).unwrap(), json);
        for bad in [
            json.replace("40000000000", "0"),
            json.replace(r#""warn_percent":80"#, r#""warn_percent":100"#),
            json.replace(r#""suspend""#, r#""revoke""#),
            json.replace(r#","action":"suspend""#, ""),
            json.replace('}', r#","extra":1}"#),
        ] {
            assert!(serde_json::from_str::<Quota>(&bad).is_err(), "{bad}");
        }
        assert_eq!(
            serde_json::from_str::<QuotaAction>("\"warn\"").unwrap(),
            QuotaAction::Warn
        );
    }
}
