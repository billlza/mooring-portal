//! The journal Mooring pulls, its token and its cursor.

use portal_core::payment::OrderId;
use portal_core::payment::feed::{Entry, EntryKind};
use portal_core::payment::stripe::{BillingReason, Event};

use super::paying::*;
use super::*;
use crate::{Progress, PullError, PullState, Pulled, Recorded, StoreError};

/// A store whose journal holds five entries: two customers' grants, a
/// refund and a dispute of the first's, and a renewal of the second's.
fn journalled() -> (Store, Account, Account) {
    let mut store = shop();
    let first = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let second = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let month_order = place(&mut store, 1, &month(), 1);
    let auto_order = place(&mut store, 2, &auto(), 2);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, month_order, &month())),
            event(2, 61, charged("pi_1", 1)),
            event(3, 70, completed(2, auto_order, &auto())),
            event(4, 71, invoice_paid_by("in_2", "pi_2")),
            event(5, 72, charged("pi_2", 2)),
            event(6, 500, refunded("pi_1", 300, false)),
            event(7, 900, invoiced("in_3", "sub_2", BillingReason::Cycle, 750)),
            event(8, 901, invoice_paid_by("in_3", "pi_3")),
            event(9, 902, charged("pi_3", 2)),
            event(10, 950, disputed("pi_1", 800)),
        ],
    );
    (store, first, second)
}

fn pull(store: &mut Store, after: u64, limit: u32) -> Result<Pulled, PullError> {
    store.pull_entitlements(&digest(77), after, limit, at(NOW + 5_000))
}

#[test]
fn the_journal_gives_each_entry_with_its_orders_terms() {
    let (mut store, ..) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    let renewal = OrderId::for_renewal(&sid("in_3"));
    let of_month = |seq, kind, recorded: u64| Entry {
        seq,
        kind,
        order: oid(1),
        user: user("pilotusera"),
        terms: month(),
        paid_at: at(NOW + 60),
        renewal_of: None,
        recorded_at: at(NOW + recorded),
    };
    let of_auto = |seq, order, paid: u64, renewal_of, recorded: u64| Entry {
        seq,
        kind: EntryKind::Grant,
        order,
        user: user("pilotuserb"),
        terms: auto(),
        paid_at: at(NOW + paid),
        renewal_of,
        recorded_at: at(NOW + recorded),
    };
    assert_eq!(
        pull(&mut store, 0, 200).unwrap(),
        Pulled {
            entries: vec![
                of_month(1, EntryKind::Grant, 62),
                of_auto(2, oid(2), 70, None, 73),
                of_month(
                    3,
                    EntryKind::Refund {
                        amount_refunded: 300,
                        in_full: false,
                    },
                    501
                ),
                of_auto(4, renewal, 900, Some(oid(2)), 903),
                of_month(
                    5,
                    EntryKind::Dispute {
                        amount_disputed: 800
                    },
                    951
                ),
            ],
            more: false,
            livemode: false,
        }
    );
}

#[test]
fn a_document_says_which_money_the_database_holds() {
    // The same entries, of a database of real payments.
    let mut live = store();
    live.enter_payment_mode(true).unwrap();
    customer(&mut live, "a@qq.com", 1, "pilotusera");
    let order = place(&mut live, 1, &month(), 1);
    for (n, kind) in [(1, completed(1, order, &month())), (2, charged("pi_1", 1))] {
        let event = Event {
            livemode: true,
            ..event(n, 60, kind)
        };
        assert_eq!(record(&mut live, &event), Recorded::New);
    }
    live.set_pull_token(&digest(77), at(NOW)).unwrap();
    let pulled = pull(&mut live, 0, 10).unwrap();
    assert!(pulled.livemode);
    assert_eq!(pulled.entries.len(), 1);
    let (mut rehearsal, ..) = journalled();
    rehearsal.set_pull_token(&digest(77), at(NOW)).unwrap();
    assert!(!pull(&mut rehearsal, 0, 10).unwrap().livemode);

    // A database that has entered no mode has nothing of Mooring's, and
    // says so rather than answer with nothing; the cursor is not noted.
    let mut unsold = store();
    unsold.set_pull_token(&digest(77), at(NOW)).unwrap();
    let refused = pull(&mut unsold, 0, 10);
    assert!(matches!(refused, Err(PullError::NoMode)), "{refused:?}");
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the database has never been served with payments"
    );
    assert_eq!(unsold.pull_state().unwrap().pulled_at, None);
    // The token is asked for first: a stranger learns nothing of it.
    assert!(matches!(
        unsold.pull_entitlements(&digest(78), 0, 10, at(NOW)),
        Err(PullError::Unauthorized)
    ));
}

