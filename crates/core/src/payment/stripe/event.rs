//! Stripe's events, read for the few values the portal acts on.
//!
//! An event is an envelope (its id, when it was made, whether it is of
//! live mode, its type, and the version of Stripe's API it is rendered
//! for) around the object the event is about. Of the types the portal acts
//! on, the object is read for identifiers, amounts and states; whatever
//! else it holds (a payer's name, email and postal address, a card's
//! brand and last digits) is never decoded. Every other type is accepted
//! and ignored: Stripe sends whatever the endpoint is subscribed to, and
//! a subscription wider than needed must not make deliveries fail.
//!
//! Within a release Stripe only adds to its shapes, so a value the portal
//! does not know (a new mode, a new status) is read as "something else",
//! never refused. No refusal carries text from the event.

use serde::Deserialize;
use serde::de::{DeserializeOwned, IgnoredAny};
use serde_json::value::RawValue;

use super::{CardFingerprint, StripeId};
use crate::json::Malformation;
use crate::payment::catalogue::Currency;
use crate::payment::order::OrderId;
use crate::time::UnixTime;

/// The release of Stripe's API whose shapes the portal reads: the name
/// after the date in a version such as `2026-10-28.endive`. A webhook
/// endpoint keeps the version it was created with, so the operator
/// creates the portal's with one of this release.
pub const API_RELEASE: &str = "endive";
/// The largest event accepted, in bytes. Stripe's are a few kilobytes; a
/// checkout with many line items, a few tens.
pub const MAX_EVENT_BYTES: usize = 512 * 1024;

/// Why an event was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    /// Larger than [`MAX_EVENT_BYTES`].
    #[error("the event is larger than {MAX_EVENT_BYTES} bytes")]
    TooLarge,
    /// Not JSON, or not an event's envelope.
    #[error("the event does not parse: {0}")]
    Malformed(Malformation),
    /// Of a type the portal acts on, but rendered for another release of
    /// Stripe's API, whose shapes the portal was not written for.
    #[error(
        "the event is rendered for another release of Stripe's API than {API_RELEASE:?}; \
         create the webhook endpoint with an API version of that release"
    )]
    ApiRelease,
    /// Of a type the portal acts on, but its object is not that type's.
    #[error("the event's object is not what its type says: {0}")]
    Object(Malformation),
}

/// An event from Stripe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Its id. Stripe may deliver an event more than once; the id tells.
    pub id: StripeId,
    /// When Stripe made it.
    pub created: UnixTime,
    /// Whether it is about real payments.
    pub livemode: bool,
    /// What it says.
    pub kind: EventKind,
}

/// What an event says, for the types the portal acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// A checkout on a payment page was completed
    /// (`checkout.session.completed`), or its payment has since been made
    /// (`checkout.session.async_payment_succeeded`).
    CheckoutCompleted(CheckoutSession),
    /// A payment was charged (`charge.succeeded`).
    ChargeSucceeded(Charge),
    /// An invoice of a subscription was paid (`invoice.paid`).
    InvoicePaid(Invoice),
    /// Which payment paid an invoice (`invoice_payment.paid`).
    InvoicePaymentPaid(InvoicePayment),
    /// A subscription changed or ended (`customer.subscription.updated`,
    /// `customer.subscription.deleted`).
    SubscriptionChanged(SubscriptionChange),
    /// Some or all of a payment was refunded (`charge.refunded`).
    ChargeRefunded(Refunded),
    /// A cardholder disputed a payment with their bank
    /// (`charge.dispute.created`).
    DisputeOpened(Dispute),
    /// A type the portal does not act on, or an object of one it does that
    /// names no payment the portal could know.
    Unused,
}

/// A completed checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutSession {
    /// The session's id.
    pub id: StripeId,
    /// The order the payment page's address named, if it named one.
    pub order: Option<OrderId>,
    /// The Payment Link the checkout was made on.
    pub payment_link: Option<StripeId>,
    /// What the checkout set up.
    pub mode: CheckoutMode,
    /// Whether its payment is made.
    pub status: PaymentStatus,
    /// What was charged, in the currency's hundredths.
    pub amount_total: Option<u64>,
    /// The currency, if it is one the portal prices in.
    pub currency: Option<Currency>,
    /// The payment, for a checkout that takes one payment.
    pub payment_intent: Option<StripeId>,
    /// The subscription, for a checkout that starts one.
    pub subscription: Option<StripeId>,
    /// The subscription's first invoice.
    pub invoice: Option<StripeId>,
}

