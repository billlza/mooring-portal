//! Mac rental bookings: made for the signed-in account alone, one per
//! machine, withdrawn by their owner, closed by the operator, and gone
//! with their account.

use portal_core::rental::{BookingTerms, Machine, MachineId, Period};

use super::*;
use crate::schema::MIGRATIONS;
use crate::{Booking, BookingError, OpenMode, OperatorBooking, StoreError};

/// The terms `id` is offered on for `period`.
fn terms(id: &str, period: Period) -> BookingTerms {
    BookingTerms::of(Machine::find(id).unwrap(), period)
}

/// `account` signed in for a day from [`NOW`] with the token digest
/// `byte`.
fn signed_in(store: &mut Store, account: &Account, byte: u8) -> TokenDigest {
    store
        .start_session(&session(account, byte, NOW, 86_400))
        .unwrap();
    digest(byte)
}

fn machine(id: &str) -> MachineId {
    MachineId::parse(id).unwrap()
}

#[test]
fn a_booking_is_made_for_the_signed_in_account_on_the_offers_terms() {
    let mut store = store();
    let alice = register(&mut store, "alice@qq.com", 1);
    let bob = register(&mut store, "bob@163.com", 2);
    let session = signed_in(&mut store, &alice, 1);
    let asked = terms("m6-24-512", Period::Month);
    store.book(&session, &asked, at(NOW + 5)).unwrap();
    let booking = Booking {
        id: 1,
        terms: asked,
        placed_at: at(NOW + 5),
    };
    assert_eq!(
        store.bookings_of(alice.id).unwrap(),
        std::slice::from_ref(&booking)
    );
    assert_eq!(store.bookings_of(bob.id).unwrap(), []);
    assert_eq!(
        store.bookings().unwrap(),
        [OperatorBooking {
            booking,
            account: alice.code,
            email: email("alice@qq.com"),
        }]
    );
}

#[test]
fn booking_a_machine_again_keeps_or_replaces_its_booking() {
    let mut store = store();
    let alice = register(&mut store, "alice@qq.com", 1);
    let session = signed_in(&mut store, &alice, 1);
    store
        .book(&session, &terms("m6-16-256", Period::Month), at(NOW))
        .unwrap();
    // Asked again on the same terms (a second click), it stays as it was.
    store
        .book(&session, &terms("m6-16-256", Period::Month), at(NOW + 60))
        .unwrap();
    let kept = store.bookings_of(alice.id).unwrap();
    assert_eq!(
        kept.iter()
            .map(|booking| (booking.id, booking.placed_at))
            .collect::<Vec<_>>(),
        [(1, at(NOW))]
    );
    // On another period it is replaced, under a new number.
    store
        .book(
            &session,
            &terms("m6-16-256", Period::Quarter),
            at(NOW + 120),
        )
        .unwrap();
    // Another machine is another booking.
    store
        .book(&session, &terms("m4-16-256", Period::Day), at(NOW + 180))
        .unwrap();
    let now_held: Vec<(u64, &str, Period, UnixTime)> = store
        .bookings_of(alice.id)
        .unwrap()
        .iter()
        .map(|booking| {
            (
                booking.id,
                Machine::find(booking.terms.machine.as_str()).unwrap().id,
                booking.terms.period,
                booking.placed_at,
            )
        })
        .collect();
    assert_eq!(
        now_held,
        [
            (2, "m6-16-256", Period::Quarter, at(NOW + 120)),
            (3, "m4-16-256", Period::Day, at(NOW + 180)),
        ]
    );
    // On other prices for the same period (the offer changed), it is
    // replaced too.
    let mut dearer = terms("m4-16-256", Period::Day);
    dearer.price += 100;
    dearer.first_price += 100;
    store.book(&session, &dearer, at(NOW + 240)).unwrap();
    let ids: Vec<u64> = store
        .bookings_of(alice.id)
        .unwrap()
        .iter()
        .map(|booking| booking.id)
        .collect();
    assert_eq!(ids, [2, 4]);
}

#[test]
fn only_a_live_session_books_or_withdraws() {
    let mut store = store();
    let alice = register(&mut store, "alice@qq.com", 1);
    let session = signed_in(&mut store, &alice, 1);
    let asked = terms("m4-16-256", Period::Week);
    for (who, when) in [(digest(9), NOW), (session, NOW + 86_400)] {
        assert!(matches!(
            store.book(&who, &asked, at(when)),
            Err(BookingError::SignedOut)
        ));
    }
    assert_eq!(store.bookings().unwrap(), []);
    store.book(&session, &asked, at(NOW)).unwrap();
    for (who, when) in [(digest(9), NOW), (session, NOW + 86_400)] {
        assert!(matches!(
            store.withdraw_booking(&who, &machine("m4-16-256"), at(when)),
            Err(BookingError::SignedOut)
        ));
    }
    assert_eq!(store.bookings_of(alice.id).unwrap().len(), 1);
}

