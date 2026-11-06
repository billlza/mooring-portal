//! Conversions between domain values and SQLite values.
//!
//! Rows are read as raw SQLite values and converted here, so a value that
//! no longer parses becomes [`StoreError::Corrupt`] naming what it was.

use portal_core::account::{AccountCode, Email};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;

use crate::StoreError;

// Every instant fits SQLite's signed 64-bit integers, so `time_to_sql` is
// exact.
const _: () = assert!(UnixTime::MAX.as_secs() <= i64::MAX as u64);

/// An instant as SQLite stores it.
pub(crate) const fn time_to_sql(time: UnixTime) -> i64 {
    time.as_secs() as i64
}

/// An instant read back.
pub(crate) fn time_from_sql(secs: i64, what: &'static str) -> Result<UnixTime, StoreError> {
    u64::try_from(secs)
        .ok()
        .and_then(|secs| UnixTime::from_secs(secs).ok())
        .ok_or_else(|| corrupt(what, format!("{secs} is not an instant")))
}

/// An account code read back.
pub(crate) fn code_from_sql(text: &str) -> Result<AccountCode, StoreError> {
    AccountCode::parse(text).map_err(|error| corrupt("account code", error.to_string()))
}

/// An email address read back.
pub(crate) fn email_from_sql(text: &str) -> Result<Email, StoreError> {
    Email::parse(text).map_err(|error| corrupt("email address", error.to_string()))
}

/// A Mooring user id read back.
pub(crate) fn user_from_sql(text: &str) -> Result<MooringUserId, StoreError> {
    MooringUserId::parse(text).map_err(|error| corrupt("Mooring user id", error.to_string()))
}

/// A [`StoreError::Corrupt`].
pub(crate) const fn corrupt(what: &'static str, detail: String) -> StoreError {
    StoreError::Corrupt { what, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_round_trip_and_bad_ones_are_corrupt() {
        for secs in [0, 1_793_880_000, UnixTime::MAX.as_secs()] {
            let time = UnixTime::from_secs(secs).unwrap();
            assert_eq!(time_from_sql(time_to_sql(time), "t").unwrap(), time);
        }
        for bad in [-1, i64::MAX] {
            assert!(matches!(
                time_from_sql(bad, "session expiry"),
                Err(StoreError::Corrupt {
                    what: "session expiry",
                    ..
                })
            ));
        }
    }

    #[test]
    fn unparsable_text_is_corrupt() {
        assert!(matches!(
            code_from_sql("nope"),
            Err(StoreError::Corrupt {
                what: "account code",
                ..
            })
        ));
        assert!(matches!(
            email_from_sql("nope"),
            Err(StoreError::Corrupt {
                what: "email address",
                ..
            })
        ));
        assert!(matches!(
            user_from_sql("nope"),
            Err(StoreError::Corrupt {
                what: "Mooring user id",
                ..
            })
        ));
        assert!(code_from_sql("0000-0000-0000").is_ok());
        assert!(email_from_sql("a@b.cn").is_ok());
        assert!(user_from_sql("pilotusera").is_ok());
    }
}
