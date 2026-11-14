//! The private introductory offer and the end of its initial term.
//!
//! Mooring's operator knows who is new; a portal registration or a billing
//! email doesn't establish that. The operator issues one private Payment
//! Link to a bound Mooring id. An order snapshots the link, and a paid
//! checkout has to name both before it can claim that offer.
//!
//! A request to stop is durable, but only Stripe's confirmation changes
//! what we tell the customer about billing. The operator uses Workbench
//! for writes; the portal still holds no key that can start a charge.

use portal_core::account::AccountCode;
use portal_core::payment::stripe::{CheckoutSession, IntroPayment, ScheduleChange, StripeId};
use portal_core::payment::{INTRO_PERIOD_SECS, INTRO_PERIODS, OrderId, OrderTerms};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::payment_mode::mode;
use crate::rows::{order_from_sql, stripe_id_from_sql, time_from_sql, time_to_sql};
use crate::{AccountId, Store, StoreError};

/// An unclaimed link issued to a bound Mooring user. Its URL is private.
#[derive(Clone, PartialEq, Eq)]
pub struct IntroLink {
    /// Payment Link id as Stripe includes it in the webhook.
    pub id: StripeId,
    /// The one-use test checkout page.
    pub url: String,
}

impl std::fmt::Debug for IntroLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntroLink")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl IntroLink {
    /// Checks that the URL and id are for a Stripe sandbox Payment Link.
    pub fn parse(id: StripeId, url: String) -> Result<Self, IntroError> {
        let valid = |text: &str| {
            (1..=128).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric())
        };
        if !id.as_str().strip_prefix("plink_").is_some_and(valid)
            || !url
                .strip_prefix("https://buy.stripe.com/test_")
                .is_some_and(valid)
        {
            return Err(IntroError::Link);
        }
        Ok(Self { id, url })
    }
}

/// The account's current offer, or the contract started by a claimed offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntroState {
    /// No offer for this account's bound user.
    None,
    /// An offer still available to buy.
    Available(IntroLink),
    /// The offer has been paid for. No second link can be issued.
    Claimed {
        /// The end of the minimum term.
        end: UnixTime,
        /// The state of a request not to renew at the end of the term.
        stop: IntroStop,
    },
}

/// A term's renewal instruction, from the customer's request and Stripe's reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntroStop {
    /// No request; the subscription will continue unless it ends for another reason.
    Renewing,
    /// The request is recorded; the operator must arrange it in Stripe.
    Pending,
    /// Stripe has confirmed that the schedule will cancel at the term boundary.
    Confirmed,
    /// The schedule and subscription have ended, with cancellation.
    Ended,
    /// Stripe closed or released the schedule without confirming the requested stop.
    Unavailable,
}

/// A request that needs an operator to set `end_behavior=cancel` in Stripe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroRequest {
    /// First order for the term.
    pub order: OrderId,
    /// The Stripe customer returned by the paid checkout.
    pub customer: StripeId,
    /// The schedule to update, if it has been created.
    pub schedule: Option<StripeId>,
    /// Exact time at which the subscription must end.
    pub end: UnixTime,
}

/// A verified first payment ready for the operator to create its schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroStart {
    /// The paid order; copy to the schedule's `moorlight_intro_order` metadata.
    pub order: OrderId,
    /// Stripe customer to bill.
    pub customer: StripeId,
    /// The already paid intent; use its saved payment method, without reading card details.
    pub payment: StripeId,
    /// First charge of £13.50.
    pub start: UnixTime,
    /// The end of the 12-period term.
    pub end: UnixTime,
    /// The customer already asked to stop at the end of the term.
    pub stop: bool,
}

