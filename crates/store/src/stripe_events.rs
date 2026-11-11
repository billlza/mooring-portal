//! What Stripe says, written down as it arrives.
//!
//! Stripe delivers an event at least once and in no particular order, so
//! each is recorded once, by its id, and what it says is kept in a form
//! that does not depend on what came before it: a checkout is matched to
//! its order at once, since the order is placed before anyone is sent to
//! pay, and takes with it the renewals that were told of before it;
//! everything else is a fact kept by Stripe's own id (a payment's
//! card, refunds and dispute; an invoice's payment; a subscription's
//! state) until the order it belongs to is known. After every event the
//! facts are brought together (`settle.rs`), in the same transaction, so
//! that an event is either recorded with all it leads to or not at all.
//!
//! A payment that is no order's is kept for the operator
//! (`stray_payments`): the portal grants nothing for it and guesses
//! nothing about it.
//!
//! An event of the other mode than the database's (`payment_mode.rs`) is
//! not recorded at all.

use portal_core::payment::stripe::StripeId;
use portal_core::payment::stripe::{
    BillingReason, CardUsed, Charge, CheckoutSession, Dispute, Event, EventKind, Invoice,
    InvoicePayment, PaymentStatus, Refunded, SubscriptionChange,
};
use portal_core::payment::{CardVerdict, Currency, OrderId, OrderTerms};
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::orders::{terms_row, terms_to_sql};
use crate::payment_mode::mode;
use crate::rows::{count_from_sql, count_to_sql, stripe_id_from_sql, time_from_sql, time_to_sql};
use crate::settle::settle;
use crate::{Store, StoreError};

/// What became of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// It was new, and is now recorded with all it led to.
    New,
    /// Stripe had delivered it before; nothing changed.
    Duplicate,
    /// It is of the other mode than the database's (a rehearsal's event
    /// to a database of real payments, or the reverse), or the database
    /// has entered no mode. Nothing of it is recorded, not even that it
    /// came, so Stripe's next delivery of it is judged afresh.
    OtherMode,
}

/// Why a payment is no order's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrayReason {
    /// The checkout named no order, or none the portal has.
    NoOrder,
    /// The checkout named an order that another checkout had paid.
    PaidAgain,
    /// An invoice renewed a subscription that no order began.
    NoSubscription,
    /// An invoice that neither began a subscription nor renewed one.
    OtherInvoice,
}

impl StrayReason {
    /// Every reason.
    pub const ALL: [Self; 4] = [
        Self::NoOrder,
        Self::PaidAgain,
        Self::NoSubscription,
        Self::OtherInvoice,
    ];

    /// The reason as the database writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoOrder => "no_order",
            Self::PaidAgain => "paid_again",
            Self::NoSubscription => "no_subscription",
            Self::OtherInvoice => "other_invoice",
        }
    }

    /// Reads what [`Self::as_str`] writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.as_str() == text)
    }
}

impl Store {
    /// Records `event`, received at `now`, with all it leads to. `on_sale`
    /// is what the catalogue offers now: the checkout of an order placed
    /// on other terms waits for the operator.
    pub fn record_stripe_event(
        &mut self,
        event: &Event,
        on_sale: &[OrderTerms],
        now: UnixTime,
    ) -> Result<Recorded, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if mode(&tx)? != Some(event.livemode) {
            return Ok(Recorded::OtherMode);
        }
        let fresh = tx.execute(
            "INSERT OR IGNORE INTO stripe_events (id, received_at) VALUES (?1, ?2)",
            params![event.id.as_str(), time_to_sql(now)],
        )?;
        if fresh == 0 {
            return Ok(Recorded::Duplicate);
        }
        match &event.kind {
            EventKind::CheckoutCompleted(checkout) => {
                completed(&tx, checkout, on_sale, event.created, now)
            }
            EventKind::ChargeSucceeded(charge) => charged(&tx, charge, now),
            EventKind::InvoicePaid(invoice) => invoiced(&tx, invoice, event.created, now),
            EventKind::InvoicePaymentPaid(payment) => invoice_paid_by(&tx, payment, now),
            EventKind::SubscriptionChanged(change) => changed(&tx, change, event.created, now),
            EventKind::ChargeRefunded(refund) => refunded(&tx, refund, now),
            EventKind::DisputeOpened(dispute) => disputed(&tx, dispute, now),
            EventKind::Unused => Ok(()),
        }?;
        settle(&tx, now)?;
        tx.commit()?;
        Ok(Recorded::New)
    }
}