#[test]
fn a_refund_says_whether_it_is_of_the_whole_payment() {
    let refunds = |store: &mut Store| -> Vec<(u64, bool)> {
        pull(store, 0, 200)
            .unwrap()
            .entries
            .iter()
            .filter_map(|entry| match entry.kind {
                EntryKind::Refund {
                    amount_refunded,
                    in_full,
                } => Some((amount_refunded, in_full)),
                _ => None,
            })
            .collect()
    };
    let (mut store, first, _) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    assert_eq!(refunds(&mut store), [(300, false)]);
    // The rest goes back: the journal says all of it has.
    record_all(&mut store, &[event(20, 2_000, refunded("pi_1", 800, true))]);
    assert_eq!(refunds(&mut store), [(300, false), (800, true)]);
    // Told again, or told late of the part that went back first, nothing
    // is added and nothing is taken back.
    record_all(
        &mut store,
        &[
            event(21, 2_001, refunded("pi_1", 800, true)),
            event(22, 2_002, refunded("pi_1", 300, false)),
        ],
    );
    assert_eq!(refunds(&mut store), [(300, false), (800, true)]);
    let order = order_of(&store, &first, oid(1));
    assert_eq!((order.refunded, order.refunded_in_full), (800, true));

    // Were Stripe to say "all of it" of an amount the journal already
    // has, the journal says so too: the amount alone does not tell.
    let (mut store, ..) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    record_all(
        &mut store,
        &[
            event(20, 2_000, refunded("pi_2", 750, false)),
            event(21, 2_001, refunded("pi_2", 750, true)),
            event(22, 2_002, refunded("pi_2", 750, true)),
        ],
    );
    assert_eq!(
        refunds(&mut store),
        [(300, false), (750, false), (750, true)]
    );
}

#[test]
fn entries_come_after_the_cursor_a_page_at_a_time() {
    let (mut store, ..) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    let numbers =
        |pulled: &Pulled| -> Vec<u64> { pulled.entries.iter().map(|entry| entry.seq).collect() };
    for (after, limit, entries, more) in [
        (0, 2, vec![1, 2], true),
        (2, 2, vec![3, 4], true),
        (4, 2, vec![5], false),
        (5, 2, vec![], false),
        (0, 5, vec![1, 2, 3, 4, 5], false),
        (0, 4, vec![1, 2, 3, 4], true),
        (3, 1, vec![4], true),
        (0, 0, vec![], true),
        (5, 0, vec![], false),
    ] {
        let pulled = pull(&mut store, after, limit).unwrap();
        assert_eq!(numbers(&pulled), entries, "after {after}, {limit} at most");
        assert_eq!(pulled.more, more, "after {after}, {limit} at most");
    }
}

#[test]
fn only_the_holder_of_the_token_pulls() {
    let (mut store, ..) = journalled();
    // No token is set: nobody pulls.
    assert!(matches!(
        pull(&mut store, 0, 10),
        Err(PullError::Unauthorized)
    ));
    assert!(!store.is_pull_token(&digest(77)).unwrap());
    store.set_pull_token(&digest(77), at(NOW + 10)).unwrap();
    assert!(store.is_pull_token(&digest(77)).unwrap());
    assert!(!store.is_pull_token(&digest(78)).unwrap());
    assert!(pull(&mut store, 0, 10).is_ok());
    assert!(matches!(
        store.pull_entitlements(&digest(78), 0, 10, at(NOW)),
        Err(PullError::Unauthorized)
    ));
    // A refused pull moves nothing.
    assert_eq!(store.pull_state().unwrap().pulled_through, 0);
    // A new token takes the old one's place; what was pulled stands.
    pull(&mut store, 3, 10).unwrap();
    store.set_pull_token(&digest(79), at(NOW + 20)).unwrap();
    assert_eq!(store.pull_state().unwrap().pulled_through, 3);
    assert!(matches!(
        pull(&mut store, 0, 10),
        Err(PullError::Unauthorized)
    ));
    assert!(
        store
            .pull_entitlements(&digest(79), 3, 10, at(NOW + 6_000))
            .is_ok()
    );
    assert_eq!(
        store.pull_state().unwrap(),
        PullState {
            token_created_at: Some(at(NOW + 20)),
            pulled_through: 3,
            pulled_at: Some(at(NOW + 6_000)),
            last_seq: 5,
        }
    );
    // Revoked, nobody pulls; the cursor stays.
    assert!(store.revoke_pull_token().unwrap());
    assert!(!store.is_pull_token(&digest(79)).unwrap());
    assert!(!store.revoke_pull_token().unwrap());
    assert!(matches!(
        store.pull_entitlements(&digest(79), 3, 10, at(NOW)),
        Err(PullError::Unauthorized)
    ));
    assert_eq!(
        store.pull_state().unwrap(),
        PullState {
            token_created_at: None,
            pulled_through: 3,
            pulled_at: Some(at(NOW + 6_000)),
            last_seq: 5,
        }
    );
    assert_eq!(
        PullError::Unauthorized.to_string(),
        "the token is not the one Mooring pulls with"
    );
}