/// Why an introductory offer or a stop request could not be recorded.
#[derive(Debug, thiserror::Error)]
pub enum IntroError {
    /// Offers in this build are enabled only in a Stripe test database.
    #[error("introductory offers require a Stripe test-mode database")]
    Mode,
    /// The link is not a test Payment Link.
    #[error("give a Stripe sandbox Payment Link id and https://buy.stripe.com/test_ URL")]
    Link,
    /// The account has no matching Mooring binding.
    #[error("that account is not bound to the Mooring user named")]
    Binding,
    /// The user already paid for a plan or has an offer.
    #[error("that Mooring user is not new, or already has an introductory offer")]
    Used,
    /// The request has no contract to apply to.
    #[error("no introductory contract for that account")]
    NoContract,
    /// The term has already finished.
    #[error("the minimum term has ended")]
    TooLate,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for IntroError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

impl Store {
    /// Issue one test offer after the operator verifies that `user` is new in Mooring.
    pub fn issue_introductory_offer(
        &mut self,
        code: &AccountCode,
        user: &MooringUserId,
        link: &IntroLink,
    ) -> Result<(), IntroError> {
        let link = IntroLink::parse(link.id.clone(), link.url.clone())?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if mode(&tx)? != Some(false) {
            return Err(IntroError::Mode);
        }
        let bound: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts a JOIN bindings b ON b.account_id = a.id
                WHERE a.code = ?1 AND b.mooring_user = ?2)",
            params![code.to_string(), user.as_str()],
            |row| row.get(0),
        )?;
        if !bound {
            return Err(IntroError::Binding);
        }
        let occupied: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM introductory_offers WHERE mooring_user = ?1 OR link_id = ?2
                    OR link_url = ?3) OR EXISTS (SELECT 1 FROM orders WHERE mooring_user = ?1
                    AND plan_kind IN ('month', 'month_auto') AND paid_at IS NOT NULL)",
            params![user.as_str(), link.id.as_str(), link.url], |row| row.get(0),
        )?;
        if occupied {
            return Err(IntroError::Used);
        }
        tx.execute(
            "INSERT INTO introductory_offers (mooring_user, link_url, link_id) VALUES (?1, ?2, ?3)",
            params![user.as_str(), link.url, link.id.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// What the signed-in account is entitled to see about the offer.
    pub fn introductory_offer(&self, account: AccountId) -> Result<IntroState, StoreError> {
        intro_state(&self.conn, account)
    }

    /// Record an account owner's request. Only Stripe can confirm it.
    pub fn request_introductory_stop(
        &mut self,
        account: AccountId,
        now: UnixTime,
    ) -> Result<(), IntroError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let IntroState::Claimed { end, stop } = intro_state(&tx, account)? else {
            return Err(IntroError::NoContract);
        };
        if stop == IntroStop::Renewing && now >= end {
            return Err(IntroError::TooLate);
        }
        if stop == IntroStop::Renewing {
            tx.execute(
                "UPDATE introductory_offers SET requested_at = COALESCE(requested_at, ?2)
                 WHERE order_id IN (SELECT id FROM orders WHERE account_id = ?1)
                 AND mooring_user IN (SELECT mooring_user FROM bindings WHERE account_id = ?1)",
                params![account.0, time_to_sql(now)],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Unconfirmed requests, in the order customers made them.
    pub fn introductory_requests(&self) -> Result<Vec<IntroRequest>, StoreError> {
        let rows = self
            .conn
            .prepare(
                "SELECT order_id, customer, schedule, paid_at FROM introductory_offers
             WHERE requested_at IS NOT NULL AND order_id IS NOT NULL AND customer IS NOT NULL
               AND paid_at IS NOT NULL
               AND COALESCE(schedule_ends, 0) = 0 AND COALESCE(schedule_state, 'open') = 'open'
             ORDER BY requested_at, order_id",
            )?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(order, customer, schedule, paid)| {
                Ok(IntroRequest {
                    order: order_from_sql(&order)?,
                    customer: stripe_id_from_sql(&customer)?,
                    schedule: schedule.as_deref().map(stripe_id_from_sql).transpose()?,
                    end: time_from_sql(
                        term_boundary(paid, INTRO_PERIODS)?,
                        "introductory term end",
                    )?,
                })
            })
            .collect()
    }

    /// First payments for which a correctly configured Stripe schedule is still missing.
    pub fn introductory_starts(&self) -> Result<Vec<IntroStart>, StoreError> {
        let rows = self
            .conn
            .prepare(
                "SELECT o.id, f.customer, f.paid_intent, f.paid_at, f.requested_at IS NOT NULL
             FROM introductory_offers f JOIN orders o ON o.id = f.order_id
             JOIN stripe_payments p ON p.payment_intent = f.paid_intent
             JOIN bindings b ON b.account_id = o.account_id AND b.mooring_user = f.mooring_user
             WHERE f.schedule IS NULL AND f.claimed_at IS NOT NULL AND f.paid_at IS NOT NULL
             AND o.state = 'paid' AND p.card = 'clear' AND p.refunded_in_full = 0
             AND p.disputed_amount IS NULL ORDER BY f.paid_at, o.id",
            )?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, bool>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(order, customer, payment, paid, stop)| {
                Ok(IntroStart {
                    order: order_from_sql(&order)?,
                    customer: stripe_id_from_sql(&customer)?,
                    payment: stripe_id_from_sql(&payment)?,
                    start: time_from_sql(term_boundary(paid, 1)?, "introductory first renewal")?,
                    end: time_from_sql(
                        term_boundary(paid, INTRO_PERIODS)?,
                        "introductory term end",
                    )?,
                    stop,
                })
            })
            .collect()
    }
}

/// Whether a paid intro is waiting for its recurring subscription to start.
pub(crate) fn recurring_waits(tx: &Transaction<'_>, user: &str) -> Result<bool, StoreError> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM introductory_offers f JOIN orders o ON o.id = f.order_id
             LEFT JOIN stripe_payments p ON p.payment_intent = o.payment_intent
         WHERE f.mooring_user = ?1 AND f.claimed_at IS NOT NULL AND o.subscription IS NULL
           AND COALESCE(f.schedule_state, 'open') = 'open'
           AND (f.schedule IS NOT NULL OR (COALESCE(p.refunded_in_full, 0) = 0 AND p.disputed_amount IS NULL)))",
        [user], |r| r.get(0),
    )?)
}

