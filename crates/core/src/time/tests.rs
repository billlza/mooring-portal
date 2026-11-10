use super::*;

fn date(text: &str) -> CalendarDate {
    CalendarDate::parse(text).unwrap()
}

#[test]
fn every_day_round_trips_between_civil_and_day_numbers() {
    // Every day from 1970 to the end of 2400 (leap centuries included),
    // against a date counter stepped by hand.
    let (mut year, mut month, mut day) = (1970_u32, 1_u8, 1_u8);
    for days in 0..158_000_u64 {
        assert_eq!(civil_from_days(days), (year, month, day), "{days}");
        assert_eq!(days_from_civil(year, month, day), days);
        day += 1;
        if day > days_in_month(year, month) {
            day = 1;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        }
    }
}

#[test]
fn leap_years_follow_the_gregorian_rule() {
    for (year, leap) in [
        (1972, true),
        (1900, false),
        (2000, true),
        (2100, false),
        (2024, true),
        (2026, false),
    ] {
        assert_eq!(is_leap(year), leap, "{year}");
    }
    assert!(CalendarDate::from_ymd(2024, 2, 29).is_some());
    assert!(CalendarDate::from_ymd(2026, 2, 29).is_none());
    assert!(CalendarDate::from_ymd(2000, 2, 29).is_some());
}

#[test]
fn dates_parse_exactly() {
    let parsed = date("2026-11-05");
    assert_eq!(parsed.ymd(), (2026, 11, 5));
    assert_eq!(parsed.to_string(), "2026-11-05");
    assert_eq!(parsed.midnight().as_secs(), 1_793_836_800);
    assert_eq!(date("1970-01-01").midnight(), UnixTime::EPOCH);
    assert_eq!(date("9999-12-31").ymd(), (9999, 12, 31));
    for bad in [
        "",
        "2026-1-05",
        "2026-11-5",
        "2026/11/05",
        "26-11-05",
        "2026-11-05 ",
        " 2026-11-05",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "2026-11-00",
        "1969-12-31",
        "+026-11-05",
        "2026-1a-05",
        "２０２６-11-05",
        "10000-01-01",
    ] {
        assert_eq!(
            CalendarDate::parse(bad),
            Err(TimeError::InvalidDate {
                text: bad.to_owned()
            }),
            "{bad:?}"
        );
    }
}

#[test]
fn dates_order_chronologically_and_encode_as_strings() {
    assert!(date("2026-10-31") < date("2026-11-01"));
    assert!(date("2025-12-31") < date("2026-01-01"));
    let json = serde_json::to_string(&date("2026-11-01")).unwrap();
    assert_eq!(json, "\"2026-11-01\"");
    assert_eq!(
        serde_json::from_str::<CalendarDate>(&json).unwrap(),
        date("2026-11-01")
    );
    assert!(serde_json::from_str::<CalendarDate>("\"2026-02-30\"").is_err());
    assert!(serde_json::from_str::<CalendarDate>("20261101").is_err());
}

#[test]
fn instants_are_bounded_to_the_year_9999() {
    assert_eq!(UnixTime::from_secs(253_402_300_799), Ok(UnixTime::MAX));
    assert_eq!(
        UnixTime::from_secs(253_402_300_800),
        Err(TimeError::OutOfRange)
    );
    assert_eq!(UnixTime::MAX.to_string(), "9999-12-31T23:59:59Z");
    assert_eq!(
        UnixTime::MAX.checked_add_secs(1),
        Err(TimeError::OutOfRange)
    );
    assert_eq!(
        UnixTime::EPOCH.checked_add_secs(u64::MAX),
        Err(TimeError::OutOfRange)
    );
    assert_eq!(
        UnixTime::EPOCH.checked_add_secs(86_400).unwrap().as_secs(),
        86_400
    );
    assert!(serde_json::from_str::<UnixTime>("253402300800").is_err());
    assert!(serde_json::from_str::<UnixTime>("-1").is_err());
    assert_eq!(
        serde_json::to_string(&UnixTime::MAX).unwrap(),
        "253402300799"
    );
    assert_eq!(u64::from(UnixTime::MAX), 253_402_300_799);
}