#[test]
fn the_cursor_says_what_mooring_has_and_never_goes_back() {
    let (mut store, first, second) = journalled();
    assert_eq!(
        store.pull_state().unwrap(),
        PullState {
            token_created_at: None,
            pulled_through: 0,
            pulled_at: None,
            last_seq: 5,
        }
    );
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    let granted = |store: &Store, account: &Account, order| match progress(store, account, order) {
        Progress::Granted { pulled } => pulled,
        other => panic!("{other:?}"),
    };
    // Asking from the start says Mooring has nothing yet.
    pull(&mut store, 0, 200).unwrap();
    assert!(!granted(&store, &first, oid(1)));
    assert!(!granted(&store, &second, oid(2)));
    // Asking after the first entry says Mooring has it.
    pull(&mut store, 1, 200).unwrap();
    assert!(granted(&store, &first, oid(1)));
    assert!(!granted(&store, &second, oid(2)));
    pull(&mut store, 4, 200).unwrap();
    assert!(granted(&store, &second, oid(2)));
    assert!(granted(&store, &second, OrderId::for_renewal(&sid("in_3"))));
    // Asking from an earlier entry again takes nothing back.
    let again = pull(&mut store, 1, 200).unwrap();
    assert_eq!(again.entries.len(), 4);
    assert_eq!(store.pull_state().unwrap().pulled_through, 4);
    assert!(granted(&store, &second, oid(2)));
}

#[test]
fn a_cursor_beyond_the_journal_is_refused() {
    let (mut store, ..) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    assert!(pull(&mut store, 5, 10).is_ok());
    let refused = pull(&mut store, 6, 10);
    assert!(
        matches!(
            refused,
            Err(PullError::Ahead {
                after: 6,
                last_seq: 5
            })
        ),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the cursor 6 is beyond the journal's last entry, 5: this is not the journal the \
         cursor was read from"
    );
    assert_eq!(store.pull_state().unwrap().pulled_through, 5);
    // An empty journal has given out no number.
    let mut empty = shop();
    empty.set_pull_token(&digest(77), at(NOW)).unwrap();
    assert_eq!(
        pull(&mut empty, 0, 10).unwrap(),
        Pulled {
            entries: vec![],
            more: false,
            livemode: false,
        }
    );
    assert!(matches!(
        pull(&mut empty, 1, 10),
        Err(PullError::Ahead {
            after: 1,
            last_seq: 0
        })
    ));
    assert!(matches!(
        pull(&mut empty, u64::MAX, 10),
        Err(PullError::Ahead { .. })
    ));
}

#[test]
fn entries_that_go_with_their_account_leave_the_numbers_as_they_were() {
    let (mut store, first, _) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    // A ban takes the first customer's orders, and their entries.
    store.ban(&first.code, &[], at(NOW)).unwrap();
    let left: Vec<u64> = pull(&mut store, 0, 10)
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.seq)
        .collect();
    assert_eq!(left, [2, 4]);
    // The last number given out is still the last: a cursor at it is
    // answered, and the next entry gets a number beyond it.
    assert_eq!(store.pull_state().unwrap().last_seq, 5);
    assert!(pull(&mut store, 5, 10).is_ok());
    record_all(&mut store, &[event(20, 2_000, refunded("pi_2", 10, false))]);
    assert_eq!(
        pull(&mut store, 5, 10).unwrap().entries[0].seq,
        6,
        "numbers are never given out twice"
    );
}