pub(crate) fn intro_state(
    conn: &rusqlite::Connection,
    account: AccountId,
) -> Result<IntroState, StoreError> {
    let found = conn
        .query_row(
            "SELECT f.link_id, f.link_url, f.claimed_at, f.term_end, f.schedule_state,
                COALESCE(f.schedule_ends, 0), f.requested_at IS NOT NULL,
                EXISTS (SELECT 1 FROM orders p WHERE p.mooring_user = f.mooring_user
                   AND p.plan_kind IN ('month','month_auto') AND p.paid_at IS NOT NULL), f.paid_at
         FROM bindings b JOIN introductory_offers f ON f.mooring_user = b.mooring_user
         LEFT JOIN orders o ON o.id = f.order_id
         WHERE b.account_id = ?1 AND (f.claimed_at IS NULL OR o.account_id = ?1)",
            [account.0],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, bool>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                ))
            },
        )
        .optional()?;
    let Some((link, url, claimed, end, state, stops, requested, has_paid, paid)) = found else {
        return Ok(IntroState::None);
    };
    if claimed.is_some() {
        let Some(paid) = paid else {
            return Ok(IntroState::None);
        };
        let end = end.map_or_else(|| term_boundary(paid, INTRO_PERIODS), Ok)?;
        let stop = match (state.as_deref(), stops, requested) {
            (Some("ended"), true, _) => IntroStop::Ended,
            (Some("open"), true, _) => IntroStop::Confirmed,
            (Some("ended"), false, true) => IntroStop::Unavailable,
            (_, _, true) => IntroStop::Pending,
            _ => IntroStop::Renewing,
        };
        return Ok(IntroState::Claimed {
            end: time_from_sql(end, "introductory term end")?,
            stop,
        });
    }
    if has_paid {
        return Ok(IntroState::None);
    }
    let page = IntroLink::parse(stripe_id_from_sql(&link)?, url)
        .map_err(|_| crate::rows::corrupt("introductory link", "invalid stored link".to_owned()))?;
    Ok(IntroState::Available(page))
}

