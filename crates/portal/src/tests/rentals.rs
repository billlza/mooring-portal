//! Listing and closing Mac rental bookings.

use std::process::ExitCode;

use portal_core::auth::TokenDigest;
use portal_core::rental::{BookingTerms, ChipName, Machine, MachineId, Period};
use portal_core::time::UnixTime;

use super::{FailAfter, Fixture, NOW, run_line};

/// Books `terms` with the session `session` at `at` seconds past [`NOW`].
fn book(fixture: &Fixture, session: &TokenDigest, terms: &BookingTerms, at: u64) {
    fixture
        .store()
        .book(session, terms, UnixTime::from_secs(NOW + at).unwrap())
        .unwrap();
}

fn offered(id: &str, period: Period) -> BookingTerms {
    BookingTerms::of(Machine::find(id).unwrap(), period)
}

#[test]
fn bookings_are_listed_oldest_first_with_whose_they_are() {
    let fixture = Fixture::new();
    let empty = fixture.run(&["bookings"]);
    assert_eq!(empty.code, ExitCode::SUCCESS, "{}", empty.err);
    assert_eq!(
        empty.out,
        "NUMBER  BOOKED                ACCOUNT         MACHINE             PERIOD   PRICE        \
         FIRST        EMAIL\n0 booking(s)\n"
    );
    let (_, alice) = fixture.signed_in("alice@qq.com", 1);
    let (_, bob) = fixture.signed_in("bob@163.com", 2);
    book(&fixture, &bob, &offered("m5pro-24-512", Period::Year), 60);
    book(&fixture, &alice, &offered("m6-24-512", Period::Month), 0);
    let listed = fixture.run(&["bookings"]);
    assert_eq!(listed.code, ExitCode::SUCCESS, "{}", listed.err);
    assert_eq!(
        listed.out,
        "NUMBER  BOOKED                ACCOUNT         MACHINE             PERIOD   PRICE        \
         FIRST        EMAIL\n\
         2       2026-11-05T12:00:00Z  0000-0000-0001  M6 24GB/512GB       month    169.00 gbp   \
         101.40 gbp   alice@qq.com\n\
         1       2026-11-05T12:01:00Z  0000-0000-0002  M5 Pro 24GB/512GB   year     2234.00 gbp  \
         2234.00 gbp  bob@163.com\n\
         2 booking(s)\n\
         once one is dealt with (its machine is ordered, or its owner has said no), close it with \
         `close-booking --id <number>`\n"
    );
}

#[test]
fn a_closed_booking_is_gone_from_the_database_files() {
    let fixture = Fixture::new();
    // The server's connection, which keeps the write-ahead log in place.
    let server = fixture.store();
    let (code, alice) = fixture.signed_in("alice@qq.com", 1);
    // A machine no longer on offer, so that its id is in no other row.
    let gone = BookingTerms {
        machine: MachineId::parse("zz-gone-machine").unwrap(),
        chip: ChipName::parse("Gone Chip").unwrap(),
        ..offered("m4-16-256", Period::Week)
    };
    book(&fixture, &alice, &gone, 0);
    book(&fixture, &alice, &offered("m6-16-256", Period::Day), 1);
    assert!(fixture.files_hold(b"zz-gone-machine"));
    let closed = fixture.run(&["close-booking", "--id", "1"]);
    assert_eq!(closed.code, ExitCode::SUCCESS, "{}", closed.err);
    assert_eq!(
        closed.out,
        format!(
            "closed booking #1: Gone Chip 16GB/256GB by the week, for account {code}\n\
             erased the deleted rows from the database files\n"
        )
    );
    assert!(!fixture.files_hold(b"zz-gone-machine"));
    assert!(!fixture.files_hold(b"Gone Chip"));
    // The other booking stays.
    let left = server.bookings().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].booking.terms.machine.as_str(), "m6-16-256");
    assert!(fixture.files_hold(b"m6-16-256"));
}

#[test]
fn closing_a_booking_that_is_not_there_changes_nothing() {
    let fixture = Fixture::new();
    let (_, alice) = fixture.signed_in("alice@qq.com", 1);
    book(&fixture, &alice, &offered("m6-16-256", Period::Day), 0);
    for id in ["2", "0", "18446744073709551615"] {
        let refused = fixture.run(&["close-booking", "--id", id]);
        assert_eq!(refused.code, ExitCode::FAILURE, "{id}");
        assert_eq!(
            refused.err,
            format!(
                "mooring-portal: there is no booking #{id}; nothing changed (`bookings` lists \
                 them)\n"
            )
        );
        assert!(refused.out.is_empty());
    }
    assert_eq!(fixture.store().bookings().unwrap().len(), 1);
    // A number that is not one is the command line's error.
    for line in [
        &["close-booking"][..],
        &["close-booking", "--id", "one"],
        &["close-booking", "--id", "-1"],
    ] {
        assert_eq!(fixture.run(line).code, ExitCode::from(2), "{line:?}");
    }
}

#[test]
fn a_close_whose_report_cannot_be_written_still_erases() {
    let fixture = Fixture::new();
    let _server = fixture.store();
    let (_, alice) = fixture.signed_in("alice@qq.com", 1);
    let gone = BookingTerms {
        machine: MachineId::parse("zz-gone-machine").unwrap(),
        ..offered("m4-16-256", Period::Week)
    };
    book(&fixture, &alice, &gone, 0);
    let db = fixture.db();
    let line = [
        "mooring-portal",
        "--db",
        db.to_str().unwrap(),
        "close-booking",
        "--id",
        "1",
    ];
    let mut out = FailAfter(0);
    let mut err = Vec::new();
    let code = crate::run(line, &fixture.clock, &mut out, &mut err);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "mooring-portal: cannot write the output: pipe closed\n"
    );
    assert!(!fixture.files_hold(b"zz-gone-machine"));
    assert_eq!(fixture.store().bookings().unwrap(), []);
    // A listing whose output fails says so.
    let mut out = FailAfter(10);
    let mut err = Vec::new();
    let line = ["mooring-portal", "--db", db.to_str().unwrap(), "bookings"];
    assert_eq!(
        crate::run(line, &fixture.clock, &mut out, &mut err),
        ExitCode::FAILURE
    );
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "mooring-portal: cannot write the output: pipe closed\n"
    );
    let listed = run_line(
        &["mooring-portal", "--db", db.to_str().unwrap(), "bookings"],
        &fixture.clock,
    );
    assert!(listed.out.ends_with("0 booking(s)\n"), "{}", listed.out);
}
