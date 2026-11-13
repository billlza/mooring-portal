//! Bringing together what is known of each order's payment.
//!
//! Run after every event and after every act of the operator that can let
//! an order through, in the transaction that made the change. Each step
//! reads only what is recorded, so the outcome is the same in whatever
//! order Stripe's events arrived:
//!
//! 1. an order paid by invoice learns its payment once the invoice's
//!    payment is known;
//! 2. a paid order whose card is a banned account's, or could not be
//!    checked, waits for the operator, unless it is in the journal
//!    already or the operator has passed its card;
//! 3. a paid order whose card has passed, by the check or by the
//!    operator, is entered in the journal for Mooring to grant, if its
//!    account is still bound to the Mooring user it names; if not, it
//!    waits for the operator. An order refunded in full or disputed
//!    before this is never entered;
//! 4. a granted order's refunds and dispute are entered after it;
//! 5. a payment that is no order's, once refunded in full, needs no one's
//!    attention unless its subscription still needs cancelling;
//! 6. what Stripe said long ago of payments that never became an order's
//!    is forgotten.

use portal_core::time::UnixTime;
use rusqlite::{Transaction, params};

use crate::StoreError;
use crate::orders::STRAY_NEEDS_CANCELLATION;
use crate::rows::time_to_sql;

/// How long the portal remembers an event's id, and what Stripe said of a
/// payment, an invoice or a subscription that no order and no stray
/// payment names: 45 days. Stripe redelivers for three days.
pub(crate) const FORGET_AFTER_SECS: i64 = 45 * 86_400;

/// Whether `orders o` is in the journal for Mooring to grant.
const GRANTED: &str =
    "EXISTS (SELECT 1 FROM entitlements e WHERE e.order_id = o.id AND e.kind = 'grant')";
/// Whether `orders o`, with its `stripe_payments p`, is fit to be granted:
/// paid, not refunded in full nor disputed, and paid with a card that has
/// passed the check or that the operator has passed.
const PASSED: &str = "o.state = 'paid'
     AND (o.card_passed = 1 OR p.card = 'clear')
     AND COALESCE(p.refunded_in_full, 0) = 0
     AND p.disputed_amount IS NULL";
/// Whether the account of `orders o` is bound to the Mooring user the
/// order names.
const BOUND: &str = "EXISTS (SELECT 1 FROM bindings b
     WHERE b.account_id = o.account_id AND b.mooring_user = o.mooring_user)";

/// Brings the orders up to what is recorded, at `now`.
pub(crate) fn settle(tx: &Transaction<'_>, now: UnixTime) -> Result<(), StoreError> {
    let now_sql = time_to_sql(now);
    // 1. Were two invoices ever said to be paid by one payment, the
    // second order would break the rule that a payment is one order's; it
    // is left without its payment, where the operator's list shows it
    // waiting, rather than fail every event from then on.
    for table in ["orders", "stray_payments"] {
        tx.execute(
            &format!(
                "UPDATE OR IGNORE {table} SET payment_intent =
                     (SELECT i.payment_intent FROM stripe_invoice_payments i
                      WHERE i.invoice = {table}.invoice)
                 WHERE payment_intent IS NULL AND invoice IN
                     (SELECT invoice FROM stripe_invoice_payments)"
            ),
            [],
        )?;
    }
    // 2. What is in the journal is Mooring's already: nothing said of its
    // card afterwards takes it back to the operator.
    tx.execute(
        &format!(
            "UPDATE orders SET state = 'review', review_reason = v.reason
             FROM (SELECT o.id AS id,
                          CASE p.card WHEN 'banned' THEN 'card_banned' ELSE 'card_unchecked' END
                              AS reason
                   FROM orders o JOIN stripe_payments p ON p.payment_intent = o.payment_intent
                   WHERE o.state = 'paid' AND o.card_passed = 0
                     AND p.card IN ('banned', 'unchecked') AND NOT {GRANTED}) AS v
             WHERE orders.id = v.id"
        ),
        [],
    )?;
    // 3.
    tx.execute(
        &format!(
            "UPDATE orders SET state = 'review', review_reason = 'binding'
             WHERE id IN (SELECT o.id FROM orders o
                 LEFT JOIN stripe_payments p ON p.payment_intent = o.payment_intent
                 WHERE {PASSED} AND NOT {GRANTED} AND NOT {BOUND})"
        ),
        [],
    )?;
    // The statement before has sent every unbound order to review, so
    // the binding is checked again only so that this statement grants to
    // no one but the user the order names, whatever is done to that one.
    tx.execute(
        &format!(
            "INSERT INTO entitlements (order_id, kind, amount, recorded_at)
             SELECT o.id, 'grant', NULL, ?1 FROM orders o
             LEFT JOIN stripe_payments p ON p.payment_intent = o.payment_intent
             WHERE {PASSED} AND NOT {GRANTED} AND {BOUND}
             ORDER BY o.paid_at, o.id"
        ),
        [now_sql],
    )?;
    // 4. A refund is entered each time more has gone back than the journal
    // says, or all of it has and the journal does not say so; a dispute
    // once.
    tx.execute(
        &format!(
            "INSERT INTO entitlements (order_id, kind, amount, in_full, recorded_at)
             SELECT o.id, 'refund', p.refunded_amount, p.refunded_in_full, ?1 FROM orders o
             JOIN stripe_payments p ON p.payment_intent = o.payment_intent
             WHERE {GRANTED}
               AND (p.refunded_amount > COALESCE(
                        (SELECT max(e.amount) FROM entitlements e
                         WHERE e.order_id = o.id AND e.kind = 'refund'), 0)
                    OR (p.refunded_in_full = 1 AND NOT EXISTS
                        (SELECT 1 FROM entitlements e
                         WHERE e.order_id = o.id AND e.kind = 'refund' AND e.in_full = 1)))
             ORDER BY o.paid_at, o.id"
        ),
        [now_sql],
    )?;
    tx.execute(
        &format!(
            "INSERT INTO entitlements (order_id, kind, amount, recorded_at)
             SELECT o.id, 'dispute', p.disputed_amount, ?1 FROM orders o
             JOIN stripe_payments p ON p.payment_intent = o.payment_intent
             WHERE {GRANTED}
               AND p.disputed_amount IS NOT NULL
               AND NOT EXISTS
                   (SELECT 1 FROM entitlements e WHERE e.order_id = o.id AND e.kind = 'dispute')
             ORDER BY o.paid_at, o.id"
        ),
        [now_sql],
    )?;
    // 5.
    tx.execute(
        &format!(
            "DELETE FROM stray_payments AS sp
             WHERE sp.payment_intent IN
                 (SELECT payment_intent FROM stripe_payments WHERE refunded_in_full = 1)
               AND NOT ({STRAY_NEEDS_CANCELLATION})"
        ),
        [],
    )?;
    // 6.
    let before = now_sql - FORGET_AFTER_SECS;
    tx.execute("DELETE FROM stripe_events WHERE received_at < ?1", [before])?;
    for (table, key) in [
        ("stripe_payments", "payment_intent"),
        ("stripe_invoice_payments", "invoice"),
        ("stripe_subscriptions", "subscription"),
    ] {
        tx.execute(
            &format!(
                "DELETE FROM {table} WHERE noticed_at < ?1
                   AND {key} NOT IN (SELECT {key} FROM orders WHERE {key} IS NOT NULL)
                   AND {key} NOT IN (SELECT {key} FROM stray_payments WHERE {key} IS NOT NULL)"
            ),
            params![before],
        )?;
    }
    Ok(())
}
