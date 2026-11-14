//! What the subscription page says, derived from the stored record, the
//! time and the language.
//!
//! The page states only what the record says. Usage Mooring could not
//! count is "unavailable", never zero; a quota whose usage is unavailable
//! has an unknown level, never "fine"; a record whose billing cycle has
//! ended speaks of that cycle, never of the one now running; an account
//! without a binding has no subscription, and one bound but not yet
//! imported is waiting for data. Failures to load never reach here: they
//! are errors, not empty states.

use portal_core::snapshot::{
    BillingCycle, QuotaAction, QuotaLevel, SuspendReason, UserSnapshot, UserStatus,
};
use portal_core::time::UnixTime;
use portal_store::Subscription;

use crate::format::{beijing, beijing_date, bytes};
use crate::i18n::{Strings, fill};

mod cfm;
mod feedback;
mod release;
mod rental;
mod review;
mod shop;

pub(crate) use cfm::CfmPanel;
pub(crate) use feedback::{MessageView, Reader};
pub(crate) use release::ReleaseView;
pub(crate) use rental::{BookingView, MachineView, PeriodView};
pub(crate) use review::{WaitingView, approve_refusal, shown_from_key, why_it_waits};
pub(crate) use shop::{OrderView, PayAction, PlanView, ShopView};

/// A snapshot older than this is flagged as possibly out of date.
pub(crate) const STALE_AFTER_SECS: u64 = 48 * 3600;

/// How a figure should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Good,
    Warn,
    Bad,
    Neutral,
}

impl Tone {
    /// The CSS class.
    pub(crate) const fn class(self) -> &'static str {
        match self {
            Self::Good => "tone-good",
            Self::Warn => "tone-warn",
            Self::Bad => "tone-bad",
            Self::Neutral => "tone-neutral",
        }
    }
}

/// A labelled value with an optional note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fact {
    pub(crate) value: String,
    pub(crate) note: Option<String>,
    pub(crate) tone: Tone,
}

/// Where usage stands against the quota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Gauge {
    pub(crate) label: &'static str,
    pub(crate) tone: Tone,
    /// "40% used", when the usage is known.
    pub(crate) used: Option<String>,
    /// The bar's filled share, 0..=100, when the usage is known.
    pub(crate) bar: Option<u8>,
}

/// The quota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuotaFact {
    pub(crate) value: String,
    pub(crate) gauge: Option<Gauge>,
    pub(crate) note: &'static str,
}

/// Everything the page shows about a bound user with a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Details {
    pub(crate) status: Fact,
    /// The current cycle, or an ended one.
    pub(crate) cycle_label: &'static str,
    pub(crate) cycle: String,
    /// When the cycle resets, or when it ended.
    pub(crate) resets: String,
    pub(crate) usage_label: &'static str,
    pub(crate) usage: Fact,
    /// How far the usage is counted, or that Mooring had no usage report.
    pub(crate) counted: String,
    pub(crate) quota: QuotaFact,
    pub(crate) expiry: Fact,
    pub(crate) as_of: String,
    /// Warnings above the figures, most important first.
    pub(crate) alerts: Vec<&'static str>,
    pub(crate) lag_note: Option<&'static str>,
}

/// Whether the record's billing cycle is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CycleState {
    Running,
    /// Over: the record's figures say nothing about the cycle now running,
    /// whose figures have not been imported yet.
    Ended,
}

/// The subscription page's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubscriptionView {
    /// Not bound to a Mooring user.
    Unbound,
    /// Bound, but no record imported yet.
    Awaiting { message: String },
    /// Bound, with a record.
    Ready(Box<Details>),
}

impl SubscriptionView {
    /// The view of `subscription` at `now` in the language of `t`.
    pub(crate) fn of(subscription: &Subscription, now: UnixTime, t: &Strings) -> Self {
        match subscription {
            Subscription::Unbound => Self::Unbound,
            Subscription::Bound {
                binding,
                record: None,
            } => Self::Awaiting {
                message: fill(t.subscription.awaiting_body, beijing_date(binding.bound_at)),
            },
            Subscription::Bound {
                record: Some(record),
                ..
            } => Self::Ready(Box::new(details(record, now, t))),
        }
    }
}