#[test]
fn a_journal_that_no_longer_parses_is_reported_as_corrupt() {
    for (change, what) in [
        (
            "UPDATE entitlements SET recorded_at = -1",
            "journal entry's time",
        ),
        ("UPDATE orders SET mooring_user = 'nope'", "Mooring user id"),
        ("UPDATE orders SET plan = 'No Plan'", "order's plan"),
        (
            "UPDATE orders SET renewal_of = 'short' WHERE id = '11111111111111111111'",
            "order id",
        ),
        (
            "UPDATE entitlements SET amount = -5 WHERE kind = 'refund'",
            "journal entry's amount",
        ),
    ] {
        let (mut store, ..) = journalled();
        store.set_pull_token(&digest(77), at(NOW)).unwrap();
        store
            .conn
            .execute_batch(&format!("PRAGMA foreign_keys = OFF; {change};"))
            .unwrap();
        let read = pull(&mut store, 0, 10);
        assert!(
            matches!(&read, Err(PullError::Store(StoreError::Corrupt { what: found, .. })) if *found == what),
            "{change}: {read:?}"
        );
    }
    // An entry whose order was never paid cannot have been written; nor
    // one of no kind, or with what belongs to another kind.
    for (change, what) in [
        ("UPDATE orders SET paid_at = NULL", "journal entry"),
        (
            "UPDATE entitlements SET kind = 'gift' WHERE seq = 1",
            "journal entry's kind",
        ),
        (
            "UPDATE entitlements SET in_full = NULL WHERE kind = 'refund'",
            "journal entry's kind",
        ),
        (
            "UPDATE entitlements SET in_full = 1 WHERE kind = 'grant'",
            "journal entry's kind",
        ),
        (
            "UPDATE entitlements SET in_full = 0 WHERE kind = 'dispute'",
            "journal entry's kind",
        ),
        (
            "UPDATE entitlements SET amount = 5 WHERE kind = 'grant'",
            "journal entry's kind",
        ),
    ] {
        let (mut store, ..) = journalled();
        store.set_pull_token(&digest(77), at(NOW)).unwrap();
        store
            .conn
            .execute_batch(&format!("PRAGMA ignore_check_constraints = ON; {change};"))
            .unwrap();
        let read = pull(&mut store, 0, 10);
        assert!(
            matches!(&read, Err(PullError::Store(StoreError::Corrupt { what: found, .. })) if *found == what),
            "{change}: {read:?}"
        );
    }
    // What is kept of Mooring's pulls is checked as it is read, too.
    for (change, what) in [
        ("pulled_through = -1", "pull cursor"),
        ("pulled_at = -1", "last pull's time"),
        ("token_created_at = -1", "pull token's time"),
    ] {
        let (mut store, ..) = journalled();
        store.set_pull_token(&digest(77), at(NOW)).unwrap();
        pull(&mut store, 1, 10).unwrap();
        store
            .conn
            .execute(&format!("UPDATE pull_access SET {change}"), [])
            .unwrap();
        let read = store.pull_state();
        assert!(
            matches!(&read, Err(StoreError::Corrupt { what: found, .. }) if *found == what),
            "{change}: {read:?}"
        );
    }
    // What the schema refuses cannot be stored at all.
    let (mut store, ..) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    for refused in [
        "UPDATE entitlements SET kind = 'gift'",
        "UPDATE entitlements SET amount = 1 WHERE kind = 'grant'",
        "UPDATE entitlements SET amount = NULL WHERE kind = 'refund'",
        "UPDATE entitlements SET in_full = NULL WHERE kind = 'refund'",
        "UPDATE entitlements SET in_full = 2 WHERE kind = 'refund'",
        "UPDATE entitlements SET in_full = 0 WHERE kind = 'grant'",
        "UPDATE entitlements SET in_full = 1 WHERE kind = 'dispute'",
        "INSERT INTO entitlements (order_id, kind, recorded_at) VALUES ('11111111111111111111', 'grant', 1)",
        "INSERT INTO entitlements (order_id, kind, amount, recorded_at) VALUES ('11111111111111111111', 'dispute', 1, 1)",
        "INSERT INTO entitlements (order_id, kind, recorded_at) VALUES ('99999999999999999999', 'grant', 1)",
        "INSERT INTO pull_access (id) VALUES (2)",
        "UPDATE pull_access SET token_digest = x'00'",
    ] {
        assert!(store.conn.execute(refused, []).is_err(), "{refused}");
    }
}

#[test]
fn a_pull_that_cannot_read_the_journal_fails_and_is_no_empty_answer() {
    let (mut store, ..) = journalled();
    store.set_pull_token(&digest(77), at(NOW)).unwrap();
    store
        .conn
        .execute_batch("DROP TABLE entitlements;")
        .unwrap();
    assert!(matches!(
        pull(&mut store, 0, 10),
        Err(PullError::Store(StoreError::Sqlite(_)))
    ));
    // The cursor it presented was not noted.
    assert_eq!(store.pull_state().unwrap().pulled_through, 0);
}
