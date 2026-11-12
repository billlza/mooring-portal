use std::num::NonZeroU64;

use portal_core::snapshot::{
    BillingCycle, MooringUserId, Quota, QuotaAction, Snapshot, SuspendReason, UserRecord,
    UserStatus, WarnPercent,
};
use portal_core::time::CalendarDate;
use portal_store::Binding;

use super::*;
use crate::i18n::Lang;

/// 2026-11-05T12:00:00Z, 20:00 in Beijing.
const GENERATED: u64 = 1_793_880_000;
const HOUR: u64 = 3600;
const DAY: u64 = 86_400;

fn at(secs: u64) -> UnixTime {
    UnixTime::from_secs(secs).unwrap()
}

fn quota(bytes: u64, action: QuotaAction) -> Quota {
    Quota {
        cycle_served_bytes: NonZeroU64::new(bytes).unwrap(),
        warn_percent: WarnPercent::new(80).unwrap(),
        action,
    }
}

fn active(used: Option<u64>) -> UserRecord {
    UserRecord {
        status: UserStatus::Active,
        cycle_served_bytes: used,
        reality_downlink_counted_at_close: true,
        quota: Some(quota(40_000_000_000, QuotaAction::Suspend)),
        expires_at: None,
    }
}

/// The subscription of a user bound at the start of November with
/// `record`, generated at [`GENERATED`] with usage counted to five
/// minutes earlier (or without a usage report).
fn bound(record: UserRecord, report: bool) -> Subscription {
    bound_with(record, GENERATED, report)
}

/// [`bound`], generated at `generated`.
fn bound_with(record: UserRecord, generated: u64, report: bool) -> Subscription {
    let cycle = BillingCycle::new(
        CalendarDate::parse("2026-11-01").unwrap(),
        CalendarDate::parse("2026-12-01").unwrap(),
    )
    .unwrap();
    let user = MooringUserId::parse("pilotusera").unwrap();
    let snapshot = Snapshot::new(
        at(generated),
        cycle,
        report.then(|| at(generated - 300)),
        [(user.clone(), record)],
    )
    .unwrap();
    Subscription::Bound {
        binding: Binding {
            user,
            bound_at: at(GENERATED - 4 * DAY),
        },
        record: snapshot.per_user().next(),
    }
}

fn ready(subscription: &Subscription, now: u64, lang: Lang) -> Details {
    match SubscriptionView::of(subscription, at(now), lang.strings()) {
        SubscriptionView::Ready(details) => *details,
        other => panic!("not ready: {other:?}"),
    }
}

#[test]
fn unbound_and_awaiting_accounts_say_so() {
    let t = Lang::Zh.strings();
    assert_eq!(
        SubscriptionView::of(&Subscription::Unbound, at(GENERATED), t),
        SubscriptionView::Unbound
    );
    let Subscription::Bound { binding, .. } = bound(active(None), true) else {
        unreachable!()
    };
    let awaiting = Subscription::Bound {
        binding,
        record: None,
    };
    assert_eq!(
        SubscriptionView::of(&awaiting, at(GENERATED), t),
        SubscriptionView::Awaiting {
            message: "你的账号已于 2026-11-01 绑定，运营者还没有导入你的订阅数据。".to_owned()
        }
    );
}

#[test]
fn a_known_figure_is_shown_with_its_cycle_and_quota() {
    let details = ready(
        &bound(active(Some(12_345_678_901)), true),
        GENERATED,
        Lang::Zh,
    );
    assert_eq!(details.status.value, "使用中");
    assert_eq!(details.status.tone, Tone::Good);
    assert_eq!(details.cycle, "2026-11-01 08:00 – 2026-12-01 08:00");
    assert_eq!(details.resets, "下次重置：2026-12-01 08:00");
    assert_eq!(details.usage.value, "12.35 GB");
    assert_eq!(details.usage.note, None);
    assert_eq!(details.counted, "统计截至 2026-11-05 19:55");
    assert_eq!(details.quota.value, "40 GB");
    assert_eq!(
        details.quota.gauge,
        Some(Gauge {
            label: "正常",
            tone: Tone::Good,
            used: Some("已用 30%".to_owned()),
            bar: Some(30),
        })
    );
    assert_eq!(
        details.quota.note,
        "用完后暂停到下一个计费周期，买了够补上超出部分的流量包会提前恢复。"
    );
    assert_eq!(details.as_of, "数据截至 2026-11-05 20:00");
    assert!(details.alerts.is_empty());
    assert!(details.lag_note.is_some());
}

