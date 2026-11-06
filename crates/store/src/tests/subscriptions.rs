use super::*;
use crate::{ImportError, ImportReport, MAX_CLOCK_SKEW_SECS, StoreError, Subscription};

fn record_of(store: &Store, account: &Account) -> Option<u64> {
    match store.subscription(account.id).unwrap() {
        Subscription::Bound {
            record: Some(record),
            ..
        } => record.record().cycle_served_bytes,
        other => panic!("no record: {other:?}"),
    }
}

#[test]
fn only_bound_users_are_kept_and_the_rest_reported() {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&second.code, &user("pilotuserz"), at(NOW))
        .unwrap();
    let report = store
        .import(
            &snapshot(
                NOW,
                &[
                    ("pilotusera", Some(10)),
                    ("pilotuserb", None),
                    ("pilotuserc", Some(0)),
                ],
            ),
            at(NOW),
        )
        .unwrap();
    assert_eq!(
        report,
        ImportReport {
            stored: vec![user("pilotusera")],
            unbound: 2,
            missing: vec![(second.code, user("pilotuserz"))],
        }
    );
    let kept: i64 = store
        .conn
        .query_row("SELECT count(*) FROM subscription_records", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kept, 1);
    assert_eq!(record_of(&store, &first), Some(10));
    assert!(matches!(
        store.subscription(second.id).unwrap(),
        Subscription::Bound { record: None, binding } if binding.user == user("pilotuserz")
    ));
}

#[test]
fn subscriptions_distinguish_unbound_awaiting_and_recorded() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    assert_eq!(
        store.subscription(account.id).unwrap(),
        Subscription::Unbound
    );
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    assert!(matches!(
        store.subscription(account.id).unwrap(),
        Subscription::Bound { record: None, .. }
    ));
    store
        .import(&snapshot(NOW, &[("pilotusera", None)]), at(NOW))
        .unwrap();
    let Subscription::Bound {
        record: Some(record),
        binding,
    } = store.subscription(account.id).unwrap()
    else {
        panic!("expected a record");
    };
    assert_eq!(binding.user, user("pilotusera"));
    // An unknown figure stays unknown through storage.
    assert_eq!(record.record().cycle_served_bytes, None);
    assert_eq!(record.generated_at(), at(NOW));
}

#[test]
fn newer_snapshots_replace_records_and_older_ones_are_refused_whole() {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&second.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    store
        .import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW))
        .unwrap();
    // The same snapshot again is harmless.
    store
        .import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW))
        .unwrap();
    store
        .import(
            &snapshot(NOW + 20, &[("pilotusera", Some(2))]),
            at(NOW + 20),
        )
        .unwrap();
    assert_eq!(record_of(&store, &first), Some(2));
    store
        .import(
            &snapshot(NOW + 60, &[("pilotuserb", Some(5))]),
            at(NOW + 60),
        )
        .unwrap();
    // Users go in id order: the older snapshot stores pilotusera's record
    // (newer than the one held), then meets pilotuserb's newer record. The
    // write it already made goes with the rest.
    let refused = store.import(
        &snapshot(
            NOW + 30,
            &[("pilotusera", Some(3)), ("pilotuserb", Some(9))],
        ),
        at(NOW + 90),
    );
    assert!(matches!(
        refused,
        Err(ImportError::Older { user: refused_user, stored, incoming })
            if refused_user == user("pilotuserb") && stored == at(NOW + 60) && incoming == at(NOW + 30)
    ));
    assert_eq!(record_of(&store, &first), Some(2));
    assert_eq!(record_of(&store, &second), Some(5));
}

#[test]
fn snapshots_from_the_future_are_refused_beyond_the_skew() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    let ahead = NOW + MAX_CLOCK_SKEW_SECS;
    store
        .import(&snapshot(ahead, &[("pilotusera", Some(1))]), at(NOW))
        .unwrap();
    assert!(matches!(
        store.import(&snapshot(ahead + 1, &[("pilotusera", Some(1))]), at(NOW)),
        Err(ImportError::FromTheFuture { generated_at, now })
            if generated_at == at(ahead + 1) && now == at(NOW)
    ));
}

#[test]
fn unbinding_drops_the_record() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW))
        .unwrap();
    store.unbind(&account.code).unwrap();
    store
        .bind(&account.code, &user("pilotusera"), at(NOW + 5))
        .unwrap();
    assert!(matches!(
        store.subscription(account.id).unwrap(),
        Subscription::Bound { record: None, .. }
    ));
}

#[test]
fn a_record_that_fails_the_contract_is_corrupt() {
    let mut store = store();
    let account = register(&mut store, "a@qq.com", 1);
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW))
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE subscription_records SET document = replace(document, '\"cycle_served_bytes\":1', '\"cycle_served_bytes\":-1')",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.subscription(account.id),
        Err(StoreError::Corrupt {
            what: "subscription record",
            ..
        })
    ));
    assert!(matches!(
        store.import(&snapshot(NOW + 1, &[("pilotusera", Some(1))]), at(NOW + 1)),
        Err(ImportError::Store(StoreError::Corrupt { .. }))
    ));
}

#[test]
fn a_record_filed_under_another_user_is_corrupt() {
    let mut store = store();
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    store
        .bind(&first.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store
        .bind(&second.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    store
        .import(&snapshot(NOW, &[("pilotusera", Some(1))]), at(NOW))
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE subscription_records SET mooring_user = 'pilotuserb'",
            [],
        )
        .unwrap();
    let error = store.subscription(second.id).unwrap_err();
    assert!(
        matches!(&error, StoreError::Corrupt { what: "subscription record", detail }
            if detail.contains("pilotusera") && detail.contains("pilotuserb")),
        "{error}"
    );
}