#[test]
fn instants_print_in_utc_and_read_in_beijing_time() {
    let instant = UnixTime::from_secs(1_793_923_199).unwrap();
    assert_eq!(instant.to_string(), "2026-11-05T23:59:59Z");
    assert_eq!(instant.utc_date(), date("2026-11-05"));
    let beijing = instant.civil(UtcOffset::BEIJING);
    assert_eq!(
        beijing,
        CivilDateTime {
            year: 2026,
            month: 11,
            day: 6,
            hour: 7,
            minute: 59,
            second: 59
        }
    );
    // The last hours of 9999 read east of UTC fall in 10000.
    assert_eq!(UnixTime::MAX.civil(UtcOffset::BEIJING).year, 10_000);
    assert_eq!(UnixTime::EPOCH.civil(UtcOffset::UTC).hour, 0);
}

#[test]
fn secs_since_saturates() {
    let early = UnixTime::from_secs(100).unwrap();
    let late = UnixTime::from_secs(250).unwrap();
    assert_eq!(late.secs_since(early), 150);
    assert_eq!(early.secs_since(late), 0);
}

#[test]
fn clocks_read_the_time() {
    let now = SystemClock.now().unwrap();
    assert!(now > date("2026-01-01").midnight());
    let manual = ManualClock::new(date("2026-11-05").midnight());
    assert_eq!(manual.now().unwrap(), date("2026-11-05").midnight());
    manual.advance(SECS_PER_DAY).unwrap();
    assert_eq!(manual.now().unwrap(), date("2026-11-06").midnight());
    manual.set(UnixTime::MAX);
    assert_eq!(manual.advance(1), Err(TimeError::OutOfRange));
    assert_eq!(manual.now().unwrap(), UnixTime::MAX);
}

#[test]
fn instants_parse_back_from_what_they_print() {
    for secs in [
        0,
        59,
        86_399,
        86_400,
        951_782_400,
        1_793_880_000,
        UnixTime::MAX.as_secs(),
    ] {
        let time = UnixTime::from_secs(secs).unwrap();
        assert_eq!(UnixTime::parse_utc(&time.to_string()), Ok(time), "{time}");
    }
    assert_eq!(
        UnixTime::parse_utc("2026-11-07T15:27:01Z")
            .unwrap()
            .as_secs(),
        1_794_065_221
    );
    assert_eq!(
        UnixTime::parse_utc("2000-02-29T23:59:59Z")
            .unwrap()
            .as_secs(),
        951_868_799
    );
}

#[test]
fn only_utc_instants_to_the_second_are_read() {
    for bad in [
        "",
        "2026-11-07",
        "2026-11-07T15:27:01",
        "2026-11-07 15:27:01Z",
        "2026-11-07t15:27:01Z",
        "2026-11-07T15:27:01z",
        "2026-11-07T15:27:01+00:00",
        "2026-11-07T23:27:01+08:00",
        "2026-11-07T15:27:01.000Z",
        "2026-11-07T15:27:1Z",
        "2026-11-07T15-27-01Z",
        "2026-11-07T24:00:00Z",
        "2026-11-07T23:60:00Z",
        "2026-11-07T23:59:60Z",
        "2026-02-30T00:00:00Z",
        "1969-12-31T23:59:59Z",
        "10000-01-01T00:00:00Z",
        " 2026-11-07T15:27:01Z",
        "2026-11-07T15:27:01Z ",
        "2026-11-07T+5:27:01Z",
        "2026-11-07T１5:27:01Z",
    ] {
        assert_eq!(
            UnixTime::parse_utc(bad),
            Err(TimeError::InvalidInstant {
                text: bad.to_owned()
            }),
            "{bad:?}"
        );
    }
}
