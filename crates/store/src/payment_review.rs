//! What the operator sees of payments, and the decisions that are the
//! operator's alone.
//!
//! The portal grants a payment only when everything about it is as its
//! order asked. Whatever is not waits here: an order in review, which the
//! operator settles by refunding it at Stripe or by approving it; and a
//! payment that is no order's, which the operator refunds at Stripe or
//! dismisses. The portal holds no key that could refund, so it only
//! learns of a refund when Stripe tells it.
//!
//! Approving waives the one thing the operator was shown, and nothing
//! else: an order let through for its amount is still judged by its card.

use portal_core::account::AccountCode;
use portal_core::payment::stripe::{Renewal, StripeId};
use portal_core::payment::{CardVerdict, Currency, OrderId, PlanId, ReviewReason};
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, Row, TransactionBehavior, params};

use crate::orders::{
    ORDER_COLUMNS, ORDER_TABLES, ORDER_WIDTH, Order, OrderRow, Progress, RENEWAL_TABLES, RENEWS,
    order_row, renewal_from_sql,
};
use crate::rows::{
    code_from_sql, corrupt, count_from_sql, count_to_sql, stripe_id_from_sql, time_from_sql,
};
use crate::settle::settle;
use crate::stripe_events::StrayReason;
use crate::{Store, StoreError};

/// An order as the operator sees it: with its account and with the ids
/// Stripe's Dashboard finds its payment by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorOrder {
    /// The order.
    pub order: Order,
    /// Its account.
    pub account: AccountCode,
    /// What the check of its card found, once known.
    pub card: Option<CardVerdict>,
    /// The checkout that paid it.
    pub session: Option<StripeId>,
    /// Its payment.
    pub payment_intent: Option<StripeId>,
    /// The invoice that billed it, for a subscription.
    pub invoice: Option<StripeId>,
    /// Its subscription.
    pub subscription: Option<StripeId>,
}

/// A payment that is no order's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrayPayment {
    /// The number the operator dismisses it by.
    pub id: u64,
    /// When the portal learned of it.
    pub noticed_at: UnixTime,
    /// Why it is no order's.
    pub reason: StrayReason,
    /// The checkout.
    pub session: Option<StripeId>,
    /// The invoice.
    pub invoice: Option<StripeId>,
    /// The payment.
    pub payment_intent: Option<StripeId>,
    /// The subscription it began or renewed.
    pub subscription: Option<StripeId>,
    /// What was paid.
    pub amount: Option<u64>,
    /// In which currency, if one the portal prices in.
    pub currency: Option<Currency>,
    /// What the check of its card found, once known.
    pub card: Option<CardVerdict>,
    /// How much of it has been refunded.
    pub refunded: u64,
}

/// A subscription that Stripe may charge again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRenewal {
    /// The subscription.
    pub subscription: StripeId,
    /// The account whose order began it.
    pub account: AccountCode,
    /// The plan it renews.
    pub plan: PlanId,
    /// Whether it will be charged again: never [`Renewal::Ended`].
    pub state: Renewal,
}

/// A refund or a dispute of a payment that no order and no stray payment
/// names: after an account is deleted, what Stripe says of its payments
/// arrives like this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmatchedPayment {
    /// The payment.
    pub payment_intent: StripeId,
    /// How much of it has been refunded.
    pub refunded: u64,
    /// How much of it is disputed.
    pub disputed: Option<u64>,
}

/// What approving an order did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Approved {
    /// The order's account.
    pub account: AccountCode,
    /// What was waived: why the order was in review, or `None` if it was
    /// paid and waiting for word of its card, which is passed unchecked.
    pub waived: Option<ReviewReason>,
    /// Where the order stands now: in the journal, or waiting for
    /// something the operator was not shown.
    pub standing: Progress,
}

