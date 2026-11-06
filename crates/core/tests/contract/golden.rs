//! The golden cases: valid documents, refusals and stored records.

use std::collections::BTreeMap;
use std::fs;

use portal_core::snapshot::{
    QuotaAction, QuotaLevel, Snapshot, SnapshotError, SuspendReason, UserSnapshot, UserStatus,
};
use portal_core::time::UnixTime;

use crate::fixtures::{fixture, fixture_dir, id, pilot};

#[test]
fn valid_documents_decode_and_encode_back_to_the_same_text() {
    for name in ["valid/pilot.json", "valid/no-usage-report.json"] {
        let text = fixture(name);
        let snapshot = Snapshot::parse(text.as_bytes()).unwrap();
        assert_eq!(
            snapshot.to_json_pretty().unwrap(),
            text.trim_end(),
            "{name}"
        );
    }
}

#[test]
fn the_pilot_document_reads_as_the_portal_shows_it() {
    let snapshot = pilot();
    assert_eq!(snapshot.generated_at().to_string(), "2026-11-05T12:00:00Z");
    assert_eq!(snapshot.cycle().start().to_string(), "2026-11-01");
    assert_eq!(snapshot.cycle().next_start().to_string(), "2026-12-01");
    assert_eq!(
        snapshot.counted_through().map(|t| t.to_string()),
        Some("2026-11-05T11:55:00Z".to_owned())
    );
    let levels: BTreeMap<&str, Option<QuotaLevel>> = snapshot
        .users()
        .map(|(user, record)| {
            (
                user.as_str(),
                record
                    .quota
                    .map(|quota| quota.level(record.cycle_served_bytes)),
            )
        })
        .collect();
    assert_eq!(
        levels,
        BTreeMap::from([
            ("pilotusera", Some(QuotaLevel::Within)),
            ("pilotuserb", Some(QuotaLevel::Near)),
            ("pilotuserc", Some(QuotaLevel::Over)),
            ("pilotuserd", Some(QuotaLevel::Unknown)),
            ("pilotusere", None),
            ("pilotuserf", None),
            ("pilotuserg", None),
        ])
    );
    let records: BTreeMap<&str, _> = snapshot
        .users()
        .map(|(user, record)| (user.as_str(), *record))
        .collect();
    // Unknown usage stays unknown; a known zero stays zero.
    assert_eq!(records["pilotuserd"].cycle_served_bytes, None);
    assert_eq!(records["pilotuserf"].cycle_served_bytes, Some(0));
    assert_eq!(
        records["pilotuserd"].quota.map(|q| q.action),
        Some(QuotaAction::Warn)
    );
    assert_eq!(
        records["pilotuserc"].status,
        UserStatus::Suspended {
            reason: SuspendReason::QuotaExceeded,
            since: UnixTime::from_secs(1_793_784_600).unwrap(),
        }
    );
    assert_eq!(records["pilotuserf"].status, UserStatus::Revoked);
    assert!(records["pilotusera"].reality_downlink_counted_at_close);
    assert!(!records["pilotuserg"].reality_downlink_counted_at_close);
}

#[test]
fn a_snapshot_without_a_usage_report_has_no_figures() {
    let snapshot = Snapshot::parse(fixture("valid/no-usage-report.json").as_bytes()).unwrap();
    assert_eq!(snapshot.counted_through(), None);
    for (user, record) in snapshot.users() {
        assert_eq!(record.cycle_served_bytes, None, "{user}");
        if let Some(quota) = record.quota {
            assert_eq!(quota.level(record.cycle_served_bytes), QuotaLevel::Unknown);
        }
    }
}

#[test]
fn stored_records_are_one_user_documents_in_compact_form() {
    let snapshot = pilot();
    for user in ["pilotusera", "pilotuserg"] {
        let golden = fixture(&format!("stored/{user}.json"));
        let part = snapshot
            .per_user()
            .find(|part| part.user() == &id(user))
            .unwrap();
        assert_eq!(part.to_document().unwrap(), golden.trim_end(), "{user}");
        assert_eq!(
            UserSnapshot::from_document(golden.trim_end()).unwrap(),
            part
        );
    }
}

/// What an invalid fixture must be refused with.
enum Refusal {
    /// A field-level problem, named in the decoder's message.
    Field(&'static str),
    /// A cross-field rule.
    Rule(fn(&SnapshotError) -> bool),
}

#[test]
fn every_invalid_document_is_refused_for_its_rule() {
    use Refusal::{Field, Rule};
    let cases = [
        (
            "wrong-format",
            Rule(|e| matches!(e, SnapshotError::WrongFormat { found } if found == "mooring-state")),
        ),
        (
            "version-2",
            Rule(|e| matches!(e, SnapshotError::UnsupportedVersion { found: 2 })),
        ),
        ("missing-key", Field("missing field `expires_at`")),
        ("unknown-key-alias", Field("unknown field `alias`")),
        ("unknown-key-node", Field("unknown field `node`")),
        (
            "alias-as-user",
            Field("\"k7qd-m2xa\" is not a Mooring user id"),
        ),
        ("zero-quota", Field("expected a nonzero u64")),
        ("warn-percent-100", Field("100 is not a warning share")),
        (
            "empty-cycle",
            Field("the billing cycle must end after it starts"),
        ),
        ("revoke-pending", Field("unknown variant `revoke_pending`")),
        (
            "negative-usage",
            Field("invalid value: integer `-1`, expected u64"),
        ),
        (
            "expiry-after-9999",
            Field("the instant is after 9999-12-31T23:59:59Z"),
        ),
        ("truncated", Field("EOF while parsing")),
        (
            "usage-without-report",
            Rule(|e| matches!(e, SnapshotError::UsageWithoutReport { .. })),
        ),
        (
            "counted-through-outside-cycle",
            Rule(|e| matches!(e, SnapshotError::CountedThroughOutsideCycle { .. })),
        ),
        (
            "counted-through-after-generation",
            Rule(|e| matches!(e, SnapshotError::CountedThroughAfterGeneration { .. })),
        ),
        (
            "cycle-after-generation",
            Rule(|e| matches!(e, SnapshotError::CycleAfterGeneration { .. })),
        ),
        (
            "duplicate-user",
            Rule(|e| matches!(e, SnapshotError::DuplicateUser { .. })),
        ),
        ("no-users", Rule(|e| matches!(e, SnapshotError::NoUsers))),
        (
            "suspended-after-generation",
            Rule(|e| matches!(e, SnapshotError::SuspendedAfterGeneration { .. })),
        ),
    ];
    let listed = fs::read_dir(fixture_dir("invalid")).unwrap().count();
    assert_eq!(listed, cases.len(), "every invalid fixture has a case");
    for (name, refusal) in cases {
        let error =
            Snapshot::parse(fixture(&format!("invalid/{name}.json")).as_bytes()).expect_err(name);
        match refusal {
            Field(message) => {
                assert!(matches!(error, SnapshotError::Json(_)), "{name}: {error}");
                assert!(error.to_string().contains(message), "{name}: {error}");
            }
            Rule(check) => assert!(check(&error), "{name}: {error}"),
        }
    }
}
