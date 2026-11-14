//! What the operator's page of payments says of an order that waits: why
//! it waits, what the operator may do about it, and whether the page
//! offers to approve it. It says what `orders` on the host says, in the
//! page's language.
//!
//! Approving waives the one thing shown and nothing else
//! ([`portal_store::OperatorActions::approve_order`]): the approving form
//! names what the page showed the order waits for, and an order that waits
//! for something else by the time the form arrives is not approved. The
//! page says, before the operator approves an order in review for its
//! checkout, what its card will decide next.

use portal_core::payment::{CardVerdict, ReviewReason};
use portal_store::{ApproveError, OperatorOrder, Progress};

use super::OrderView;
use crate::i18n::{Strings, fill};

/// What an approving form names for an order that was paid and waits for
/// word of its card; one in review is named by its reason.
const CHECKING: &str = "checking";

/// An order that waits, as the operator's page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WaitingView {
    pub(crate) order: OrderView,
    pub(crate) account: String,
    /// The ids the Stripe Dashboard finds its payment by, each with what
    /// it is, those there are.
    pub(crate) at_stripe: Vec<(&'static str, String)>,
    /// Why it waits.
    pub(crate) why: &'static str,
    /// What the operator may do about it.
    pub(crate) todo: Vec<&'static str>,
    /// Whether the page offers to approve it.
    pub(crate) approvable: bool,
    /// What the approving form says the page showed it waits for.
    pub(crate) shown: &'static str,
}

/// What an approving form names for an order that waits for `shown`: why
/// it is in review, or `None` for word of its card.
pub(crate) fn shown_key(shown: Option<ReviewReason>) -> &'static str {
    shown.map_or(CHECKING, ReviewReason::as_str)
}

/// What an approving form's `key` says the page showed the order waits
/// for, as [`shown_key`] names it; `None` for a key no page writes.
pub(crate) fn shown_from_key(key: &str) -> Option<Option<ReviewReason>> {
    if key == CHECKING {
        Some(None)
    } else {
        ReviewReason::parse(key).map(Some)
    }
}

/// Why an order in review for `reason` waits, in the language of `t`.
pub(crate) const fn why_it_waits(reason: ReviewReason, t: &Strings) -> &'static str {
    let r = &t.review;
    match reason {
        ReviewReason::Unpaid => r.why_unpaid,
        ReviewReason::Link => r.why_link,
        ReviewReason::Mode => r.why_mode,
        ReviewReason::Amount => r.why_amount,
        ReviewReason::Terms => r.why_terms,
        ReviewReason::CardBanned => r.why_card_banned,
        ReviewReason::CardUnchecked => r.why_card_unchecked,
        ReviewReason::Binding => r.why_binding,
    }
}

/// What approving an order in review for its checkout leads to: its card
/// is judged next.
const fn after_approval(card: Option<CardVerdict>, t: &Strings) -> &'static str {
    let r = &t.review;
    match card {
        Some(CardVerdict::Clear) => r.next_clear,
        Some(CardVerdict::Banned) => r.next_banned,
        Some(CardVerdict::Unchecked) => r.next_unchecked,
        None => r.next_unknown,
    }
}

impl WaitingView {
    /// `listed` in the language of `t`.
    pub(crate) fn of(listed: &OperatorOrder, t: &Strings) -> Self {
        let r = &t.review;
        let order = &listed.order;
        let waits = match order.progress {
            Progress::Review(reason) => Some(reason),
            _ => None,
        };
        // Whatever else it waited for: a disputed payment is neither
        // approved nor, at Stripe, refunded.
        let (why, todo, approvable) = if order.disputed {
            (r.disputed, vec![], false)
        } else {
            match waits {
                Some(reason @ ReviewReason::CardBanned) => {
                    (why_it_waits(reason, t), vec![r.todo_card_banned], false)
                }
                Some(reason @ ReviewReason::CardUnchecked) => {
                    (why_it_waits(reason, t), vec![r.todo_card_unchecked], true)
                }
                Some(reason @ ReviewReason::Binding) => {
                    (why_it_waits(reason, t), vec![r.todo_binding], true)
                }
                Some(
                    reason @ (ReviewReason::Unpaid
                    | ReviewReason::Link
                    | ReviewReason::Mode
                    | ReviewReason::Amount
                    | ReviewReason::Terms),
                ) => (
                    why_it_waits(reason, t),
                    vec![after_approval(listed.card, t), r.todo_checkout],
                    true,
                ),
                // Paid, and waiting for word of its card.
                None => {
                    let why = if listed.payment_intent.is_none() {
                        r.waits_invoice
                    } else {
                        r.waits_charge
                    };
                    (why, vec![r.todo_checking], true)
                }
            }
        };
        let at_stripe = [
            (r.id_checkout, listed.session.as_ref()),
            (r.id_payment, listed.payment_intent.as_ref()),
            (r.id_invoice, listed.invoice.as_ref()),
            (r.id_subscription, listed.subscription.as_ref()),
        ]
        .into_iter()
        .filter_map(|(what, id)| id.map(|id| (what, id.to_string())))
        .collect();
        Self {
            order: OrderView::of(order, t),
            account: listed.account.to_string(),
            at_stripe,
            why,
            todo,
            approvable,
            shown: shown_key(waits),
        }
    }
}

/// Why the order an approval named was not approved, in the language of
/// `t`; `None` for a failure of the database, which is no answer to give
/// the operator but an error.
pub(crate) fn approve_refusal(error: &ApproveError, t: &Strings) -> Option<String> {
    let r = &t.review;
    let (phrase, id) = match error {
        ApproveError::Unknown(id) => (r.unknown_order, id),
        ApproveError::Unpaid(id) => (r.refused_unpaid, id),
        ApproveError::Granted(id) => (r.refused_granted, id),
        ApproveError::Returned(id) => (r.refused_returned, id),
        ApproveError::CardBanned(id) => (r.refused_card_banned, id),
        ApproveError::Unbound(id) => (r.refused_unbound, id),
        ApproveError::Changed(id) => (r.refused_changed, id),
        ApproveError::Store(_) => return None,
    };
    Some(fill(phrase, id))
}

#[cfg(test)]
mod tests;