/// Why an order was not approved.
#[derive(Debug, thiserror::Error)]
pub enum ApproveError {
    /// No order has the id.
    #[error("no order has the id {0}")]
    Unknown(OrderId),
    /// The order has not been paid.
    #[error("order {0} has not been paid; nothing changed")]
    Unpaid(OrderId),
    /// The order is in the journal already.
    #[error("order {0} is already in the journal for Mooring; nothing changed")]
    Granted(OrderId),
    /// Its payment has been refunded in full, or is disputed.
    #[error("order {0} was refunded in full or is disputed, so it is not to be granted")]
    Returned(OrderId),
    /// It was paid with a card of a banned account.
    #[error(
        "order {0} was paid with a card of a banned account: such a payment is refunded at \
         Stripe, never granted"
    )]
    CardBanned(OrderId),
    /// Its account is not bound to the Mooring user it names.
    #[error(
        "the account of order {0} is not bound to the Mooring user the order names; bind it \
         first"
    )]
    Unbound(OrderId),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ApproveError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// What follows [`ORDER_COLUMNS`] in an operator's query.
const OPERATOR_COLUMNS: &str =
    "a.code, p.card, o.session, o.payment_intent, o.invoice, o.subscription";

/// An operator's order as SQLite gives it.
struct OperatorRow {
    order: OrderRow,
    account: String,
    card: Option<String>,
    ids: [Option<String>; 4],
}

fn operator_row(row: &Row<'_>) -> rusqlite::Result<OperatorRow> {
    Ok(OperatorRow {
        order: order_row(row)?,
        account: row.get(ORDER_WIDTH)?,
        card: row.get(ORDER_WIDTH + 1)?,
        ids: [
            row.get(ORDER_WIDTH + 2)?,
            row.get(ORDER_WIDTH + 3)?,
            row.get(ORDER_WIDTH + 4)?,
            row.get(ORDER_WIDTH + 5)?,
        ],
    })
}

fn stripe_id(text: Option<String>) -> Result<Option<StripeId>, StoreError> {
    text.as_deref().map(stripe_id_from_sql).transpose()
}

/// A card's verdict read back.
fn card_verdict(text: Option<String>) -> Result<Option<CardVerdict>, StoreError> {
    text.map(|text| {
        CardVerdict::parse(&text).ok_or_else(|| corrupt("card verdict", format!("{text:?}")))
    })
    .transpose()
}

impl OperatorRow {
    fn checked(self) -> Result<OperatorOrder, StoreError> {
        let [session, payment_intent, invoice, subscription] = self.ids;
        Ok(OperatorOrder {
            order: self.order.checked()?,
            account: code_from_sql(&self.account)?,
            card: card_verdict(self.card)?,
            session: stripe_id(session)?,
            payment_intent: stripe_id(payment_intent)?,
            invoice: stripe_id(invoice)?,
            subscription: stripe_id(subscription)?,
        })
    }
}

impl Store {
    /// The operator's orders that `filter` (a condition on `orders o` and
    /// its `stripe_payments p`) keeps, in `order`.
    fn operator_orders(
        &self,
        filter: &str,
        order: &str,
        limit: i64,
    ) -> Result<Vec<OperatorOrder>, StoreError> {
        let rows: Vec<OperatorRow> = self
            .conn
            .prepare(&format!(
                "SELECT {ORDER_COLUMNS}, {OPERATOR_COLUMNS} FROM {ORDER_TABLES}
                 JOIN accounts a ON a.id = o.account_id
                 WHERE {filter} ORDER BY {order} LIMIT ?1"
            ))?
            .query_map([limit], operator_row)?
            .collect::<Result<_, _>>()?;
        rows.into_iter().map(OperatorRow::checked).collect()
    }