/// What a checkout sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutMode {
    /// One payment.
    Payment,
    /// A subscription, which Stripe charges again each period.
    Subscription,
    /// Anything else.
    Other,
}

/// Whether a checkout's payment is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatus {
    /// It is.
    Paid,
    /// Not yet: the method settles later, and Stripe says so again when
    /// it has.
    Unpaid,
    /// Nothing was to be paid, or a status the portal does not know.
    Other,
}

/// A successful charge: the card behind a payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charge {
    /// The payment charged.
    pub payment_intent: StripeId,
    /// What paid.
    pub card: CardUsed,
}

/// What a payment was made with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardUsed {
    /// A card, typed into Stripe's page: its number's fingerprint.
    Card(CardFingerprint),
    /// A card held in a wallet (Apple Pay, Google Pay, Link): the
    /// fingerprint may be the wallet's token's, not the card's.
    Wallet,
    /// Not a card, or a card without a fingerprint.
    Other,
}

/// A subscription's paid invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    /// The invoice's id.
    pub id: StripeId,
    /// The subscription it bills.
    pub subscription: StripeId,
    /// Why Stripe made it.
    pub reason: BillingReason,
    /// What was paid, in the currency's hundredths.
    pub amount_paid: u64,
    /// The currency, if it is one the portal prices in.
    pub currency: Option<Currency>,
}

/// Why Stripe made an invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillingReason {
    /// The subscription began: its checkout says the same.
    Create,
    /// A new period began: a renewal.
    Cycle,
    /// Anything else: a change of plan, an invoice made by hand.
    Other,
}

/// The payment that paid an invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoicePayment {
    /// The invoice.
    pub invoice: StripeId,
    /// The payment.
    pub payment_intent: StripeId,
}

/// A subscription's state after a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionChange {
    /// The subscription.
    pub id: StripeId,
    /// Whether Stripe will charge for it again.
    pub state: Renewal,
}

/// Whether Stripe will charge for a subscription again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Renewal {
    /// It will, each period.
    Active,
    /// Not after the period now running, unless its payer changes their
    /// mind before the period ends.
    Ending,
    /// Never: the subscription is over.
    Ended,
}

impl Renewal {
    /// Every state.
    pub const ALL: [Self; 3] = [Self::Active, Self::Ending, Self::Ended];

    /// The state as the database writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Ending => "ending",
            Self::Ended => "ended",
        }
    }

    /// Reads what [`Self::as_str`] writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == text)
    }
}

/// A refund of a payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refunded {
    /// The payment.
    pub payment_intent: StripeId,
    /// All that has been refunded of it so far, in the currency's
    /// hundredths.
    pub amount_refunded: u64,
    /// Whether that is all of it.
    pub in_full: bool,
}

/// A dispute of a payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispute {
    /// The payment.
    pub payment_intent: StripeId,
    /// What is disputed, in the currency's hundredths.
    pub amount: u64,
}

#[derive(Deserialize)]
struct Envelope<'a> {
    id: StripeId,
    api_version: Option<String>,
    created: UnixTime,
    livemode: bool,
    #[serde(rename = "type")]
    kind: String,
    #[serde(borrow)]
    data: Data<'a>,
}

#[derive(Deserialize)]
struct Data<'a> {
    #[serde(borrow)]
    object: &'a RawValue,
}

#[derive(Deserialize)]
struct WrittenSession {
    id: StripeId,
    client_reference_id: Option<String>,
    payment_link: Option<StripeId>,
    mode: String,
    payment_status: String,
    amount_total: Option<u64>,
    currency: Option<String>,
    payment_intent: Option<StripeId>,
    subscription: Option<StripeId>,
    invoice: Option<StripeId>,
}

#[derive(Deserialize)]
struct WrittenCharge {
    payment_intent: Option<StripeId>,
    payment_method_details: Option<WrittenMethod>,
}