/// 2026-12-01T00:00:00Z, when November's cycle ends.
const NOVEMBER_ENDS: u64 = GENERATED + 25 * DAY + 12 * HOUR;

#[test]
fn a_cycle_that_has_ended_is_named_so_and_its_quota_level_is_dropped() {
    // Generated an hour before the reset, so never stale by age alone.
    let record = bound_with(active(Some(12_345_678_901)), NOVEMBER_ENDS - HOUR, true);
    let running = ready(&record, NOVEMBER_ENDS - 1, Lang::Zh);
    assert_eq!(running.cycle_label, "本计费周期");
    assert_eq!(running.usage_label, "本期用量");
    assert_eq!(running.resets, "下次重置：2026-12-01 08:00");
    assert!(running.quota.gauge.is_some());
    assert!(running.alerts.is_empty());
    let ended = ready(&record, NOVEMBER_ENDS, Lang::Zh);
    assert_eq!(ended.cycle_label, "已结束的计费周期");
    assert_eq!(ended.usage_label, "该周期用量");
    assert_eq!(ended.resets, "已于 2026-12-01 08:00 结束");
    assert_eq!(ended.usage.value, "12.35 GB", "that cycle's figure stays");
    assert_eq!(ended.quota.value, "40 GB");
    assert_eq!(ended.quota.gauge, None, "no level for a cycle now over");
    let t = Lang::Zh.strings();
    assert_eq!(ended.alerts, vec![t.subscription.cycle_ended]);
    let later = ready(&record, NOVEMBER_ENDS + 48 * HOUR, Lang::Zh);
    assert_eq!(
        later.alerts,
        vec![t.subscription.cycle_ended, t.subscription.stale]
    );
    let english = ready(&record, NOVEMBER_ENDS, Lang::En);
    assert_eq!(english.cycle_label, "Ended billing cycle");
    assert_eq!(english.usage_label, "Usage in that cycle");
    assert_eq!(english.resets, "Ended 2026-12-01 08:00");
}

#[test]
fn unknown_usage_is_unavailable_never_zero() {
    for report in [true, false] {
        let details = ready(&bound(active(None), report), GENERATED, Lang::Zh);
        assert_eq!(details.usage.value, "无法获取");
        assert_eq!(details.usage.tone, Tone::Warn);
        let gauge = details.quota.gauge.unwrap();
        assert_eq!(gauge.label, "用量无法获取，无法判断");
        assert_eq!(gauge.used, None);
        assert_eq!(gauge.bar, None);
        if report {
            assert!(details.usage.note.unwrap().contains("没能统计到"));
            assert_eq!(details.counted, "统计截至 2026-11-05 19:55");
        } else {
            assert_eq!(details.usage.note, None);
            assert_eq!(details.counted, "运营者导出这份数据时没有用量报告。");
        }
        assert!(!details.usage.value.contains('0'));
    }
    let english = ready(&bound(active(None), false), GENERATED, Lang::En);
    assert_eq!(english.usage.value, "Unavailable");
}

#[test]
fn a_known_zero_is_zero() {
    let details = ready(&bound(active(Some(0)), true), GENERATED, Lang::Zh);
    assert_eq!(details.usage.value, "0 B");
    assert_eq!(
        details.quota.gauge.unwrap().used.as_deref(),
        Some("已用 0%")
    );
}

