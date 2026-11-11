//! Byte counts, amounts of money and times as pages show them.

use portal_core::payment::Currency;
use portal_core::time::{UnixTime, UtcOffset};

/// Decimal units, as quotas are set (40 GB is 40 000 000 000 bytes).
const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];

/// `bytes` in the largest decimal unit that keeps the figure at least 1,
/// rounded half up to two decimals, without trailing zeros: `12.35 GB`,
/// `40 GB`, `999 B`.
pub(crate) fn bytes(bytes: u64) -> String {
    let mut unit = 0;
    let mut scale: u128 = 1;
    while unit + 1 < UNITS.len() && u128::from(bytes) >= scale * 1000 {
        unit += 1;
        scale *= 1000;
    }
    let mut hundredths = (u128::from(bytes) * 100 + scale / 2) / scale;
    // Rounding can carry into the next unit: 999 999 bytes is 1 MB.
    if hundredths >= 100_000 && unit + 1 < UNITS.len() {
        unit += 1;
        scale *= 1000;
        hundredths = (u128::from(bytes) * 100 + scale / 2) / scale;
    }
    let whole = hundredths / 100;
    let fraction = hundredths % 100;
    let number = match fraction {
        0 => whole.to_string(),
        f if f % 10 == 0 => format!("{whole}.{}", f / 10),
        f => format!("{whole}.{f:02}"),
    };
    format!("{number} {}", UNITS[unit])
}

/// `amount`, counted in the hundredths of `currency`, as a price:
/// `£8.00`. Every currency the portal prices in has hundredths; each
/// dollar is told from the others.
pub(crate) fn money(amount: u64, currency: Currency) -> String {
    let sign = match currency {
        Currency::Gbp => "£",
        Currency::Eur => "€",
        Currency::Cny => "CN¥",
        Currency::Usd => "US$",
        Currency::Hkd => "HK$",
        Currency::Sgd => "S$",
        Currency::Aud => "A$",
        Currency::Cad => "CA$",
    };
    format!("{sign}{}.{:02}", amount / 100, amount % 100)
}

/// `time` in Beijing time: `2026-11-05 20:00`.
pub(crate) fn beijing(time: UnixTime) -> String {
    let civil = time.civil(UtcOffset::BEIJING);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        civil.year, civil.month, civil.day, civil.hour, civil.minute
    )
}

/// The Beijing date of `time`: `2026-11-05`.
pub(crate) fn beijing_date(time: UnixTime) -> String {
    let civil = time.civil(UtcOffset::BEIJING);
    format!("{:04}-{:02}-{:02}", civil.year, civil.month, civil.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counts_use_decimal_units() {
        for (count, text) in [
            (0, "0 B"),
            (999, "999 B"),
            (1_000, "1 KB"),
            (1_500, "1.5 KB"),
            (1_234, "1.23 KB"),
            (1_235, "1.24 KB"),
            (999_994, "999.99 KB"),
            (999_995, "1 MB"),
            (999_999, "1 MB"),
            (12_345_678_901, "12.35 GB"),
            (40_000_000_000, "40 GB"),
            (40_100_000_000, "40.1 GB"),
            (2_000_000_000, "2 GB"),
            (1_000_000_000_000, "1 TB"),
            (u64::MAX, "18.45 EB"),
        ] {
            assert_eq!(bytes(count), text, "{count}");
        }
    }

    #[test]
    fn prices_are_written_with_their_currencys_sign_and_two_decimals() {
        for (amount, code, text) in [
            (800, "gbp", "£8.00"),
            (5, "gbp", "£0.05"),
            (1_234_567, "eur", "€12345.67"),
            (300, "cny", "CN¥3.00"),
            (999, "usd", "US$9.99"),
            (100, "hkd", "HK$1.00"),
            (110, "sgd", "S$1.10"),
            (10, "aud", "A$0.10"),
            (0, "cad", "CA$0.00"),
        ] {
            assert_eq!(
                money(amount, Currency::parse(code).unwrap()),
                text,
                "{code}"
            );
        }
    }

    #[test]
    fn times_read_in_beijing() {
        let instant = UnixTime::from_secs(1_793_880_000).unwrap();
        assert_eq!(beijing(instant), "2026-11-05 20:00");
        assert_eq!(beijing_date(instant), "2026-11-05");
        let late = UnixTime::from_secs(1_793_923_199).unwrap();
        assert_eq!(beijing(late), "2026-11-06 07:59");
        assert_eq!(beijing_date(late), "2026-11-06");
    }
}
