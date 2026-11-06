//! Instants and UTC calendar dates.
//!
//! Both are bounded to the years 1970..=9999, so every conversion between
//! them, and every date the portal prints, is total.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Seconds in a day.
pub const SECS_PER_DAY: u64 = 86_400;

/// Why a time value was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    /// An instant after 9999-12-31T23:59:59Z, or an addition past it.
    #[error("the instant is after 9999-12-31T23:59:59Z")]
    OutOfRange,
    /// Text that is not a `YYYY-MM-DD` date between 1970 and 9999.
    #[error("{text:?} is not a date written YYYY-MM-DD between 1970-01-01 and 9999-12-31")]
    InvalidDate {
        /// The refused text.
        text: String,
    },
    /// The system clock reads before 1970.
    #[error("the system clock reads before 1970-01-01")]
    ClockBeforeEpoch,
}

/// Seconds since 1970-01-01T00:00:00Z, at most [`UnixTime::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct UnixTime(u64);

impl UnixTime {
    /// 1970-01-01T00:00:00Z.
    pub const EPOCH: Self = Self(0);
    /// 9999-12-31T23:59:59Z, the latest instant the portal handles.
    pub const MAX: Self = Self(253_402_300_799);

    /// The instant `secs` seconds after the epoch (`OutOfRange` after [`Self::MAX`]).
    pub const fn from_secs(secs: u64) -> Result<Self, TimeError> {
        if secs <= Self::MAX.0 {
            Ok(Self(secs))
        } else {
            Err(TimeError::OutOfRange)
        }
    }

    /// Seconds since the epoch.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0
    }

    /// The instant `secs` later (`OutOfRange` past [`Self::MAX`]).
    pub const fn checked_add_secs(self, secs: u64) -> Result<Self, TimeError> {
        match self.0.checked_add(secs) {
            Some(sum) => Self::from_secs(sum),
            None => Err(TimeError::OutOfRange),
        }
    }

    /// Seconds from `earlier` to `self`; zero when `earlier` is not before `self`.
    #[must_use]
    pub const fn secs_since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// The UTC calendar date of the instant.
    #[must_use]
    pub const fn utc_date(self) -> CalendarDate {
        // At most 2_932_896 days, so the narrowing is exact.
        CalendarDate {
            days: (self.0 / SECS_PER_DAY) as u32,
        }
    }

    /// The wall-clock reading of the instant at `offset`.
    #[must_use]
    pub const fn civil(self, offset: UtcOffset) -> CivilDateTime {
        let local = self.0 + offset.east_secs as u64;
        let (year, month, day) = civil_from_days(local / SECS_PER_DAY);
        let secs_of_day = local % SECS_PER_DAY;
        // Each component is below its modulus, so the narrowings are exact.
        CivilDateTime {
            year,
            month,
            day,
            hour: (secs_of_day / 3600) as u8,
            minute: (secs_of_day % 3600 / 60) as u8,
            second: (secs_of_day % 60) as u8,
        }
    }
}

impl TryFrom<u64> for UnixTime {
    type Error = TimeError;

    fn try_from(secs: u64) -> Result<Self, Self::Error> {
        Self::from_secs(secs)
    }
}

impl From<UnixTime> for u64 {
    fn from(time: UnixTime) -> Self {
        time.0
    }
}

/// RFC 3339 in UTC, for example `2026-11-05T12:00:00Z`.
impl fmt::Display for UnixTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.civil(UtcOffset::UTC);
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            c.year, c.month, c.day, c.hour, c.minute, c.second
        )
    }
}

/// A fixed offset east of UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcOffset {
    east_secs: u32,
}

impl UtcOffset {
    /// UTC itself.
    pub const UTC: Self = Self { east_secs: 0 };
    /// China Standard Time (Beijing time), UTC+8 all year.
    pub const BEIJING: Self = Self {
        east_secs: 8 * 3600,
    };
}

/// A wall-clock reading: a proleptic Gregorian date and a time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilDateTime {
    /// The year; 10000 only for the last hours of 9999 read east of UTC.
    pub year: u32,
    /// 1..=12.
    pub month: u8,
    /// 1..=31.
    pub day: u8,
    /// 0..=23.
    pub hour: u8,
    /// 0..=59.
    pub minute: u8,
    /// 0..=59.
    pub second: u8,
}

/// A UTC calendar date between 1970-01-01 and 9999-12-31, written
/// `YYYY-MM-DD`. Dates order chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CalendarDate {
    days: u32,
}