fn details(snapshot: &UserSnapshot, now: UnixTime, t: &Strings) -> Details {
    let s = &t.subscription;
    let record = snapshot.record();
    let cycle = snapshot.cycle();
    let state = if now >= cycle.ends_at() {
        CycleState::Ended
    } else {
        CycleState::Running
    };
    let mut alerts = Vec::new();
    if state == CycleState::Ended {
        alerts.push(s.cycle_ended);
    }
    if now.secs_since(snapshot.generated_at()) > STALE_AFTER_SECS {
        alerts.push(s.stale);
    }
    let (cycle_label, resets, usage_label) = match state {
        CycleState::Running => (s.cycle, s.resets, s.usage),
        CycleState::Ended => (s.cycle_ended_label, s.ended, s.usage_ended),
    };
    Details {
        status: status(record.status, cycle, t),
        cycle_label,
        cycle: format!(
            "{} – {}",
            beijing(cycle.starts_at()),
            beijing(cycle.ends_at())
        ),
        resets: fill(resets, beijing(cycle.ends_at())),
        usage_label,
        usage: usage(snapshot, t),
        counted: snapshot.counted_through().map_or_else(
            || s.no_report.to_owned(),
            |through| fill(s.counted_through, beijing(through)),
        ),
        quota: quota(record.quota, record.cycle_served_bytes, state, t),
        expiry: expiry(record.expires_at, now, t),
        as_of: fill(s.as_of, beijing(snapshot.generated_at())),
        alerts,
        lag_note: record
            .reality_downlink_counted_at_close
            .then_some(s.lag_note),
    }
}

fn status(status: UserStatus, cycle: BillingCycle, t: &Strings) -> Fact {
    let s = &t.subscription;
    match status {
        UserStatus::Active => Fact {
            value: s.active.to_owned(),
            note: None,
            tone: Tone::Good,
        },
        UserStatus::Suspended { reason, since } => Fact {
            value: s.suspended.to_owned(),
            note: Some(format!(
                "{} {}",
                match reason {
                    SuspendReason::Operator => s.reason_operator,
                    // As Mooring decides whether a suspension for quota
                    // holds: by whether it was made in the cycle.
                    SuspendReason::QuotaExceeded if since >= cycle.starts_at() => s.reason_quota,
                    SuspendReason::QuotaExceeded => s.reason_quota_earlier,
                    SuspendReason::Expired => s.reason_expired,
                    SuspendReason::AbuseReview => s.reason_abuse,
                },
                fill(s.since, beijing(since))
            )),
            tone: Tone::Warn,
        },
        UserStatus::Revoked => Fact {
            value: s.revoked.to_owned(),
            note: Some(s.revoked_body.to_owned()),
            tone: Tone::Bad,
        },
    }
}

fn usage(snapshot: &UserSnapshot, t: &Strings) -> Fact {
    let s = &t.subscription;
    match snapshot.record().cycle_served_bytes {
        Some(served) => Fact {
            value: bytes(served),
            note: None,
            tone: Tone::Neutral,
        },
        None => Fact {
            value: s.usage_unknown.to_owned(),
            // Without a usage report there was nothing to count from; the
            // counting line says so.
            note: snapshot
                .counted_through()
                .map(|_| s.usage_unknown_hint.to_owned()),
            tone: Tone::Warn,
        },
    }
}

fn quota(
    quota: Option<portal_core::snapshot::Quota>,
    used: Option<u64>,
    state: CycleState,
    t: &Strings,
) -> QuotaFact {
    let s = &t.subscription;
    let Some(quota) = quota else {
        return QuotaFact {
            value: s.quota_none.to_owned(),
            gauge: None,
            note: s.quota_none_hint,
        };
    };
    let (label, tone) = match quota.level(used) {
        QuotaLevel::Within => (s.level_within, Tone::Good),
        QuotaLevel::Near => (s.level_near, Tone::Warn),
        QuotaLevel::Over => (s.level_over, Tone::Bad),
        QuotaLevel::Unknown => (s.level_unknown, Tone::Warn),
    };
    let percent = used.map(|used| quota.used_percent(used));
    QuotaFact {
        value: bytes(quota.cycle_served_bytes.get()),
        // A level is for the cycle now running; an ended cycle has none.
        gauge: (state == CycleState::Running).then(|| Gauge {
            label,
            tone,
            used: percent.map(|percent| fill(s.quota_used, percent)),
            bar: percent.map(bar_share),
        }),
        note: match quota.action {
            QuotaAction::Suspend => s.action_suspend,
            QuotaAction::Warn => s.action_warn,
        },
    }
}

/// The bar's filled share: the percentage, capped at a full bar.
const fn bar_share(percent: u64) -> u8 {
    if percent >= 100 {
        100
    } else {
        // Below 100, so the narrowing is exact.
        percent as u8
    }
}

fn expiry(expires_at: Option<UnixTime>, now: UnixTime, t: &Strings) -> Fact {
    let s = &t.subscription;
    match expires_at {
        None => Fact {
            value: s.expiry_never.to_owned(),
            note: None,
            tone: Tone::Neutral,
        },
        Some(at) if at <= now => Fact {
            value: beijing(at),
            note: Some(s.expired.to_owned()),
            tone: Tone::Bad,
        },
        Some(at) => {
            // Whole days left, rounded up, as Mooring counts them.
            let days = at.secs_since(now).div_ceil(portal_core::time::SECS_PER_DAY);
            let phrase = if days == 1 {
                s.days_left_one
            } else {
                s.days_left_many
            };
            Fact {
                value: beijing(at),
                note: Some(fill(phrase, days)),
                tone: if days <= 3 { Tone::Warn } else { Tone::Neutral },
            }
        }
    }
}

#[cfg(test)]
mod tests;