#[test]
fn quota_levels_follow_the_record() {
    let near = ready(
        &bound(active(Some(35_000_000_000)), true),
        GENERATED,
        Lang::Zh,
    );
    let gauge = near.quota.gauge.unwrap();
    assert_eq!(
        (gauge.label, gauge.tone, gauge.bar),
        ("接近配额", Tone::Warn, Some(87))
    );
    let over = ready(
        &bound(active(Some(41_600_000_000)), true),
        GENERATED,
        Lang::Zh,
    );
    let gauge = over.quota.gauge.unwrap();
    assert_eq!(gauge.label, "已达配额");
    assert_eq!(gauge.tone, Tone::Bad);
    assert_eq!(gauge.used.as_deref(), Some("已用 104%"));
    assert_eq!(gauge.bar, Some(100));
    let warn_only = UserRecord {
        quota: Some(quota(40_000_000_000, QuotaAction::Warn)),
        ..active(Some(1))
    };
    let details = ready(&bound(warn_only, true), GENERATED, Lang::Zh);
    assert_eq!(details.quota.note, "用完后只提醒运营者，不暂停服务。");
    let none = UserRecord {
        quota: None,
        ..active(Some(1))
    };
    let details = ready(&bound(none, true), GENERATED, Lang::Zh);
    assert_eq!(details.quota.value, "未设置个人配额");
    assert_eq!(details.quota.gauge, None);
    assert_eq!(details.quota.note, "所有用户仍共用一个月度流量上限。");
}

#[test]
fn suspensions_name_their_reason_and_revocation_is_final() {
    let since = at(GENERATED - 2 * HOUR);
    for (reason, phrase) in [
        (SuspendReason::Operator, "运营者暂停了你的服务。"),
        // Mooring's nightly job, between 3 and 6 a.m. Beijing time, resumes
        // a user a pack now covers, and one who has renewed.
        (
            SuspendReason::QuotaExceeded,
            "该计费周期的流量配额已经用完，服务暂停到周期结束。\
             买一个够补上超出部分的流量包，会在下一次北京时间凌晨 3 点到 6 点的更新时恢复。",
        ),
        (
            SuspendReason::Expired,
            "访问已经到期，服务暂停到你续费为止。\
             续费后，会在下一次北京时间凌晨 3 点到 6 点的更新时恢复。",
        ),
        (SuspendReason::AbuseReview, "因投诉正在审核"),
    ] {
        let record = UserRecord {
            status: UserStatus::Suspended { reason, since },
            ..active(Some(1))
        };
        let details = ready(&bound(record, true), GENERATED, Lang::Zh);
        assert_eq!(details.status.value, "已暂停");
        assert_eq!(details.status.tone, Tone::Warn);
        let note = details.status.note.unwrap();
        assert!(
            note.contains(phrase) && note.ends_with("自 2026-11-05 18:00 起"),
            "{note}"
        );
    }
    // A suspension for an earlier cycle's quota has lapsed by Mooring's
    // rules, though the record still shows it.
    let earlier = UserRecord {
        status: UserStatus::Suspended {
            reason: SuspendReason::QuotaExceeded,
            since: at(GENERATED - 6 * DAY),
        },
        ..active(Some(1))
    };
    let note = ready(&bound(earlier, true), GENERATED, Lang::Zh)
        .status
        .note
        .unwrap();
    assert!(
        note.starts_with("因更早一个计费周期的配额用完而暂停") && !note.contains("该计费周期"),
        "{note}"
    );
    let revoked = UserRecord {
        status: UserStatus::Revoked,
        ..active(Some(1))
    };
    let details = ready(&bound(revoked, true), GENERATED, Lang::En);
    assert_eq!(details.status.value, "Ended");
    assert_eq!(details.status.tone, Tone::Bad);
}