impl CalendarDate {
    /// The date `year-month-day`, if it exists and lies in 1970..=9999.
    #[must_use]
    pub const fn from_ymd(year: u32, month: u8, day: u8) -> Option<Self> {
        if year < 1970 || year > 9999 || month < 1 || month > 12 {
            return None;
        }
        if day < 1 || day > days_in_month(year, month) {
            return None;
        }
        // Below 2_932_897 for years up to 9999, so the narrowing is exact.
        Some(Self {
            days: days_from_civil(year, month, day) as u32,
        })
    }

    /// Parses `YYYY-MM-DD` exactly: four, two and two ASCII digits.
    pub fn parse(text: &str) -> Result<Self, TimeError> {
        let invalid = || TimeError::InvalidDate {
            text: text.to_owned(),
        };
        let bytes = text.as_bytes();
        let [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1] = *bytes else {
            return Err(invalid());
        };
        let year = decimal(&[y0, y1, y2, y3]).ok_or_else(invalid)?;
        let month = decimal(&[m0, m1]).ok_or_else(invalid)?;
        let day = decimal(&[d0, d1]).ok_or_else(invalid)?;
        let month = u8::try_from(month).map_err(|_| invalid())?;
        let day = u8::try_from(day).map_err(|_| invalid())?;
        Self::from_ymd(year, month, day).ok_or_else(invalid)
    }

    /// UTC midnight at the start of the date.
    #[must_use]
    pub const fn midnight(self) -> UnixTime {
        UnixTime(self.days as u64 * SECS_PER_DAY)
    }

    /// The year, month and day.
    #[must_use]
    pub const fn ymd(self) -> (u32, u8, u8) {
        civil_from_days(self.days as u64)
    }
}

impl TryFrom<String> for CalendarDate {
    type Error = TimeError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<CalendarDate> for String {
    fn from(date: CalendarDate) -> Self {
        date.to_string()
    }
}

impl fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.ymd();
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// A source of the current time.
pub trait Clock: fmt::Debug + Send + Sync {
    /// The current instant.
    fn now(&self) -> Result<UnixTime, TimeError>;
}

/// The operating system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<UnixTime, TimeError> {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TimeError::ClockBeforeEpoch)?;
        UnixTime::from_secs(since_epoch.as_secs())
    }
}

/// A clock that reads whatever it was last set to: for tests and demos
/// that need time to stand still or jump.
#[derive(Debug)]
pub struct ManualClock {
    secs: AtomicU64,
}

impl ManualClock {
    /// A clock reading `start`.
    #[must_use]
    pub const fn new(start: UnixTime) -> Self {
        Self {
            secs: AtomicU64::new(start.0),
        }
    }

    /// Sets the reading to `to`.
    pub fn set(&self, to: UnixTime) {
        self.secs.store(to.0, Ordering::SeqCst);
    }

    /// Moves the reading `secs` later (`OutOfRange` past [`UnixTime::MAX`]).
    pub fn advance(&self, secs: u64) -> Result<(), TimeError> {
        let next = self.read().checked_add_secs(secs)?;
        self.set(next);
        Ok(())
    }

    fn read(&self) -> UnixTime {
        UnixTime(self.secs.load(Ordering::SeqCst))
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Result<UnixTime, TimeError> {
        Ok(self.read())
    }
}

fn decimal(digits: &[u8]) -> Option<u32> {
    digits.iter().try_fold(0_u32, |value, &digit| {
        digit
            .is_ascii_digit()
            .then(|| value * 10 + u32::from(digit - b'0'))
    })
}

const fn is_leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days_in_month(year: u32, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a date in 1970..=9999 (H. Hinnant's
/// `days_from_civil`, specialised to non-negative years and days).
const fn days_from_civil(year: u32, month: u8, day: u8) -> u64 {
    let year = (if month <= 2 { year - 1 } else { year }) as u64;
    let era = year / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (if month > 2 { month - 3 } else { month + 9 }) as u64;
    let day_of_year = (153 * shifted_month + 2) / 5 + day as u64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` after 1970-01-01 (H. Hinnant's `civil_from_days`).
const fn civil_from_days(days: u64) -> (u32, u8, u8) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };
    // The year is at most 10000 for any reading the portal makes, the
    // month 1..=12 and the day 1..=31, so the narrowings are exact.
    (year as u32, month as u8, day as u8)
}

#[cfg(test)]
mod tests;