#[test]
fn a_booking_is_withdrawn_by_its_owner_alone() {
    let mut store = store();
    let alice = register(&mut store, "alice@qq.com", 1);
    let bob = register(&mut store, "bob@163.com", 2);
    let alices = signed_in(&mut store, &alice, 1);
    let bobs = signed_in(&mut store, &bob, 2);
    for session in [&alices, &bobs] {
        store
            .book(session, &terms("m6-16-256", Period::Month), at(NOW))
            .unwrap();
    }
    store
        .book(&alices, &terms("m4-16-256", Period::Day), at(NOW))
        .unwrap();
    // Bob's withdrawal takes his own booking of the machine, not Alice's.
    assert!(
        store
            .withdraw_booking(&bobs, &machine("m6-16-256"), at(NOW))
            .unwrap()
    );
    assert!(
        !store
            .withdraw_booking(&bobs, &machine("m4-16-256"), at(NOW))
            .unwrap()
    );
    assert_eq!(store.bookings_of(bob.id).unwrap(), []);
    assert_eq!(store.bookings_of(alice.id).unwrap().len(), 2);
    assert!(
        store
            .withdraw_booking(&alices, &machine("m6-16-256"), at(NOW))
            .unwrap()
    );
    // Withdrawn twice (a second click), it is simply gone.
    assert!(
        !store
            .withdraw_booking(&alices, &machine("m6-16-256"), at(NOW))
            .unwrap()
    );
    let left: Vec<String> = store
        .bookings_of(alice.id)
        .unwrap()
        .into_iter()
        .map(|booking| booking.terms.machine.to_string())
        .collect();
    assert_eq!(left, ["m4-16-256"]);
}

#[test]
fn the_operator_closes_a_booking_by_its_number() {
    let mut store = store();
    let alice = register(&mut store, "alice@qq.com", 1);
    let session = signed_in(&mut store, &alice, 1);
    for (id, when) in [("m6-24-512", NOW), ("m4-16-256", NOW + 1)] {
        store
            .book(&session, &terms(id, Period::Year), at(when))
            .unwrap();
    }
    let listed = store.bookings().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|listed| listed.booking.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let closed = store.close_booking(1).unwrap().unwrap();
    assert_eq!(closed, listed[0]);
    assert_eq!(store.bookings().unwrap(), [listed[1].clone()]);
    for gone in [1, 3, u64::MAX] {
        assert_eq!(store.close_booking(gone).unwrap(), None, "{gone}");
    }
    // A number is never given again, even to the next booking after the
    // newest is closed.
    store.close_booking(2).unwrap().unwrap();
    store
        .book(&session, &terms("m4-16-256", Period::Year), at(NOW + 2))
        .unwrap();
    assert_eq!(store.bookings_of(alice.id).unwrap()[0].id, 3);
}

#[test]
fn bookings_go_with_their_account() {
    let mut store = store();
    let alice = register(&mut store, "alice@qq.com", 1);
    let bob = register(&mut store, "bob@163.com", 2);
    for (account, byte) in [(&alice, 1), (&bob, 2)] {
        let session = signed_in(&mut store, account, byte);
        store
            .book(&session, &terms("m6-16-256", Period::Week), at(NOW))
            .unwrap();
    }
    store.delete_account(&alice.code).unwrap();
    store.ban(&bob.code, &[], at(NOW)).unwrap();
    let left: i64 = store
        .conn
        .query_row("SELECT count(*) FROM mac_bookings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_booking_that_no_longer_parses_is_corrupt() {
    for (column, value, what) in [
        ("id", "-5", "booking's number"),
        ("machine", "'M6'", "booking's machine"),
        ("chip", "''", "booking's chip"),
        ("memory_gb", "70000", "booking's memory"),
        ("storage_gb", "-1", "booking's storage"),
        ("period", "'hour'", "booking's period"),
        ("price", "-1", "booking's price"),
        ("first_price", "-1", "booking's first price"),
        ("placed_at", "-1", "booking's time"),
    ] {
        let mut store = store();
        let alice = register(&mut store, "alice@qq.com", 1);
        let session = signed_in(&mut store, &alice, 1);
        store
            .book(&session, &terms("m6-16-256", Period::Week), at(NOW))
            .unwrap();
        // As a hand edit could leave it: the period's check is the
        // schema's, so it is set aside to write one.
        store
            .conn
            .pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        store
            .conn
            .execute(&format!("UPDATE mac_bookings SET {column} = {value}"), [])
            .unwrap();
        for read in [
            store.bookings_of(alice.id).map(|_| ()),
            store.bookings().map(|_| ()),
            store.close_booking(1).map(|_| ()),
        ] {
            assert!(
                matches!(read, Err(StoreError::Corrupt { what: found, .. }) if found == what)
                    // Renumbered, it is not booking 1.
                    || (column == "id" && read.is_ok()),
                "{column}"
            );
        }
        // Booking the machine again reads its booking first, and fails the
        // same way; a row that no longer names the machine is not read.
        let rebooked = store.book(&session, &terms("m6-16-256", Period::Day), at(NOW));
        if column == "machine" {
            rebooked.unwrap();
        } else {
            assert!(
                matches!(
                    rebooked,
                    Err(BookingError::Store(StoreError::Corrupt { what: found, .. }))
                        if found == what
                ),
                "{column}"
            );
        }
    }
}

#[test]
fn a_version_5_database_gains_bookings() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_file(&dir);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
    }
    let mut store = Store::open(&path, OpenMode::Existing).unwrap();
    let alice = register(&mut store, "alice@qq.com", 1);
    let session = signed_in(&mut store, &alice, 1);
    store
        .book(&session, &terms("m5pro-24-512", Period::Month), at(NOW))
        .unwrap();
    assert_eq!(store.bookings_of(alice.id).unwrap().len(), 1);
    let version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
}