/// A completed checkout: the payment of the order it names, if that order
/// is still unpaid, and otherwise a payment for the operator to look at.
fn completed(
    tx: &Transaction<'_>,
    checkout: &CheckoutSession,
    on_sale: &[OrderTerms],
    made: UnixTime,
    now: UnixTime,
) -> Result<(), StoreError> {
    // Stripe tells again once a payment that settles later has.
    if checkout.status == PaymentStatus::Unpaid {
        return Ok(());
    }
    let session = checkout.id.as_str();
    let seen: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM orders WHERE session = ?1)
             OR EXISTS (SELECT 1 FROM stray_payments WHERE session = ?1)",
        [session],
        |row| row.get(0),
    )?;
    if seen {
        return Ok(());
    }
    let named = match &checkout.order {
        Some(order) => tx
            .query_row(
                "SELECT state, plan, plan_kind, days, traffic_bytes, amount, currency,
                        payment_link_id
                 FROM orders WHERE id = ?1",
                [order.as_str()],
                |row| Ok((row.get::<_, String>(0)?, terms_row(row, 1)?)),
            )
            .optional()?
            .map(|found| (order, found)),
        None => None,
    };
    let stray = |reason: StrayReason| -> Result<(), StoreError> {
        let amount = checkout
            .amount_total
            .map(|amount| count_to_sql(amount, "paid amount"))
            .transpose()?;
        tx.execute(
            "INSERT INTO stray_payments (noticed_at, made_at, reason, session, invoice,
                 payment_intent, subscription, amount, currency)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                time_to_sql(now),
                time_to_sql(made),
                reason.as_str(),
                session,
                checkout.invoice.as_ref().map(|id| id.as_str()),
                checkout.payment_intent.as_ref().map(|id| id.as_str()),
                checkout.subscription.as_ref().map(|id| id.as_str()),
                amount,
                checkout.currency.map(|currency| currency.as_str()),
            ],
        )?;
        Ok(())
    };
    match named {
        None => stray(StrayReason::NoOrder),
        Some((order, (state, terms))) if state == "pending" => {
            let terms = terms.checked()?;
            let verdict = terms
                .accepts(checkout)
                .and_then(|()| terms.still_sold(on_sale));
            tx.execute(
                "UPDATE orders SET state = ?2, review_reason = ?3, paid_at = ?4, session = ?5,
                     payment_intent = ?6, invoice = ?7, subscription = ?8
                 WHERE id = ?1",
                params![
                    order.as_str(),
                    if verdict.is_ok() { "paid" } else { "review" },
                    verdict.err().map(|reason| reason.as_str()),
                    time_to_sql(made),
                    session,
                    checkout.payment_intent.as_ref().map(|id| id.as_str()),
                    checkout.invoice.as_ref().map(|id| id.as_str()),
                    checkout.subscription.as_ref().map(|id| id.as_str()),
                ],
            )?;
            match &checkout.subscription {
                Some(subscription) => adopt_renewals(tx, subscription),
                None => Ok(()),
            }
        }
        Some(_) => stray(StrayReason::PaidAgain),
    }
}

