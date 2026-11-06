//! Reading a cycle's usage against a quota, exactly as Mooring's
//! `QuotaState::of` does.

use super::record::Quota;

/// Where a user's cycle usage stands against their quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaLevel {
    /// Below the warning share.
    Within,
    /// At or above the warning share, below the quota.
    Near,
    /// At or above the quota.
    Over,
    /// The cycle's usage is unavailable, so the level is unknown (never
    /// assumed to be within the quota).
    Unknown,
}

impl Quota {
    /// The level of `used` served bytes, or [`QuotaLevel::Unknown`] when
    /// the usage is unavailable.
    #[must_use]
    pub fn level(&self, used: Option<u64>) -> QuotaLevel {
        let Some(used) = used else {
            return QuotaLevel::Unknown;
        };
        let quota = self.cycle_served_bytes.get();
        if used >= quota {
            QuotaLevel::Over
        } else if u128::from(used) * 100 >= u128::from(quota) * u128::from(self.warn_percent.get())
        {
            QuotaLevel::Near
        } else {
            QuotaLevel::Within
        }
    }

    /// `used` as a whole percentage of the quota, rounded down; above 100
    /// once the quota is exceeded.
    #[must_use]
    pub fn used_percent(&self, used: u64) -> u64 {
        let percent = u128::from(used) * 100 / u128::from(self.cycle_served_bytes.get());
        u64::try_from(percent).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::snapshot::{QuotaAction, WarnPercent};

    fn quota(bytes: u64, warn: u8) -> Quota {
        Quota {
            cycle_served_bytes: NonZeroU64::new(bytes).unwrap(),
            warn_percent: WarnPercent::new(warn).unwrap(),
            action: QuotaAction::Suspend,
        }
    }

    #[test]
    fn levels_change_exactly_at_the_warning_share_and_the_quota() {
        let quota = quota(1_000, 80);
        assert_eq!(quota.level(Some(0)), QuotaLevel::Within);
        assert_eq!(quota.level(Some(799)), QuotaLevel::Within);
        assert_eq!(quota.level(Some(800)), QuotaLevel::Near);
        assert_eq!(quota.level(Some(999)), QuotaLevel::Near);
        assert_eq!(quota.level(Some(1_000)), QuotaLevel::Over);
        assert_eq!(quota.level(Some(u64::MAX)), QuotaLevel::Over);
        assert_eq!(quota.level(None), QuotaLevel::Unknown);
    }

    #[test]
    fn the_warning_share_is_exact_for_quotas_that_do_not_divide_by_100() {
        // 80 % of 7 bytes is 5.6 bytes: 5 is within, 6 is near.
        let quota = quota(7, 80);
        assert_eq!(quota.level(Some(5)), QuotaLevel::Within);
        assert_eq!(quota.level(Some(6)), QuotaLevel::Near);
        // No overflow at the largest quota.
        let largest = Quota {
            cycle_served_bytes: NonZeroU64::MAX,
            ..quota
        };
        assert_eq!(largest.level(Some(u64::MAX - 1)), QuotaLevel::Near);
    }

    #[test]
    fn percentages_round_down_and_pass_100_when_over() {
        let quota = quota(40_000_000_000, 80);
        assert_eq!(quota.used_percent(0), 0);
        assert_eq!(quota.used_percent(399_999_999), 0);
        assert_eq!(quota.used_percent(400_000_000), 1);
        assert_eq!(quota.used_percent(39_999_999_999), 99);
        assert_eq!(quota.used_percent(40_000_000_000), 100);
        assert_eq!(quota.used_percent(41_600_000_000), 104);
        let tiny = self::quota(1, 50);
        assert_eq!(tiny.used_percent(u64::MAX), u64::MAX);
    }
}