fn term_boundary(start: i64, periods: u64) -> Result<i64, StoreError> {
    let seconds = crate::rows::count_to_sql(periods * INTRO_PERIOD_SECS, "introductory duration")?;
    start.checked_add(seconds).ok_or(StoreError::OutOfRange {
        what: "introductory term end",
    })
}

/// Whether this first order holds a private link, independent of the current catalogue.
pub(crate) fn is_private(tx: &Transaction<'_>, link_id: &str) -> Result<bool, StoreError> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM introductory_offers WHERE link_id = ?1)",
        [link_id],
        |r| r.get(0),
    )?)
}

/// A private checkout keeps the configured plan's terms and substitutes its own link.
pub(crate) fn intro_matches_terms(terms: &OrderTerms, on_sale: &[OrderTerms]) -> bool {
    on_sale.iter().any(|plan| {
        plan.renewal_amount.is_some()
            && *plan
                == OrderTerms {
                    payment_link_id: plan.payment_link_id.clone(),
                    ..terms.clone()
                }
    })
}

/// Atomically consume an issued offer. A stale or forwarded link cannot pay another order.
pub(crate) fn claim(
    tx: &Transaction<'_>,
    order: &OrderId,
    checkout: &CheckoutSession,
    made: UnixTime,
) -> Result<bool, StoreError> {
    let Some(customer) = &checkout.customer else {
        return Ok(false);
    };
    let Some(link) = &checkout.payment_link else {
        return Ok(false);
    };
    let Some(payment) = &checkout.payment_intent else {
        return Ok(false);
    };
    let changed = tx.execute(
        "UPDATE introductory_offers AS f SET claimed_at = ?2, customer = ?3, paid_intent = ?5
         WHERE order_id = ?1 AND link_id = ?4 AND claimed_at IS NULL
           AND EXISTS (SELECT 1 FROM orders o JOIN bindings b ON b.account_id = o.account_id
                       WHERE o.id = ?1 AND b.mooring_user = f.mooring_user AND o.mooring_user = f.mooring_user)
           AND NOT EXISTS (SELECT 1 FROM orders o WHERE o.mooring_user = f.mooring_user
                     AND o.id <> ?1 AND o.paid_at IS NOT NULL AND o.plan_kind IN ('month','month_auto'))",
        params![order.as_str(), time_to_sql(made), customer.as_str(), link.as_str(), payment.as_str()],
    )?;
    Ok(changed == 1)
}

pub(crate) fn remember_payment(
    tx: &Transaction<'_>,
    payment: &IntroPayment,
    made: UnixTime,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO intro_paid_intents (payment, customer, paid_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (payment) DO NOTHING",
        params![
            payment.id.as_str(),
            payment.customer.as_str(),
            time_to_sql(made)
        ],
    )?;
    Ok(())
}

/// Tie the customer's consented payment to the order before displaying the term dates.
pub(crate) fn match_paid_at(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE introductory_offers AS f SET paid_at = p.paid_at
         FROM intro_paid_intents p
         WHERE f.paid_intent = p.payment AND f.customer = p.customer
           AND f.paid_at IS NULL AND f.claimed_at IS NOT NULL",
        [],
    )?;
    Ok(())
}