/// The renewals of `subscription` that Stripe told of before the checkout
/// that began it. Each was kept as a payment that is no order's, and is
/// now entered as the order it would have been.
fn adopt_renewals(tx: &Transaction<'_>, subscription: &StripeId) -> Result<(), StoreError> {
    let early: Vec<(i64, String, i64, Option<String>, i64)> = tx
        .prepare(
            "SELECT id, invoice, amount, currency, made_at FROM stray_payments
             WHERE reason = 'no_subscription' AND subscription = ?1 ORDER BY id",
        )?
        .query_map([subscription.as_str()], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    for (stray, invoice, amount, currency, made) in early {
        let invoice = Invoice {
            id: stripe_id_from_sql(&invoice)?,
            subscription: subscription.clone(),
            reason: BillingReason::Cycle,
            amount_paid: count_from_sql(amount, "stray payment's amount")?,
            currency: currency.as_deref().and_then(Currency::parse),
        };
        if renew(tx, &invoice, time_from_sql(made, "stray payment's time")?)? {
            tx.execute("DELETE FROM stray_payments WHERE id = ?1", [stray])?;
        }
    }
    Ok(())
}

/// Enters the renewal that `invoice` paid, at `made`, as an order after
/// the one that began its subscription; whether an order did begin it.
fn renew(tx: &Transaction<'_>, invoice: &Invoice, made: UnixTime) -> Result<bool, StoreError> {
    let began = tx
        .query_row(
            "SELECT id, account_id, mooring_user, plan, plan_kind, days, traffic_bytes, amount,
                    currency, payment_link_id
             FROM orders WHERE subscription = ?1 AND renewal_of IS NULL
             ORDER BY created_at, id LIMIT 1",
            [invoice.subscription.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    terms_row(row, 3)?,
                ))
            },
        )
        .optional()?;
    let Some((first, account, user, terms)) = began else {
        return Ok(false);
    };
    let terms = terms.checked()?;
    let verdict = terms.accepts_renewal(invoice);
    let (plan, kind, days, traffic_bytes, amount, currency, link) = terms_to_sql(&terms)?;
    tx.execute(
        "INSERT INTO orders (id, account_id, mooring_user, plan, plan_kind, days,
             traffic_bytes, amount, currency, payment_link_id, created_at, state,
             review_reason, paid_at, invoice, subscription, renewal_of)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?11, ?14, ?15, ?16)",
        params![
            OrderId::for_renewal(&invoice.id).as_str(),
            account,
            user,
            plan,
            kind,
            days,
            traffic_bytes,
            amount,
            currency,
            link,
            time_to_sql(made),
            if verdict.is_ok() { "paid" } else { "review" },
            verdict.err().map(|reason| reason.as_str()),
            invoice.id.as_str(),
            invoice.subscription.as_str(),
            first,
        ],
    )?;
    Ok(true)
}

/// A charge: whether the payment's card is a banned account's. Only the
/// verdict is kept, never the card.
fn charged(tx: &Transaction<'_>, charge: &Charge, now: UnixTime) -> Result<(), StoreError> {
    let verdict = match &charge.card {
        CardUsed::Card(card) => {
            let banned: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM banned_cards WHERE card_digest = ?1)",
                [card.digest().as_bytes()],
                |row| row.get(0),
            )?;
            if banned {
                CardVerdict::Banned
            } else {
                CardVerdict::Clear
            }
        }
        CardUsed::Wallet | CardUsed::Other => CardVerdict::Unchecked,
    };
    // A payment is charged once. Were Stripe ever to tell of two charges,
    // the worse verdict stands.
    tx.execute(
        "INSERT INTO stripe_payments (payment_intent, card, noticed_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (payment_intent) DO UPDATE SET
             card = CASE
                 WHEN stripe_payments.card IS NULL THEN excluded.card
                 WHEN 'banned' IN (stripe_payments.card, excluded.card) THEN 'banned'
                 WHEN 'unchecked' IN (stripe_payments.card, excluded.card) THEN 'unchecked'
                 ELSE 'clear' END,
             noticed_at = excluded.noticed_at",
        params![
            charge.payment_intent.as_str(),
            verdict.as_str(),
            time_to_sql(now)
        ],
    )?;
    Ok(())
}

