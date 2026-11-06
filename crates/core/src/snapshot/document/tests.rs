use std::num::NonZeroU64;

use super::*;
use crate::snapshot::{QuotaAction, SuspendReason, WarnPercent};
use crate::time::CalendarDate;

fn at(secs: u64) -> UnixTime {
    UnixTime::from_secs(secs).unwrap()
}

fn id(text: &str) -> MooringUserId {
    MooringUserId::parse(text).unwrap()
}

/// November 2026: 2026-11-01T00:00:00Z to 2026-12-01T00:00:00Z.
fn november() -> BillingCycle {
    BillingCycle::new(
        CalendarDate::parse("2026-11-01").unwrap(),
        CalendarDate::parse("2026-12-01").unwrap(),
    )
    .unwrap()
}

const NOV_1: u64 = 1_793_491_200;
const NOV_5: u64 = 1_793_836_800;
const DEC_1: u64 = 1_796_083_200;

fn record(used: Option<u64>) -> UserRecord {
    UserRecord {
        status: UserStatus::Active,
        cycle_served_bytes: used,
        reality_downlink_counted_at_close: true,
        quota: Some(Quota {
            cycle_served_bytes: NonZeroU64::new(40_000_000_000).unwrap(),
            warn_percent: WarnPercent::new(80).unwrap(),
            action: QuotaAction::Suspend,
        }),
        expires_at: Some(at(DEC_1 + 30 * 86_400)),
    }
}

#[test]
fn the_constants_name_the_contract() {
    assert_eq!(november().starts_at(), at(NOV_1));
    assert_eq!(november().ends_at(), at(DEC_1));
    assert_eq!(at(NOV_5).to_string(), "2026-11-05T00:00:00Z");
}

#[test]
fn a_valid_snapshot_keeps_users_in_id_order() {
    let snapshot = Snapshot::new(
        at(NOV_5),
        november(),
        Some(at(NOV_5 - 300)),
        [
            (id("zzzzzzzzzz"), record(Some(5))),
            (id("aaaaaaaaaa"), record(None)),
        ],
    )
    .unwrap();
    let ids: Vec<&str> = snapshot.users().map(|(user, _)| user.as_str()).collect();
    assert_eq!(ids, ["aaaaaaaaaa", "zzzzzzzzzz"]);
    assert_eq!(snapshot.generated_at(), at(NOV_5));
    assert_eq!(snapshot.cycle(), november());
    assert_eq!(snapshot.counted_through(), Some(at(NOV_5 - 300)));
    let parts: Vec<UserSnapshot> = snapshot.per_user().collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1].user(), &id("zzzzzzzzzz"));
    assert_eq!(parts[1].record().cycle_served_bytes, Some(5));
    assert_eq!(parts[1].generated_at(), at(NOV_5));
    assert_eq!(parts[1].cycle(), november());
    assert_eq!(parts[1].counted_through(), Some(at(NOV_5 - 300)));
}

#[test]
fn counted_usage_must_lie_in_the_cycle_and_before_generation() {
    let users = || [(id("aaaaaaaaaa"), record(Some(1)))];
    // Both ends of the cycle are allowed.
    assert!(Snapshot::new(at(DEC_1), november(), Some(at(NOV_1)), users()).is_ok());
    assert!(Snapshot::new(at(DEC_1), november(), Some(at(DEC_1)), users()).is_ok());
    assert!(matches!(
        Snapshot::new(at(DEC_1 + 10), november(), Some(at(DEC_1 + 1)), users()),
        Err(SnapshotError::CountedThroughOutsideCycle { through }) if through == at(DEC_1 + 1)
    ));
    assert!(matches!(
        Snapshot::new(at(NOV_5), november(), Some(at(NOV_1 - 1)), users()),
        Err(SnapshotError::CountedThroughOutsideCycle { .. })
    ));
    assert!(matches!(
        Snapshot::new(at(NOV_5), november(), Some(at(NOV_5 + 1)), users()),
        Err(SnapshotError::CountedThroughAfterGeneration { through, generated_at })
            if through == at(NOV_5 + 1) && generated_at == at(NOV_5)
    ));
    assert!(matches!(
        Snapshot::new(
            at(NOV_1 - 1),
            november(),
            None,
            [(id("aaaaaaaaaa"), record(None))]
        ),
        Err(SnapshotError::CycleAfterGeneration { .. })
    ));
}

