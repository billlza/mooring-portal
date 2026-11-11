//! The entitlements Mooring pulls: a journal of what the portal asks
//! Mooring to grant, and of what has since happened to the money.
//!
//! Each entry has a number that only grows, so Mooring keeps a cursor:
//! it asks for the entries after the last one it has acted on, and acts
//! on each order once. A grant names a paid order whose card has passed
//! the ban list; a refund or a dispute names a granted order whose
//! payment was returned or is contested, and what Mooring does about it
//! is Mooring's to decide. An entry carries its order's terms whole, so
//! none needs another to be understood.
//!
//! The format is written here and read by Mooring; `docs/contract/
//! entitlements-v1.md` specifies it, and the golden documents under
//! `contract/fixtures/entitlements-v1/` are what this module writes.

use serde::Serialize;

use super::order::OrderId;
use super::terms::OrderTerms;
use crate::snapshot::MooringUserId;
use crate::time::UnixTime;

/// What a document's `format` says.
pub const FORMAT: &str = "mooring-portal-entitlements";
/// The version of the format this build writes.
pub const VERSION: u32 = 1;
/// Entries in one document at most. Mooring asks again while a document
/// says there are more.
pub const MAX_ENTRIES: u32 = 200;

/// What an entry asks or tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Grant the order.
    Grant,
    /// Some or all of the order's payment has been refunded.
    Refund {
        /// All that has been refunded so far, in the currency's
        /// hundredths.
        amount_refunded: u64,
        /// Whether that is the whole payment, as Stripe says: the amount
        /// alone does not tell, since what was paid may differ from the
        /// plan's price when the operator approved the order.
        in_full: bool,
    },
    /// The order's payer disputes the payment with their bank.
    Dispute {
        /// The amount disputed.
        amount_disputed: u64,
    },
}

/// One entry of the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its number: greater than every earlier entry's.
    pub seq: u64,
    /// What it asks or tells.
    pub kind: EntryKind,
    /// The order.
    pub order: OrderId,
    /// The Mooring user the order's account was bound to when the order
    /// was placed, and still was when it was granted.
    pub user: MooringUserId,
    /// What the order is for.
    pub terms: OrderTerms,
    /// When the order's payment was made.
    pub paid_at: UnixTime,
    /// For a subscription's renewal, the order that began the
    /// subscription.
    pub renewal_of: Option<OrderId>,
    /// When the portal wrote the entry.
    pub recorded_at: UnixTime,
}

/// The entries after a cursor, as one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// When the document was made.
    pub generated_at: UnixTime,
    /// Whether the orders were paid with real money. Mooring grants
    /// nothing from a document that says they were not.
    pub livemode: bool,
    /// The cursor the entries follow.
    pub after: u64,
    /// The entries, by number, at most [`MAX_ENTRIES`].
    pub entries: Vec<Entry>,
    /// Whether the journal holds entries after these.
    pub more: bool,
}

#[derive(Serialize)]
struct WrittenPage<'a> {
    format: &'static str,
    version: u32,
    generated_at: UnixTime,
    livemode: bool,
    after: u64,
    entries: Vec<WrittenEntry<'a>>,
    next: u64,
    more: bool,
}

#[derive(Serialize)]
struct WrittenEntry<'a> {
    seq: u64,
    kind: &'static str,
    order: &'a str,
    user: &'a str,
    plan: &'a str,
    plan_kind: &'static str,
    days: u32,
    traffic_bytes: u64,
    amount: u64,
    currency: &'static str,
    paid_at: UnixTime,
    renewal_of: Option<&'a str>,
    amount_refunded: Option<u64>,
    refunded_in_full: Option<bool>,
    amount_disputed: Option<u64>,
    recorded_at: UnixTime,
}

impl Page {
    /// The cursor to ask with next: the last entry's number, or the same
    /// cursor again when there is none.
    #[must_use]
    pub fn next(&self) -> u64 {
        self.entries.last().map_or(self.after, |entry| entry.seq)
    }

    /// The document, as compact JSON. Every key is always written; one
    /// that does not apply is `null`.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                let (kind, refund, amount_disputed) = match entry.kind {
                    EntryKind::Grant => ("grant", None, None),
                    EntryKind::Refund {
                        amount_refunded,
                        in_full,
                    } => ("refund", Some((amount_refunded, in_full)), None),
                    EntryKind::Dispute { amount_disputed } => {
                        ("dispute", None, Some(amount_disputed))
                    }
                };
                WrittenEntry {
                    seq: entry.seq,
                    kind,
                    order: entry.order.as_str(),
                    user: entry.user.as_str(),
                    plan: entry.terms.plan.as_str(),
                    plan_kind: entry.terms.kind.as_str(),
                    days: entry.terms.days,
                    traffic_bytes: entry.terms.traffic_bytes,
                    amount: entry.terms.amount,
                    currency: entry.terms.currency.as_str(),
                    paid_at: entry.paid_at,
                    renewal_of: entry.renewal_of.as_ref().map(OrderId::as_str),
                    amount_refunded: refund.map(|(amount, _)| amount),
                    refunded_in_full: refund.map(|(_, in_full)| in_full),
                    amount_disputed,
                    recorded_at: entry.recorded_at,
                }
            })
            .collect();
        serde_json::to_string(&WrittenPage {
            format: FORMAT,
            version: VERSION,
            generated_at: self.generated_at,
            livemode: self.livemode,
            after: self.after,
            entries,
            next: self.next(),
            more: self.more,
        })
    }
}