/// A paid invoice: a renewal of the subscription an order began, or a
/// payment for the operator to look at.
fn invoiced(
    tx: &Transaction<'_>,
    invoice: &Invoice,
    made: UnixTime,
    now: UnixTime,
) -> Result<(), StoreError> {
    // A subscription's first invoice is paid by its checkout, which says
    // the same and more.
    if invoice.reason == BillingReason::Create {
        return Ok(());
    }
    let seen: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM orders WHERE invoice = ?1)
             OR EXISTS (SELECT 1 FROM stray_payments WHERE invoice = ?1)",
        [invoice.id.as_str()],
        |row| row.get(0),
    )?;
    if seen {
        return Ok(());
    }
    let reason = match invoice.reason {
        BillingReason::Cycle if renew(tx, invoice, made)? => return Ok(()),
        // The checkout that began the subscription may yet come: if it
        // does, it takes this renewal with it.
        BillingReason::Cycle => StrayReason::NoSubscription,
        _ => StrayReason::OtherInvoice,
    };
    tx.execute(
        "INSERT INTO stray_payments (noticed_at, made_at, reason, invoice, subscription, amount,
             currency)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            time_to_sql(now),
            time_to_sql(made),
            reason.as_str(),
            invoice.id.as_str(),
            invoice.subscription.as_str(),
            count_to_sql(invoice.amount_paid, "paid amount")?,
            invoice.currency.map(|currency| currency.as_str()),
        ],
    )?;
    Ok(())
}

/// Which payment paid an invoice. An invoice is paid once; the first
/// payment told of stands.
fn invoice_paid_by(
    tx: &Transaction<'_>,
    payment: &InvoicePayment,
    now: UnixTime,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO stripe_invoice_payments (invoice, payment_intent, noticed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (invoice) DO UPDATE SET noticed_at = excluded.noticed_at",
        params![
            payment.invoice.as_str(),
            payment.payment_intent.as_str(),
            time_to_sql(now)
        ],
    )?;
    Ok(())
}

/// A subscription's state. One that has ended stays ended; otherwise the
/// state Stripe made last stands, whichever arrived last.
fn changed(
    tx: &Transaction<'_>,
    change: &SubscriptionChange,
    made: UnixTime,
    now: UnixTime,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO stripe_subscriptions (subscription, state, state_at, noticed_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (subscription) DO UPDATE SET
             state = CASE
                 WHEN 'ended' IN (stripe_subscriptions.state, excluded.state) THEN 'ended'
                 WHEN excluded.state_at >= stripe_subscriptions.state_at THEN excluded.state
                 ELSE stripe_subscriptions.state END,
             state_at = max(stripe_subscriptions.state_at, excluded.state_at),
             noticed_at = excluded.noticed_at",
        params![
            change.id.as_str(),
            change.state.as_str(),
            time_to_sql(made),
            time_to_sql(now)
        ],
    )?;
    Ok(())
}

/// A refund: all that has gone back of a payment so far. Stripe tells
/// the running total, so the largest told of stands.
fn refunded(tx: &Transaction<'_>, refund: &Refunded, now: UnixTime) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO stripe_payments (payment_intent, refunded_amount, refunded_in_full,
             noticed_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (payment_intent) DO UPDATE SET
             refunded_amount = max(stripe_payments.refunded_amount, excluded.refunded_amount),
             refunded_in_full = max(stripe_payments.refunded_in_full, excluded.refunded_in_full),
             noticed_at = excluded.noticed_at",
        params![
            refund.payment_intent.as_str(),
            count_to_sql(refund.amount_refunded, "refunded amount")?,
            refund.in_full,
            time_to_sql(now)
        ],
    )?;
    Ok(())
}

/// A dispute of a payment.
fn disputed(tx: &Transaction<'_>, dispute: &Dispute, now: UnixTime) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO stripe_payments (payment_intent, disputed_amount, noticed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (payment_intent) DO UPDATE SET
             disputed_amount =
                 max(COALESCE(stripe_payments.disputed_amount, 0), excluded.disputed_amount),
             noticed_at = excluded.noticed_at",
        params![
            dispute.payment_intent.as_str(),
            count_to_sql(dispute.amount, "disputed amount")?,
            time_to_sql(now)
        ],
    )?;
    Ok(())
}