#[test]
fn usage_figures_need_a_usage_report() {
    assert!(
        Snapshot::new(
            at(NOV_5),
            november(),
            None,
            [(id("aaaaaaaaaa"), record(None))]
        )
        .is_ok()
    );
    assert!(matches!(
        Snapshot::new(at(NOV_5), november(), None, [(id("aaaaaaaaaa"), record(Some(0)))]),
        Err(SnapshotError::UsageWithoutReport { user }) if user == id("aaaaaaaaaa")
    ));
}

#[test]
fn users_are_unique_present_and_not_suspended_in_the_future() {
    assert!(matches!(
        Snapshot::new(at(NOV_5), november(), None, []),
        Err(SnapshotError::NoUsers)
    ));
    assert!(matches!(
        Snapshot::new(
            at(NOV_5),
            november(),
            None,
            [(id("aaaaaaaaaa"), record(None)), (id("aaaaaaaaaa"), record(None))]
        ),
        Err(SnapshotError::DuplicateUser { user }) if user == id("aaaaaaaaaa")
    ));
    let suspended = |since| UserRecord {
        status: UserStatus::Suspended {
            reason: SuspendReason::Operator,
            since: at(since),
        },
        ..record(None)
    };
    assert!(
        Snapshot::new(
            at(NOV_5),
            november(),
            None,
            [(id("aaaaaaaaaa"), suspended(NOV_5))]
        )
        .is_ok()
    );
    assert!(matches!(
        Snapshot::new(at(NOV_5), november(), None, [(id("aaaaaaaaaa"), suspended(NOV_5 + 1))]),
        Err(SnapshotError::SuspendedAfterGeneration { since, .. }) if since == at(NOV_5 + 1)
    ));
}

#[test]
fn a_user_snapshot_round_trips_through_its_stored_document() {
    let snapshot = Snapshot::new(
        at(NOV_5),
        november(),
        Some(at(NOV_5 - 60)),
        [
            (id("abcdefghij"), record(Some(12_345))),
            (id("bcdefghijk"), record(None)),
        ],
    )
    .unwrap();
    for part in snapshot.per_user() {
        let stored = part.to_document().unwrap();
        assert_eq!(UserSnapshot::from_document(&stored).unwrap(), part);
    }
    let first = snapshot.per_user().next().unwrap();
    assert_eq!(
        first.to_document().unwrap(),
        concat!(
            r#"{"format":"mooring-portal-snapshot","version":1,"generated_at":1793836800,"#,
            r#""cycle":{"start":"2026-11-01","next_start":"2026-12-01"},"counted_through":1793836740,"#,
            r#""users":[{"user":"abcdefghij","status":"active","cycle_served_bytes":12345,"#,
            r#""reality_downlink_counted_at_close":true,"#,
            r#""quota":{"cycle_served_bytes":40000000000,"warn_percent":80,"action":"suspend"},"#,
            r#""expires_at":1798675200}]}"#
        )
    );
    // A stored record holds one user; the whole snapshot holds two.
    let pretty = snapshot.to_json_pretty().unwrap();
    assert!(matches!(
        UserSnapshot::from_document(&pretty),
        Err(SnapshotError::NotOneUser { found: 2 })
    ));
    assert_eq!(Snapshot::parse(pretty.as_bytes()).unwrap(), snapshot);
}

#[test]
fn documents_are_bounded_and_identified_before_decoding() {
    let large = vec![b' '; MAX_DOCUMENT_BYTES + 1];
    assert!(matches!(
        Snapshot::parse(&large),
        Err(SnapshotError::TooLarge { size }) if size == MAX_DOCUMENT_BYTES + 1
    ));
    assert!(matches!(
        Snapshot::parse(br#"{"format":"mooring-state","version":3}"#),
        Err(SnapshotError::WrongFormat { found }) if found == "mooring-state"
    ));
    assert!(matches!(
        Snapshot::parse(br#"{"format":"mooring-portal-snapshot","version":2,"anything":true}"#),
        Err(SnapshotError::UnsupportedVersion { found: 2 })
    ));
    for bad in [&b"not json"[..], b"[]", b"{}", br#"{"version":1}"#] {
        assert!(matches!(Snapshot::parse(bad), Err(SnapshotError::Json(_))));
    }
}
