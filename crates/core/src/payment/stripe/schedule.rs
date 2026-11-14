//! The fields needed to associate a Stripe schedule with its first paid order.

use serde::Deserialize;
use serde_json::value::RawValue;

use super::StripeId;
use super::event::{EventError, EventKind, written};
use crate::payment::OrderId;
use crate::time::UnixTime;

/// A Stripe customer id used to reconcile a paid introduction; never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct IntroCustomer(StripeId);

impl IntroCustomer {
    /// Stripe's customer id, for an equality check or a database lookup.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<StripeId> for IntroCustomer {
    fn from(id: StripeId) -> Self {
        Self(id)
    }
}

impl std::fmt::Debug for IntroCustomer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IntroCustomer(<redacted>)")
    }
}

/// Stripe's evidence that a first-period card payment can be reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroPayment {
    /// Paid intent.
    pub id: StripeId,
    /// The customer to which Stripe saved the payment method.
    pub customer: IntroCustomer,
}

#[derive(Deserialize)]
struct WrittenPayment {
    id: StripeId,
    customer: Option<StripeId>,
    setup_future_usage: Option<String>,
    payment_method: Option<StripeId>,
    metadata: Option<PaymentMetadata>,
}

#[derive(Deserialize)]
struct PaymentMetadata {
    moorlight_offer: Option<String>,
}

pub(super) fn read_intro_payment(object: &RawValue) -> Result<EventKind, EventError> {
    let payment: WrittenPayment = written(object)?;
    if let (Some(customer), Some(_)) = (payment.customer, payment.payment_method)
        && payment
            .metadata
            .and_then(|meta| meta.moorlight_offer)
            .as_deref()
            == Some("intro_v2")
        && payment.setup_future_usage.as_deref() == Some("off_session")
    {
        return Ok(EventKind::IntroPaymentSucceeded(IntroPayment {
            id: payment.id,
            customer: customer.into(),
        }));
    }
    Ok(EventKind::Unused)
}

/// Stripe's view of the schedule for a paid introductory period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleChange {
    /// The schedule being updated.
    pub id: StripeId,
    /// The first period's payer.
    pub customer: IntroCustomer,
    /// The order id set by the operator as `moorlight_intro_order`.
    pub introductory_order: Option<OrderId>,
    /// The first renewal invoice's time, not the first checkout's time.
    pub start: Option<UnixTime>,
    /// The end of the minimum term.
    pub end: Option<UnixTime>,
    /// Set when Stripe creates the recurring subscription.
    pub subscription: Option<StripeId>,
    /// Stripe will stop the subscription after the final phase.
    pub cancels: bool,
    /// The schedule no longer controls future charges.
    pub ended: bool,
    /// Stripe completed this schedule with its normal end behavior.
    pub completed: bool,
}

#[derive(Deserialize)]
struct WrittenSchedule {
    id: StripeId,
    customer: StripeId,
    subscription: Option<StripeId>,
    released_subscription: Option<StripeId>,
    status: String,
    end_behavior: String,
    metadata: Metadata,
    phases: Vec<Phase>,
}

#[derive(Deserialize)]
struct Metadata {
    moorlight_intro_order: Option<String>,
}

#[derive(Deserialize)]
struct Phase {
    start_date: UnixTime,
    end_date: UnixTime,
}

pub(super) fn read_schedule(object: &RawValue) -> Result<EventKind, EventError> {
    let schedule: WrittenSchedule = written(object)?;
    let order = schedule
        .metadata
        .moorlight_intro_order
        .as_deref()
        .and_then(|id| OrderId::parse(id).ok());
    if order.is_none() {
        return Ok(EventKind::Unused);
    }
    let completed = schedule.status == "completed";
    let open = matches!(schedule.status.as_str(), "not_started" | "active");
    Ok(EventKind::ScheduleChanged(ScheduleChange {
        id: schedule.id,
        customer: schedule.customer.into(),
        introductory_order: order,
        start: schedule.phases.first().map(|p| p.start_date),
        end: schedule.phases.last().map(|p| p.end_date),
        subscription: schedule.subscription.or(schedule.released_subscription),
        cancels: schedule.end_behavior == "cancel" && (open || completed),
        ended: !open,
        completed,
    }))
}