#[derive(Deserialize)]
struct WrittenMethod {
    #[serde(rename = "type")]
    kind: String,
    card: Option<WrittenCard>,
}

#[derive(Deserialize)]
struct WrittenCard {
    fingerprint: Option<String>,
    wallet: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct WrittenRefunded {
    payment_intent: Option<StripeId>,
    amount_refunded: u64,
    refunded: bool,
}

#[derive(Deserialize)]
struct WrittenInvoice {
    id: StripeId,
    parent: Option<WrittenParent>,
    billing_reason: Option<String>,
    amount_paid: u64,
    currency: String,
}

#[derive(Deserialize)]
struct WrittenParent {
    subscription_details: Option<WrittenSubscriptionDetails>,
}

#[derive(Deserialize)]
struct WrittenSubscriptionDetails {
    subscription: StripeId,
}

#[derive(Deserialize)]
struct WrittenInvoicePayment {
    invoice: StripeId,
    payment: WrittenPayment,
}

#[derive(Deserialize)]
struct WrittenPayment {
    #[serde(rename = "type")]
    kind: String,
    payment_intent: Option<StripeId>,
}

#[derive(Deserialize)]
struct WrittenSubscription {
    id: StripeId,
    status: String,
    cancel_at_period_end: bool,
    cancel_at: Option<u64>,
}

#[derive(Deserialize)]
struct WrittenDispute {
    payment_intent: Option<StripeId>,
    amount: u64,
}

impl Event {
    /// Reads an event from a delivery's body. The body must already be
    /// known to come from Stripe ([`super::verify`]).
    pub fn parse(body: &[u8]) -> Result<Self, EventError> {
        if body.len() > MAX_EVENT_BYTES {
            return Err(EventError::TooLarge);
        }
        let envelope: Envelope = serde_json::from_slice(body)
            .map_err(|error| EventError::Malformed(Malformation::of(&error)))?;
        let read: Option<Reader> = match envelope.kind.as_str() {
            "checkout.session.completed" | "checkout.session.async_payment_succeeded" => {
                Some(checkout)
            }
            "charge.succeeded" => Some(charge),
            "invoice.paid" => Some(invoice),
            "invoice_payment.paid" => Some(invoice_payment),
            "customer.subscription.updated" => Some(subscription_updated),
            "customer.subscription.deleted" => Some(subscription_deleted),
            "charge.refunded" => Some(refunded),
            "charge.dispute.created" => Some(dispute),
            _ => None,
        };
        let kind = match read {
            None => EventKind::Unused,
            Some(read) => {
                // Only a shape that is about to be read needs to be of
                // the release the portal reads.
                let of_release = envelope
                    .api_version
                    .as_deref()
                    .and_then(|version| version.rsplit_once('.'))
                    .is_some_and(|(_, release)| release == API_RELEASE);
                if !of_release {
                    return Err(EventError::ApiRelease);
                }
                read(envelope.data.object)?
            }
        };
        Ok(Self {
            id: envelope.id,
            created: envelope.created,
            livemode: envelope.livemode,
            kind,
        })
    }
}

/// Reads the object of one type of event.
type Reader = fn(&RawValue) -> Result<EventKind, EventError>;

/// An object as its type's shape.
fn written<T: DeserializeOwned>(object: &RawValue) -> Result<T, EventError> {
    serde_json::from_str(object.get()).map_err(|error| EventError::Object(Malformation::of(&error)))
}

fn checkout(object: &RawValue) -> Result<EventKind, EventError> {
    let session: WrittenSession = written(object)?;
    Ok(EventKind::CheckoutCompleted(CheckoutSession {
        id: session.id,
        // A reference that is not an order's id names no order: anyone
        // can open a payment page with a reference of their own.
        order: session
            .client_reference_id
            .as_deref()
            .and_then(|reference| OrderId::parse(reference).ok()),
        payment_link: session.payment_link,
        mode: match session.mode.as_str() {
            "payment" => CheckoutMode::Payment,
            "subscription" => CheckoutMode::Subscription,
            _ => CheckoutMode::Other,
        },
        status: match session.payment_status.as_str() {
            "paid" => PaymentStatus::Paid,
            "unpaid" => PaymentStatus::Unpaid,
            _ => PaymentStatus::Other,
        },
        amount_total: session.amount_total,
        currency: session.currency.as_deref().and_then(Currency::parse),
        payment_intent: session.payment_intent,
        subscription: session.subscription,
        invoice: session.invoice,
    }))
}

fn charge(object: &RawValue) -> Result<EventKind, EventError> {
    let charge: WrittenCharge = written(object)?;
    // A charge made without a payment is none of a checkout's.
    let Some(payment_intent) = charge.payment_intent else {
        return Ok(EventKind::Unused);
    };
    let card = match charge.payment_method_details {
        Some(method) if method.kind == "card" => match method.card {
            Some(card) if card.wallet.is_some() => CardUsed::Wallet,
            Some(card) => card
                .fingerprint
                .as_deref()
                .and_then(|text| CardFingerprint::parse(text).ok())
                .map_or(CardUsed::Other, CardUsed::Card),
            None => CardUsed::Other,
        },
        _ => CardUsed::Other,
    };
    Ok(EventKind::ChargeSucceeded(Charge {
        payment_intent,
        card,
    }))
}

fn invoice(object: &RawValue) -> Result<EventKind, EventError> {
    let invoice: WrittenInvoice = written(object)?;
    // An invoice that bills no subscription is not for anything the
    // portal sells.
    let Some(subscription) = invoice
        .parent
        .and_then(|parent| parent.subscription_details)
        .map(|details| details.subscription)
    else {
        return Ok(EventKind::Unused);
    };
    Ok(EventKind::InvoicePaid(Invoice {
        id: invoice.id,
        subscription,
        reason: match invoice.billing_reason.as_deref() {
            Some("subscription_create") => BillingReason::Create,
            Some("subscription_cycle") => BillingReason::Cycle,
            _ => BillingReason::Other,
        },
        amount_paid: invoice.amount_paid,
        currency: Currency::parse(&invoice.currency),
    }))
}

fn invoice_payment(object: &RawValue) -> Result<EventKind, EventError> {
    let paid: WrittenInvoicePayment = written(object)?;
    Ok(
        match (paid.payment.kind.as_str(), paid.payment.payment_intent) {
            ("payment_intent", Some(payment_intent)) => {
                EventKind::InvoicePaymentPaid(InvoicePayment {
                    invoice: paid.invoice,
                    payment_intent,
                })
            }
            // Paid some other way: there is no card to check.
            _ => EventKind::Unused,
        },
    )
}

fn subscription_updated(object: &RawValue) -> Result<EventKind, EventError> {
    let subscription: WrittenSubscription = written(object)?;
    let state = match subscription.status.as_str() {
        "canceled" | "incomplete_expired" => Renewal::Ended,
        _ if subscription.cancel_at_period_end || subscription.cancel_at.is_some() => {
            Renewal::Ending
        }
        _ => Renewal::Active,
    };
    Ok(EventKind::SubscriptionChanged(SubscriptionChange {
        id: subscription.id,
        state,
    }))
}

fn subscription_deleted(object: &RawValue) -> Result<EventKind, EventError> {
    let subscription: WrittenSubscription = written(object)?;
    Ok(EventKind::SubscriptionChanged(SubscriptionChange {
        id: subscription.id,
        state: Renewal::Ended,
    }))
}

fn refunded(object: &RawValue) -> Result<EventKind, EventError> {
    let charge: WrittenRefunded = written(object)?;
    Ok(charge
        .payment_intent
        .map_or(EventKind::Unused, |payment_intent| {
            EventKind::ChargeRefunded(Refunded {
                payment_intent,
                amount_refunded: charge.amount_refunded,
                in_full: charge.refunded,
            })
        }))
}

fn dispute(object: &RawValue) -> Result<EventKind, EventError> {
    let dispute: WrittenDispute = written(object)?;
    Ok(dispute
        .payment_intent
        .map_or(EventKind::Unused, |payment_intent| {
            EventKind::DisputeOpened(Dispute {
                payment_intent,
                amount: dispute.amount,
            })
        }))
}

#[cfg(test)]
mod tests;