    /// Every order that waits: for the operator (in review), or for its
    /// card to be checked. Oldest first. One whose payment was refunded
    /// in full waits for nothing.
    pub fn orders_waiting(&self) -> Result<Vec<OperatorOrder>, StoreError> {
        self.operator_orders(
            "(o.state = 'review' OR (o.state = 'paid' AND NOT EXISTS
                 (SELECT 1 FROM entitlements e WHERE e.order_id = o.id AND e.kind = 'grant')))
             AND COALESCE(p.refunded_in_full, 0) = 0",
            "o.created_at, o.id",
            -1,
        )
    }

    /// The `limit` newest orders, newest first.
    pub fn recent_orders(&self, limit: u32) -> Result<Vec<OperatorOrder>, StoreError> {
        self.operator_orders("1", "o.created_at DESC, o.id", i64::from(limit))
    }

    /// Every payment that is no order's, oldest first.
    pub fn stray_payments(&self) -> Result<Vec<StrayPayment>, StoreError> {
        type StrayRow = (
            i64,
            i64,
            String,
            [Option<String>; 4],
            Option<i64>,
            Option<String>,
            Option<String>,
            i64,
        );
        let rows: Vec<StrayRow> = self
            .conn
            .prepare(
                "SELECT s.id, s.noticed_at, s.reason, s.session, s.invoice, s.payment_intent,
                        s.subscription, s.amount, s.currency, p.card,
                        COALESCE(p.refunded_amount, 0)
                 FROM stray_payments s
                 LEFT JOIN stripe_payments p ON p.payment_intent = s.payment_intent
                 ORDER BY s.id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    [row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?],
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(
                |(id, noticed_at, reason, ids, amount, currency, card, refunded)| {
                    let [session, invoice, payment_intent, subscription] = ids;
                    Ok(StrayPayment {
                        id: count_from_sql(id, "stray payment's number")?,
                        noticed_at: time_from_sql(noticed_at, "stray payment's time")?,
                        reason: StrayReason::parse(&reason).ok_or_else(|| {
                            corrupt("stray payment's reason", format!("{reason:?}"))
                        })?,
                        session: stripe_id(session)?,
                        invoice: stripe_id(invoice)?,
                        payment_intent: stripe_id(payment_intent)?,
                        subscription: stripe_id(subscription)?,
                        amount: amount
                            .map(|amount| count_from_sql(amount, "stray payment's amount"))
                            .transpose()?,
                        currency: currency
                            .map(|text| {
                                Currency::parse(&text).ok_or_else(|| {
                                    corrupt("stray payment's currency", format!("{text:?}"))
                                })
                            })
                            .transpose()?,
                        card: card_verdict(card)?,
                        refunded: count_from_sql(refunded, "refunded amount")?,
                    })
                },
            )
            .collect()
    }

    /// Every subscription that Stripe may charge again, by account.
    pub fn open_renewals(&self) -> Result<Vec<OpenRenewal>, StoreError> {
        let rows: Vec<(String, String, String, String)> = self
            .conn
            .prepare(&format!(
                "SELECT o.subscription, a.code, o.plan, COALESCE(s.state, 'active')
                 FROM {RENEWAL_TABLES} JOIN accounts a ON a.id = o.account_id
                 WHERE o.renewal_of IS NULL AND {RENEWS}
                 ORDER BY a.created_at, a.id, o.created_at, o.id"
            ))?
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(subscription, account, plan, state)| {
                Ok(OpenRenewal {
                    subscription: stripe_id_from_sql(&subscription)?,
                    account: code_from_sql(&account)?,
                    plan: PlanId::parse(&plan)
                        .ok_or_else(|| corrupt("order's plan", format!("{plan:?}")))?,
                    state: renewal_from_sql(&state)?,
                })
            })
            .collect()
    }

    /// Every refund and dispute of a payment that no order and no stray
    /// payment names.
    pub fn unmatched_payments(&self) -> Result<Vec<UnmatchedPayment>, StoreError> {
        let rows: Vec<(String, i64, Option<i64>)> = self
            .conn
            .prepare(
                "SELECT p.payment_intent, p.refunded_amount, p.disputed_amount
                 FROM stripe_payments p
                 WHERE (p.refunded_amount > 0 OR p.disputed_amount IS NOT NULL)
                   AND NOT EXISTS
                       (SELECT 1 FROM orders o WHERE o.payment_intent = p.payment_intent)
                   AND NOT EXISTS
                       (SELECT 1 FROM stray_payments s WHERE s.payment_intent = p.payment_intent)
                 ORDER BY p.noticed_at, p.payment_intent",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(payment_intent, refunded, disputed)| {
                Ok(UnmatchedPayment {
                    payment_intent: stripe_id_from_sql(&payment_intent)?,
                    refunded: count_from_sql(refunded, "refunded amount")?,
                    disputed: disputed
                        .map(|amount| count_from_sql(amount, "disputed amount"))
                        .transpose()?,
                })
            })
            .collect()
    }

    /// Lets the order `id` through the one thing it waits for, which the
    /// operator has looked at; nothing else about it is waived. An order
    /// in review because its card could not be checked, or paid and still
    /// waiting for word of its card, has its card passed. One in review
    /// for anything else goes back to being paid, to be judged by its
    /// card like any other. One paid with a banned account's card is not
    /// approved at all.
    pub fn approve_order(&mut self, id: &OrderId, now: UnixTime) -> Result<Approved, ApproveError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(String, String, Option<String>, [bool; 3])> = tx
            .query_row(
                "SELECT a.code, o.state, o.review_reason,
                        EXISTS (SELECT 1 FROM entitlements e
                                WHERE e.order_id = o.id AND e.kind = 'grant'),
                        COALESCE(p.refunded_in_full, 0) = 1 OR p.disputed_amount IS NOT NULL,
                        EXISTS (SELECT 1 FROM bindings b WHERE b.account_id = o.account_id
                                AND b.mooring_user = o.mooring_user)
                 FROM orders o JOIN accounts a ON a.id = o.account_id
                 LEFT JOIN stripe_payments p ON p.payment_intent = o.payment_intent
                 WHERE o.id = ?1",
                [id.as_str()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        [row.get(3)?, row.get(4)?, row.get(5)?],
                    ))
                },
            )
            .optional()?;
        let Some((account, state, reason, [granted, returned, bound])) = found else {
            return Err(ApproveError::Unknown(*id));
        };
        if state == "pending" {
            return Err(ApproveError::Unpaid(*id));
        }
        if granted {
            return Err(ApproveError::Granted(*id));
        }
        if returned {
            return Err(ApproveError::Returned(*id));
        }
        let waived = reason
            .map(|reason| {
                ReviewReason::parse(&reason)
                    .ok_or_else(|| corrupt("order's review reason", format!("{reason:?}")))
            })
            .transpose()?;
        if waived == Some(ReviewReason::CardBanned) {
            return Err(ApproveError::CardBanned(*id));
        }
        if !bound {
            return Err(ApproveError::Unbound(*id));
        }
        // Only the card's check is ever waived for good: every other
        // reason was a judgement of the checkout, made once.
        let passes_card = matches!(waived, None | Some(ReviewReason::CardUnchecked));
        tx.execute(
            "UPDATE orders SET state = 'paid', review_reason = NULL, card_passed = ?2
             WHERE id = ?1",
            params![id.as_str(), passes_card],
        )?;
        settle(&tx, now)?;
        let standing = tx
            .query_row(
                &format!("SELECT {ORDER_COLUMNS} FROM {ORDER_TABLES} WHERE o.id = ?1"),
                [id.as_str()],
                order_row,
            )?
            .checked()?
            .progress;
        tx.commit()?;
        Ok(Approved {
            account: code_from_sql(&account)?,
            waived,
            standing,
        })
    }

    /// Takes the stray payment numbered `id` off the operator's list;
    /// whether there was one.
    pub fn dismiss_stray(&mut self, id: u64) -> Result<bool, StoreError> {
        let removed = self.conn.execute(
            "DELETE FROM stray_payments WHERE id = ?1",
            params![count_to_sql(id, "stray payment's number")?],
        )?;
        Ok(removed > 0)
    }
}