/// Observe an operator-created schedule. Both metadata and Stripe's customer
/// must match the paid introductory order, and the dates must be exact.
pub(crate) fn observe_schedule(
    tx: &Transaction<'_>,
    change: &ScheduleChange,
    made: UnixTime,
) -> Result<(), StoreError> {
    let (Some(order), Some(starts), Some(ends)) =
        (change.introductory_order, change.start, change.end)
    else {
        return Ok(());
    };
    tx.execute(
        "INSERT INTO intro_schedule_notices (order_id, schedule, customer, subscription, starts, ends,
            noticed, state, stops)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
           WHERE EXISTS (SELECT 1 FROM introductory_offers WHERE order_id = ?1
                      OR (schedule = ?2 AND order_id IS NULL)) AND EXISTS (SELECT 1 FROM orders WHERE id = ?1)
         ON CONFLICT (order_id, schedule) DO UPDATE SET
            subscription = COALESCE(excluded.subscription, intro_schedule_notices.subscription),
            starts = excluded.starts, ends = excluded.ends, noticed = excluded.noticed,
            state = excluded.state, stops = excluded.stops
         WHERE intro_schedule_notices.customer = excluded.customer AND intro_schedule_notices.state <> 'ended'
           AND (intro_schedule_notices.noticed < excluded.noticed OR
              (intro_schedule_notices.noticed = excluded.noticed AND
                (excluded.state = 'ended' OR excluded.stops >= intro_schedule_notices.stops)))",
        params![order.as_str(), change.id.as_str(), change.customer.as_str(),
            change.subscription.as_ref().map(StripeId::as_str), time_to_sql(starts), time_to_sql(ends),
            time_to_sql(made), if change.ended { "ended" } else { "open" }, change.cancels],
    )?;
    // A canceled schedule can outlive an account that the operator banned.
    if change.ended {
        tx.execute(
            "UPDATE introductory_offers SET schedule_state = 'ended', schedule_at = ?2,
                     schedule_ends = ?3 WHERE schedule = ?1 AND order_id IS NULL",
            params![change.id.as_str(), time_to_sql(made), change.cancels],
        )?;
    }
    Ok(())
}

/// Match the schedule even if Stripe delivered the checkout and paid intent after it.
/// An unknown second schedule is never adopted over an already matched one.
pub(crate) fn match_schedules(tx: &Transaction<'_>) -> Result<Vec<StripeId>, StoreError> {
    let period = crate::rows::count_to_sql(INTRO_PERIOD_SECS, "introductory period")?;
    let term = crate::rows::count_to_sql(INTRO_PERIODS * INTRO_PERIOD_SECS, "introductory term")?;
    let matched = tx
        .prepare(
            "SELECT n.order_id, n.schedule, n.subscription, n.ends, n.state, n.stops, n.noticed
         FROM intro_schedule_notices n JOIN introductory_offers f ON f.order_id = n.order_id
         JOIN orders o ON o.id = n.order_id WHERE (o.state = 'paid' OR f.schedule = n.schedule)
           AND n.customer = f.customer AND n.starts = f.paid_at + ?1 AND n.ends = f.paid_at + ?2
           AND (o.subscription IS NULL OR n.subscription IS NULL OR o.subscription = n.subscription)
           AND (f.schedule = n.schedule OR (f.schedule IS NULL AND NOT EXISTS (
             SELECT 1 FROM intro_schedule_notices other WHERE other.order_id = n.order_id
               AND other.schedule <> n.schedule AND other.customer = n.customer
               AND other.starts = n.starts AND other.ends = n.ends)))
           AND (f.schedule_at IS NULL OR f.schedule_at < n.noticed OR
               f.schedule_state <> n.state OR f.schedule_ends <> n.stops OR
               (o.subscription IS NULL AND n.subscription IS NOT NULL))",
        )?
        .query_map(params![period, term], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut attached = Vec::new();
    for (order, schedule, subscription, end, state, stop, noticed) in matched {
        tx.execute(
            "UPDATE introductory_offers SET schedule=?2, term_end=?3, schedule_state=?4,
                    schedule_ends=?5, schedule_at=?6 WHERE order_id=?1",
            params![order, schedule, end, state, stop, noticed],
        )?;
        if let Some(sub) = subscription
            && tx.execute(
                "UPDATE orders SET subscription=?2 WHERE id=?1 AND subscription IS NULL",
                params![order, sub],
            )? == 1
        {
            attached.push(stripe_id_from_sql(&sub)?);
        }
    }
    Ok(attached)
}
