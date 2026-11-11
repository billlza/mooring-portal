//! What an order is for, and whether a payment was for it.
//!
//! An order keeps its plan's terms as they stood when it was placed. A
//! payment is granted only if it is exactly the one the order asked for:
//! made, on the order's payment page, of the order's kind, for the
//! order's price, with a card that is not a banned account's, while the
//! account is still bound to the Mooring user the order names, and while
//! the plan is still sold on the order's terms. Anything else is the
//! operator's to look at ([`ReviewReason`]): the portal never guesses what
//! a payer meant.

use super::catalogue::{Currency, Plan, PlanId, PlanKind};
use super::stripe::{CheckoutMode, CheckoutSession, Invoice, PaymentStatus};

/// What an order is for: its plan's terms when it was placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTerms {
    /// The plan's id.
    pub plan: PlanId,
    /// What the plan is.
    pub kind: PlanKind,
    /// The days of service it buys, or the days its traffic stays valid.
    pub days: u32,
    /// Its traffic, in bytes.
    pub traffic_bytes: u64,
    /// Its price, in the currency's hundredths.
    pub amount: u64,
    /// Its currency.
    pub currency: Currency,
    /// The id of its payment page at Stripe.
    pub payment_link_id: String,
}

impl OrderTerms {
    /// The terms of `plan` as the catalogue has them now.
    #[must_use]
    pub fn of(plan: &Plan) -> Self {
        Self {
            plan: plan.id.clone(),
            kind: plan.kind,
            days: plan.days,
            traffic_bytes: plan.traffic_bytes,
            amount: plan.amount,
            currency: plan.currency,
            payment_link_id: plan.payment_link_id.clone(),
        }
    }

    /// Whether `checkout` is the payment an order on these terms asked
    /// for; if not, the first thing about it that is not.
    pub fn accepts(&self, checkout: &CheckoutSession) -> Result<(), ReviewReason> {
        if checkout.status != PaymentStatus::Paid {
            return Err(ReviewReason::Unpaid);
        }
        let on_this_page = checkout
            .payment_link
            .as_ref()
            .is_some_and(|link| link.as_str() == self.payment_link_id);
        if !on_this_page {
            return Err(ReviewReason::Link);
        }
        // A renewing plan is paid by a subscription, which bills by
        // invoice; anything else by one payment. Each must carry what the
        // card check and the renewals are found by.
        let of_this_kind = if self.kind.renews() {
            checkout.mode == CheckoutMode::Subscription
                && checkout.subscription.is_some()
                && checkout.invoice.is_some()
        } else {
            checkout.mode == CheckoutMode::Payment
                && checkout.payment_intent.is_some()
                && checkout.subscription.is_none()
        };
        if !of_this_kind {
            return Err(ReviewReason::Mode);
        }
        self.priced(checkout.amount_total, checkout.currency)
    }

    /// Whether these terms are among `on_sale`, the terms the catalogue
    /// offers now. An order keeps the terms it was placed on, and its
    /// page stops offering its payment once the catalogue has others; one
    /// paid all the same, from an address kept, is not granted unseen.
    pub fn still_sold(&self, on_sale: &[Self]) -> Result<(), ReviewReason> {
        if on_sale.contains(self) {
            Ok(())
        } else {
            Err(ReviewReason::Terms)
        }
    }

    /// Whether `invoice` is a renewal at these terms.
    pub fn accepts_renewal(&self, invoice: &Invoice) -> Result<(), ReviewReason> {
        self.priced(Some(invoice.amount_paid), invoice.currency)
    }

    fn priced(&self, amount: Option<u64>, currency: Option<Currency>) -> Result<(), ReviewReason> {
        if amount == Some(self.amount) && currency == Some(self.currency) {
            Ok(())
        } else {
            Err(ReviewReason::Amount)
        }
    }
}

/// Why a payment is not granted until the operator has looked at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewReason {
    /// The checkout was completed without a payment.
    Unpaid,
    /// It was made on another payment page than the order's plan's.
    Link,
    /// It is not the kind of checkout the plan asks for: one payment for
    /// a renewing plan, or a subscription for one that does not renew.
    Mode,
    /// It is not for the order's price, or not in its currency.
    Amount,
    /// It is the payment the order asked for, but the plan is no longer
    /// sold on the order's terms: the catalogue changed between the order
    /// and its payment.
    Terms,
    /// It was paid with a card of a banned account.
    CardBanned,
    /// It was paid from a wallet or by another method, so the card itself
    /// could not be checked against the ban list.
    CardUnchecked,
    /// The account is no longer bound to the Mooring user the order
    /// names.
    Binding,
}

impl ReviewReason {
    /// Every reason.
    pub const ALL: [Self; 8] = [
        Self::Unpaid,
        Self::Link,
        Self::Mode,
        Self::Amount,
        Self::Terms,
        Self::CardBanned,
        Self::CardUnchecked,
        Self::Binding,
    ];

    /// The reason as the database writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unpaid => "unpaid",
            Self::Link => "link",
            Self::Mode => "mode",
            Self::Amount => "amount",
            Self::Terms => "terms",
            Self::CardBanned => "card_banned",
            Self::CardUnchecked => "card_unchecked",
            Self::Binding => "binding",
        }
    }

    /// Reads what [`Self::as_str`] writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.as_str() == text)
    }
}

/// What the check of a payment's card against the ban list found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CardVerdict {
    /// A card, and no banned account's.
    Clear,
    /// A banned account's card.
    Banned,
    /// Not a card whose number Stripe fingerprints: a wallet, or another
    /// method.
    Unchecked,
}

impl CardVerdict {
    /// Every verdict.
    pub const ALL: [Self; 3] = [Self::Clear, Self::Banned, Self::Unchecked];

    /// The verdict as the database writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Banned => "banned",
            Self::Unchecked => "unchecked",
        }
    }

    /// Reads what [`Self::as_str`] writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|verdict| verdict.as_str() == text)
    }
}

#[cfg(test)]
mod tests;
