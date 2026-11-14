//! What the pages say about plans and orders.
//!
//! A page states where an order stands as the store reads it, and no
//! more: "handed over" means Mooring has fetched the order, not that the
//! service is extended, which only the next snapshot can show.

use portal_core::payment::stripe::Renewal;
use portal_core::payment::{Catalogue, OrderTerms, Plan, PlanKind};
use portal_core::snapshot::MooringUserId;
use portal_store::{AutoRenewal, IntroLink, IntroState, IntroStop, Order, Progress};

use super::Tone;
use crate::format::{beijing, bytes, money};
use crate::i18n::{Lang, Strings, fill};

/// A plan on offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanView {
    /// The plan's id, as the order form names it.
    pub(crate) id: String,
    pub(crate) name: &'static str,
    pub(crate) terms: String,
    pub(crate) price: String,
    /// It renews by itself, and the account already has such a plan.
    pub(crate) blocked: bool,
}

/// What `terms` buy, in the language of `t`: the kind's name, and its
/// days and traffic. A pack's days are left out: Mooring counts a pack
/// only in the billing cycle it grants it in, and applies no days (the
/// operator's decision of 2026-11-12).
fn describe(terms: &OrderTerms, t: &Strings) -> (&'static str, String) {
    let traffic = bytes(terms.traffic_bytes);
    let period = |phrase: &str| fill(&fill(phrase, terms.days), &traffic);
    match terms.kind {
        PlanKind::Month => (t.pay.kind_month, period(t.pay.terms_month)),
        PlanKind::MonthAuto => (t.pay.kind_month_auto, period(t.pay.terms_month_auto)),
        PlanKind::Traffic => (t.pay.kind_traffic, fill(t.pay.terms_traffic, &traffic)),
    }
}

/// What the first order costs, and what it will cost to renew, if different.
fn price(terms: &OrderTerms, t: &Strings) -> String {
    let first = money(terms.amount, terms.currency);
    let Some(renewal) = terms.renewal_amount else {
        return first;
    };
    fill(
        &fill(
            &fill(t.pay.intro_price, first),
            money(renewal, terms.currency),
        ),
        terms.days,
    )
}

impl PlanView {
    /// `plan` as a page lists it; `renewing` says the account has a plan
    /// that renews by itself.
    pub(crate) fn of(plan: &Plan, renewing: bool, t: &Strings) -> Self {
        let terms = OrderTerms::of(plan);
        let (name, described) = describe(&terms, t);
        Self {
            id: plan.id.to_string(),
            name,
            terms: if plan.introductory {
                t.pay.intro_contract.to_owned()
            } else {
                described
            },
            price: price(&terms, t),
            blocked: renewing && plan.kind.renews(),
        }
    }
}

/// An order as its owner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderView {
    pub(crate) id: String,
    pub(crate) name: &'static str,
    pub(crate) terms: String,
    pub(crate) price: String,
    pub(crate) placed: String,
    pub(crate) paid: Option<String>,
    pub(crate) state: &'static str,
    pub(crate) tone: Tone,
    /// What its state means for its owner, and what has happened to the
    /// money since.
    pub(crate) notes: Vec<String>,
    /// A subscription's renewal made it; nobody placed it.
    pub(crate) renewal: bool,
}

impl OrderView {
    /// `order` in the language of `t`.
    pub(crate) fn of(order: &Order, t: &Strings) -> Self {
        let p = &t.pay;
        let (name, terms) = describe(&order.terms, t);
        let (state, tone, note) = match order.progress {
            Progress::Pending => (p.state_pending, Tone::Neutral, None),
            Progress::Checking => (p.state_checking, Tone::Warn, Some(p.note_checking)),
            Progress::Granted { pulled: false } => {
                (p.state_granted, Tone::Good, Some(p.note_granted))
            }
            Progress::Granted { pulled: true } => (p.state_pulled, Tone::Good, Some(p.note_pulled)),
            // Why it waits is the operator's to explain: the reasons are
            // about the payment's inside, and one of them is a ban.
            Progress::Review(_) => (p.state_review, Tone::Warn, Some(p.note_review)),
            Progress::Returned => (p.state_returned, Tone::Neutral, Some(p.note_returned)),
        };
        let mut notes: Vec<String> = note.map(str::to_owned).into_iter().collect();
        if order.refunded_in_full {
            // A granted order gives no more from the refund on: Mooring
            // takes back what is not yet used (the operator's decision of
            // 2026-11-12). One never granted gave nothing.
            let gave = matches!(order.progress, Progress::Granted { .. });
            let note = if gave {
                p.note_taken_back
            } else {
                p.note_refunded_full
            };
            notes.push(note.to_owned());
        } else if order.refunded > 0 {
            notes.push(fill(
                p.note_refunded_part,
                money(order.refunded, order.terms.currency),
            ));
        }
        if order.disputed {
            notes.push(p.note_disputed.to_owned());
        }
        Self {
            id: order.id.to_string(),
            name,
            terms,
            price: price(&order.terms, t),
            placed: beijing(order.created_at),
            paid: order.paid_at.map(beijing),
            state,
            tone,
            notes,
            renewal: order.renewal_of.is_some(),
        }
    }
}

