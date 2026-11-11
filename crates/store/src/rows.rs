//! Conversions between domain values and SQLite values.
//!
//! Rows are read as raw SQLite values and converted here, so a value that
//! no longer parses becomes [`StoreError::Corrupt`] naming what it was.

use portal_core::account::{AccountCode, Email};
use portal_core::payment::OrderId;
use portal_core::payment::stripe::StripeId;
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

/// A count (of bytes, of a currency's hundredths, of entries) as SQLite
/// stores it. Every count the portal writes is far below 2^63; one that
/// is not is refused rather than wrapped.
pub(crate) fn count_to_sql(count: u64, what: &'static str) -> Result<i64, StoreError> {
    i64::try_from(count).map_err(|_| StoreError::OutOfRange { what })
}

/// A count read back.
pub(crate) fn count_from_sql(count: i64, what: &'static str) -> Result<u64, StoreError> {
    u64::try_from(count).map_err(|_| corrupt(what, format!("{count} is negative")))
}

/// An order id read back.
pub(crate) fn order_from_sql(text: &str) -> Result<OrderId, StoreError> {
    OrderId::parse(text).map_err(|error| corrupt("order id", error.to_string()))
}

/// One of Stripe's ids read back.
pub(crate) fn stripe_id_from_sql(text: &str) -> Result<StripeId, StoreError> {
    StripeId::parse(text).map_err(|error| corrupt("Stripe id", error.to_string()))
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
    fn counts_round_trip_and_ones_out_of_range_are_refused() {
        for count in [0, 800, 100_000_000_000_000, i64::MAX as u64] {
            assert_eq!(
                count_from_sql(count_to_sql(count, "amount").unwrap(), "amount").unwrap(),
                count
            );
        }
        assert!(matches!(
            count_to_sql(i64::MAX as u64 + 1, "refunded amount"),
            Err(StoreError::OutOfRange {
                what: "refunded amount"
            })
        ));
        assert!(matches!(
            count_from_sql(-1, "amount"),
            Err(StoreError::Corrupt { what: "amount", .. })
        ));
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
        assert!(matches!(
            order_from_sql("nope"),
            Err(StoreError::Corrupt {
                what: "order id",
                ..
            })
        ));
        assert!(matches!(
            stripe_id_from_sql("no pe"),
            Err(StoreError::Corrupt {
                what: "Stripe id",
                ..
            })
        ));
        assert!(order_from_sql("0123456789abcdefghjk").is_ok());
        assert!(stripe_id_from_sql("pi_3").is_ok());
        assert!(code_from_sql("0000-0000-0000").is_ok());
        assert!(email_from_sql("a@b.cn").is_ok());
        assert!(user_from_sql("pilotusera").is_ok());
    }
}