#[test]
fn an_english_pause_says_how_service_comes_back() {
    let since = at(GENERATED - 2 * HOUR);
    for (reason, phrase) in [
        (
            SuspendReason::QuotaExceeded,
            "The billing cycle's quota is used up; service is paused until the cycle ends. \
             A traffic pack that covers what you used over the quota resumes it at the next \
             update, between 3 and 6 a.m. Beijing time.",
        ),
        (
            SuspendReason::Expired,
            "Your access has expired; service stays paused until you renew. Once you have, it \
             resumes at the next update, between 3 and 6 a.m. Beijing time.",
        ),
    ] {
        let record = UserRecord {
            status: UserStatus::Suspended { reason, since },
            ..active(Some(1))
        };
        let note = ready(&bound(record, true), GENERATED, Lang::En)
            .status
            .note
            .unwrap();
        assert!(note.starts_with(phrase), "{note}");
    }
    let details = ready(&bound(active(Some(1)), true), GENERATED, Lang::En);
    assert_eq!(
        details.quota.note,
        "When used up, service pauses until the next billing cycle, or until a traffic pack \
         covers what was used over it."
    );
}

#[test]
fn expiry_counts_whole_days_up_and_warns_near_the_end() {
    let expiring = |secs| UserRecord {
        expires_at: Some(at(secs)),
        ..active(Some(1))
    };
    let never = ready(&bound(active(Some(1)), true), GENERATED, Lang::Zh);
    assert_eq!(never.expiry.value, "不到期");
    let far = ready(
        &bound(expiring(GENERATED + 10 * DAY), true),
        GENERATED,
        Lang::Zh,
    );
    assert_eq!(far.expiry.note.as_deref(), Some("还剩 10 天"));
    assert_eq!(far.expiry.tone, Tone::Neutral);
    let soon = ready(
        &bound(expiring(GENERATED + 2 * DAY + 1), true),
        GENERATED,
        Lang::Zh,
    );
    assert_eq!(soon.expiry.note.as_deref(), Some("还剩 3 天"));
    assert_eq!(soon.expiry.tone, Tone::Warn);
    let last = ready(
        &bound(expiring(GENERATED + HOUR), true),
        GENERATED,
        Lang::En,
    );
    assert_eq!(last.expiry.note.as_deref(), Some("1 day left"));
    let two = ready(
        &bound(expiring(GENERATED + 2 * DAY), true),
        GENERATED,
        Lang::En,
    );
    assert_eq!(two.expiry.note.as_deref(), Some("2 days left"));
    let gone = ready(&bound(expiring(GENERATED), true), GENERATED, Lang::Zh);
    assert_eq!(gone.expiry.value, "2026-11-05 20:00");
    assert_eq!(gone.expiry.note.as_deref(), Some("已到期"));
    assert_eq!(gone.expiry.tone, Tone::Bad);
}

#[test]
fn old_snapshots_are_flagged_and_lag_notes_follow_the_record() {
    let fresh = ready(
        &bound(active(Some(1)), true),
        GENERATED + 48 * HOUR,
        Lang::Zh,
    );
    assert!(fresh.alerts.is_empty());
    let old = ready(
        &bound(active(Some(1)), true),
        GENERATED + 48 * HOUR + 1,
        Lang::Zh,
    );
    assert_eq!(old.alerts, vec![Lang::Zh.strings().subscription.stale]);
    let settled = UserRecord {
        reality_downlink_counted_at_close: false,
        ..active(Some(1))
    };
    assert_eq!(
        ready(&bound(settled, true), GENERATED, Lang::Zh).lag_note,
        None
    );
}

#[test]
fn tones_have_classes_and_bars_cap_at_full() {
    assert_eq!(Tone::Good.class(), "tone-good");
    assert_eq!(Tone::Warn.class(), "tone-warn");
    assert_eq!(Tone::Bad.class(), "tone-bad");
    assert_eq!(Tone::Neutral.class(), "tone-neutral");
    assert_eq!(bar_share(0), 0);
    assert_eq!(bar_share(99), 99);
    assert_eq!(bar_share(100), 100);
    assert_eq!(bar_share(u64::MAX), 100);
}