/// What an order's page offers to do about its payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PayAction {
    /// Nothing: it is paid.
    None,
    /// Pay it, at this address on Stripe's site.
    Pay(String),
    /// It can no longer be paid: the catalogue's terms for its plan, or
    /// the account's binding, are not what they were when it was placed.
    Stale,
}

impl PayAction {
    /// What to offer for `order`, of an account now bound to `bound_to`.
    #[cfg(test)]
    pub(crate) fn of(
        order: &Order,
        catalogue: &Catalogue,
        bound_to: Option<&MooringUserId>,
        lang: Lang,
    ) -> Self {
        Self::with_intro(order, catalogue, bound_to, lang, None)
    }

    pub(crate) fn with_intro(
        order: &Order,
        catalogue: &Catalogue,
        bound_to: Option<&MooringUserId>,
        lang: Lang,
        intro: Option<&IntroLink>,
    ) -> Self {
        if order.progress != Progress::Pending {
            return Self::None;
        }
        if bound_to != Some(&order.user) {
            return Self::Stale;
        }
        let standing = catalogue.plan(order.terms.plan.as_str()).and_then(|plan| {
            let mut terms = OrderTerms::of(plan);
            let url = if plan.introductory {
                let intro = intro?;
                terms.payment_link_id = intro.id.as_str().to_owned();
                &intro.url
            } else {
                &plan.payment_link
            };
            (terms == order.terms).then_some(url)
        });
        match standing {
            // The order's id rides to Stripe and back as the payment's
            // reference. Neither it nor the language needs escaping.
            Some(url) => Self::Pay(format!(
                "{}?client_reference_id={}&locale={}",
                url,
                order.id,
                lang.code()
            )),
            None => Self::Stale,
        }
    }
}

/// The subscription page's shop: what a bound account can buy, and what
/// it has bought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShopView {
    pub(crate) plans: Vec<PlanView>,
    /// What to say of the account's plan that renews by itself, if it has
    /// one.
    pub(crate) renewal: Option<&'static str>,
    /// Where such a plan is managed, at Stripe.
    pub(crate) manage_url: Option<String>,
    pub(crate) intro_message: Option<String>,
    pub(crate) can_stop_intro: bool,
    pub(crate) orders: Vec<OrderView>,
    /// The catalogue's payment pages take no real payments.
    pub(crate) test_mode: bool,
}

impl ShopView {
    /// The shop of an account with `orders` and `renewals`.
    #[cfg(test)]
    pub(crate) fn of(
        catalogue: &Catalogue,
        orders: &[Order],
        renewals: &[AutoRenewal],
        t: &Strings,
    ) -> Self {
        Self::with_intro(catalogue, orders, renewals, t, &IntroState::None, None)
    }

    pub(crate) fn with_intro(
        catalogue: &Catalogue,
        orders: &[Order],
        renewals: &[AutoRenewal],
        t: &Strings,
        intro: &IntroState,
        now: Option<portal_core::time::UnixTime>,
    ) -> Self {
        let renewing = !renewals.is_empty();
        let (intro_message, can_stop_intro, lock_cancel) = match intro {
            IntroState::Claimed { end, stop } => {
                let before_end = now.is_some_and(|now| now < *end);
                let msg = match stop {
                    IntroStop::Renewing => t.pay.intro_renews,
                    IntroStop::Pending => t.pay.intro_pending,
                    IntroStop::Confirmed => t.pay.intro_confirmed,
                    IntroStop::Ended => t.pay.intro_ended,
                    IntroStop::Unavailable => t.pay.intro_unavailable,
                };
                (
                    Some(fill(msg, beijing(*end))),
                    before_end && *stop == IntroStop::Renewing,
                    before_end,
                )
            }
            _ => (None, false, false),
        };
        // Stripe will charge again unless every one of them is ending.
        let renewal = renewing.then(|| {
            if renewals
                .iter()
                .all(|renewal| renewal.state == Renewal::Ending)
            {
                t.pay.renewing_ending
            } else {
                t.pay.renewing_active
            }
        });
        Self {
            plans: catalogue
                .plans()
                .iter()
                .filter(|plan| !plan.introductory || matches!(intro, IntroState::Available(_)))
                .map(|plan| PlanView::of(plan, renewing || lock_cancel, t))
                .collect(),
            renewal,
            manage_url: (renewing && !lock_cancel)
                .then(|| catalogue.manage_url().map(str::to_owned))
                .flatten(),
            intro_message,
            can_stop_intro,
            orders: orders.iter().map(|order| OrderView::of(order, t)).collect(),
            test_mode: !catalogue.livemode(),
        }
    }
}

#[cfg(test)]
mod tests;
