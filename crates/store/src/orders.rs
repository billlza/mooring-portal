//! Orders: what an account asked to buy, and where each stands.
//!
//! An order is placed for the account of a live session, for the Mooring
//! user that account is bound to, on its plan's terms as the catalogue
//! had them then. An account has at most one unpaid order for a plan:
//! asking again gives the same order while the terms stand, and a new one
//! in its place once they have changed, so that nobody is sent to pay a
//! price the catalogue no longer has.
//!
//! Where an order stands is read, never stored: from what Stripe has said
//! of its payment and from the journal Mooring pulls (`settle.rs`).

use portal_core::auth::TokenDigest;
use portal_core::payment::stripe::Renewal;
use portal_core::payment::{Currency, OrderId, OrderTerms, PlanId, PlanKind, ReviewReason};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{Connection, OptionalExtension as _, Row, TransactionBehavior, params};

use crate::introductory::{intro_state, recurring_waits};
use crate::rows::{
    corrupt, count_from_sql, count_to_sql, order_from_sql, time_from_sql, time_to_sql,
    user_from_sql,
};
use crate::sessions::session_account_id;
use crate::{AccountId, IntroState, Store, StoreError};

/// An order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// Its id.
    pub id: OrderId,
    /// The Mooring user it is for.
    pub user: MooringUserId,
    /// What it is for.
    pub terms: OrderTerms,
    /// When it was placed; for a renewal, when it was paid.
    pub created_at: UnixTime,
    /// When its payment was made.
    pub paid_at: Option<UnixTime>,
    /// Where it stands.
    pub progress: Progress,
    /// How much of its payment has been refunded.
    pub refunded: u64,
    /// Whether that is all of it.
    pub refunded_in_full: bool,
    /// Whether its payer disputes the payment.
    pub disputed: bool,
    /// For a subscription's renewal, the order that began the
    /// subscription.
    pub renewal_of: Option<OrderId>,
}

/// Where an order stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Not paid.
    Pending,
    /// Paid; its card is not yet checked against the ban list.
    Checking,
    /// Paid and passed: in the journal for Mooring to grant.
    Granted {
        /// Whether Mooring has fetched it.
        pulled: bool,
    },
    /// Waiting for the operator, who alone decides.
    Review(ReviewReason),
    /// Paid, but refunded in full or disputed before it was granted, so
    /// it never will be.
    Returned,
}

/// A subscription of an account that Stripe may charge again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoRenewal {
    /// The plan it renews.
    pub plan: PlanId,
    /// Whether it will be charged again: never [`Renewal::Ended`].
    pub state: Renewal,
}

/// Why an order was not placed.
#[derive(Debug, thiserror::Error)]
pub enum OrderError {
    /// No live session has the digest.
    #[error("the session has ended")]
    SignedOut,
    /// The account is not bound to a Mooring user, so there is no one to
    /// grant an order to.
    #[error("the account is not bound to a Mooring user")]
    Unbound,
    /// The plan renews by itself, and the account already has a
    /// subscription that Stripe may charge again.
    #[error("the account already has a plan that renews by itself")]
    Renewing,
    /// The bound Mooring user has no available introductory offer.
    #[error("this Mooring user does not have an available introductory offer")]
    Introductory,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for OrderError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// The columns [`order_row`] reads, of `orders o` joined to what Stripe
/// has said of its payment (`stripe_payments p`).
pub(crate) const ORDER_COLUMNS: &str =
    "o.id, o.mooring_user, o.plan, o.plan_kind, o.days, o.traffic_bytes, o.amount, o.currency,
     o.payment_link_id, o.renewal_amount, o.created_at, o.paid_at, o.state, o.review_reason,
     o.renewal_of,
     (SELECT e.seq FROM entitlements e WHERE e.order_id = o.id AND e.kind = 'grant'),
     COALESCE((SELECT pulled_through FROM pull_access WHERE id = 1), 0),
     COALESCE(p.refunded_amount, 0), COALESCE(p.refunded_in_full, 0),
     p.disputed_amount IS NOT NULL";
/// The join [`ORDER_COLUMNS`] reads from.
pub(crate) const ORDER_TABLES: &str =
    "orders o LEFT JOIN stripe_payments p ON p.payment_intent = o.payment_intent";
/// How many columns [`ORDER_COLUMNS`] names: what follows them in a query
/// starts here.
pub(crate) const ORDER_WIDTH: usize = 20;

/// Whether `orders o`, joined as [`ORDER_TABLES`], has money in the air:
/// it is paid and neither in the journal nor returned (refunded in full,
/// or disputed); or it waits for the operator and is not returned; or it
/// is in the journal with an entry Mooring has not fetched.
pub(crate) const UNSETTLED: &str = "(
     (o.state = 'paid' AND COALESCE(p.refunded_in_full, 0) = 0 AND p.disputed_amount IS NULL
          AND NOT EXISTS (SELECT 1 FROM entitlements e
                          WHERE e.order_id = o.id AND e.kind = 'grant'))
     OR (o.state = 'review' AND COALESCE(p.refunded_in_full, 0) = 0
          AND p.disputed_amount IS NULL)
     OR EXISTS (SELECT 1 FROM entitlements e WHERE e.order_id = o.id
                AND e.seq > COALESCE((SELECT pulled_through FROM pull_access WHERE id = 1), 0)))";

/// `orders o` with what Stripe has said of its subscription
/// (`stripe_subscriptions s`).
pub(crate) const RENEWAL_TABLES: &str =
    "orders o LEFT JOIN stripe_subscriptions s ON s.subscription = o.subscription";
/// Whether `orders o`, joined as [`RENEWAL_TABLES`], is of a subscription
/// that Stripe may charge again. Stripe tells of a subscription only when
/// it changes, so one it has said nothing of is running.
pub(crate) const RENEWS: &str =
    "o.subscription IS NOT NULL AND COALESCE(s.state, 'active') <> 'ended'";

/// Whether a stray payment (`sp`) names a subscription Stripe may
/// charge again and no order began. An extra invoice for an order's
/// subscription is stray, but does not make the subscription stray.
pub(crate) const STRAY_NEEDS_CANCELLATION: &str = "sp.subscription IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM orders o
                     WHERE o.subscription = sp.subscription AND o.renewal_of IS NULL)
     AND NOT EXISTS (SELECT 1 FROM stripe_subscriptions s
                     WHERE s.subscription = sp.subscription AND s.state = 'ended')";

/// An order's row as SQLite gives it.
pub(crate) struct OrderRow {
    id: String,
    user: String,
    terms: TermsRow,
    created_at: i64,
    paid_at: Option<i64>,
    state: String,
    review_reason: Option<String>,
    renewal_of: Option<String>,
    granted: Option<i64>,
    pulled_through: i64,
    refunded: i64,
    refunded_in_full: bool,
    disputed: bool,
}

/// An order's terms as SQLite gives them.
pub(crate) struct TermsRow {
    plan: String,
    kind: String,
    days: i64,
    traffic_bytes: i64,
    amount: i64,
    currency: String,
    payment_link_id: String,
    renewal_amount: Option<i64>,
}

/// Reads the columns of [`ORDER_COLUMNS`].
pub(crate) fn order_row(row: &Row<'_>) -> rusqlite::Result<OrderRow> {
    Ok(OrderRow {
        id: row.get(0)?,
        user: row.get(1)?,
        terms: terms_row(row, 2)?,
        created_at: row.get(10)?,
        paid_at: row.get(11)?,
        state: row.get(12)?,
        review_reason: row.get(13)?,
        renewal_of: row.get(14)?,
        granted: row.get(15)?,
        pulled_through: row.get(16)?,
        refunded: row.get(17)?,
        refunded_in_full: row.get(18)?,
        disputed: row.get(19)?,
    })
}

/// Reads an order's eight columns of terms, the first at `from`.
pub(crate) fn terms_row(row: &Row<'_>, from: usize) -> rusqlite::Result<TermsRow> {
    Ok(TermsRow {
        plan: row.get(from)?,
        kind: row.get(from + 1)?,
        days: row.get(from + 2)?,
        traffic_bytes: row.get(from + 3)?,
        amount: row.get(from + 4)?,
        currency: row.get(from + 5)?,
        payment_link_id: row.get(from + 6)?,
        renewal_amount: row.get(from + 7)?,
    })
}

impl TermsRow {
    /// The terms, checked.
    pub(crate) fn checked(self) -> Result<OrderTerms, StoreError> {
        let invalid = |what: &'static str, text: &str| corrupt(what, format!("{text:?}"));
        Ok(OrderTerms {
            plan: PlanId::parse(&self.plan).ok_or_else(|| invalid("order's plan", &self.plan))?,
            kind: PlanKind::parse(&self.kind)
                .ok_or_else(|| invalid("order's plan kind", &self.kind))?,
            days: u32::try_from(self.days)
                .map_err(|_| invalid("order's days", &self.days.to_string()))?,
            traffic_bytes: count_from_sql(self.traffic_bytes, "order's traffic")?,
            amount: count_from_sql(self.amount, "order's amount")?,
            renewal_amount: self
                .renewal_amount
                .map(|amount| count_from_sql(amount, "order's renewal amount"))
                .transpose()?,
            currency: Currency::parse(&self.currency)
                .ok_or_else(|| invalid("order's currency", &self.currency))?,
            payment_link_id: self.payment_link_id,
        })
    }
}

impl OrderRow {
    /// The order, checked.
    pub(crate) fn checked(self) -> Result<Order, StoreError> {
        let progress = match (self.state.as_str(), self.review_reason.as_deref()) {
            ("pending", None) => Progress::Pending,
            ("paid", None) => match self.granted {
                Some(seq) => Progress::Granted {
                    pulled: seq <= self.pulled_through,
                },
                None if self.refunded_in_full || self.disputed => Progress::Returned,
                None => Progress::Checking,
            },
            ("review", Some(reason)) => Progress::Review(
                ReviewReason::parse(reason)
                    .ok_or_else(|| corrupt("order's review reason", format!("{reason:?}")))?,
            ),
            (state, _) => return Err(corrupt("order's state", format!("{state:?}"))),
        };
        Ok(Order {
            id: order_from_sql(&self.id)?,
            user: user_from_sql(&self.user)?,
            terms: self.terms.checked()?,
            created_at: time_from_sql(self.created_at, "order's time")?,
            paid_at: self
                .paid_at
                .map(|secs| time_from_sql(secs, "order's payment time"))
                .transpose()?,
            progress,
            refunded: count_from_sql(self.refunded, "refunded amount")?,
            refunded_in_full: self.refunded_in_full,
            disputed: self.disputed,
            renewal_of: self.renewal_of.as_deref().map(order_from_sql).transpose()?,
        })
    }
}

/// An order's terms as its row's eight columns.
pub(crate) type TermsColumns<'a> = (
    &'a str,
    &'static str,
    i64,
    i64,
    i64,
    &'static str,
    &'a str,
    Option<i64>,
);

/// `terms` as the eight columns of an order's row.
pub(crate) fn terms_to_sql(terms: &OrderTerms) -> Result<TermsColumns<'_>, StoreError> {
    Ok((
        terms.plan.as_str(),
        terms.kind.as_str(),
        i64::from(terms.days),
        count_to_sql(terms.traffic_bytes, "order's traffic")?,
        count_to_sql(terms.amount, "order's amount")?,
        terms.currency.as_str(),
        terms.payment_link_id.as_str(),
        terms
            .renewal_amount
            .map(|amount| count_to_sql(amount, "order's renewal amount"))
            .transpose()?,
    ))
}

/// Whether `account` has a subscription that Stripe may charge again.
fn renewing(conn: &Connection, account: AccountId) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM {RENEWAL_TABLES}
                 WHERE o.account_id = ?1 AND {RENEWS})"
        ),
        [account.0],
        |row| row.get(0),
    )?)
}

impl Store {
    /// Places an order on `terms` for the account signed in with the
    /// session whose token has `session`, and gives its id: `fresh`, or
    /// that of the account's unpaid order for the same plan if it stands
    /// on the same terms for the same Mooring user. An unpaid order on
    /// other terms is replaced.
    pub fn place_order(
        &mut self,
        session: &TokenDigest,
        terms: &OrderTerms,
        fresh: OrderId,
        now: UnixTime,
    ) -> Result<OrderId, OrderError> {
        self.place(session, terms, fresh, now, false)
    }

    /// Places an order on a private offer, checking the Mooring binding in the write transaction.
    pub fn place_introductory_order(
        &mut self,
        session: &TokenDigest,
        terms: &OrderTerms,
        fresh: OrderId,
        now: UnixTime,
    ) -> Result<OrderId, OrderError> {
        self.place(session, terms, fresh, now, true)
    }

    fn place(
        &mut self,
        session: &TokenDigest,
        terms: &OrderTerms,
        fresh: OrderId,
        now: UnixTime,
        introductory: bool,
    ) -> Result<OrderId, OrderError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(account) = session_account_id(&tx, session, now)? else {
            return Err(OrderError::SignedOut);
        };
        let user: Option<String> = tx
            .query_row(
                "SELECT mooring_user FROM bindings WHERE account_id = ?1",
                [account.0],
                |row| row.get(0),
            )
            .optional()?;
        let Some(user) = user else {
            return Err(OrderError::Unbound);
        };
        if terms.kind.renews() && (renewing(&tx, account)? || recurring_waits(&tx, &user)?) {
            return Err(OrderError::Renewing);
        }
        let terms = if introductory {
            let IntroState::Available(link) = intro_state(&tx, account)? else {
                return Err(OrderError::Introductory);
            };
            OrderTerms {
                payment_link_id: link.id.as_str().to_owned(),
                ..terms.clone()
            }
        } else {
            terms.clone()
        };
        let unpaid = tx
            .query_row(
                "SELECT id, mooring_user, plan, plan_kind, days, traffic_bytes, amount, currency,
                        payment_link_id, renewal_amount
                 FROM orders WHERE account_id = ?1 AND plan = ?2 AND state = 'pending'",
                params![account.0, terms.plan.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        terms_row(row, 2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((id, placed_for, placed_on)) = unpaid {
            if placed_for == user && placed_on.checked()? == terms {
                return Ok(order_from_sql(&id)?);
            }
            tx.execute("DELETE FROM orders WHERE id = ?1", [&id])?;
        }
        let (plan, kind, days, traffic_bytes, amount, currency, link, renewal_amount) =
            terms_to_sql(&terms)?;
        tx.execute(
            "INSERT INTO orders (id, account_id, mooring_user, plan, plan_kind, days,
                 traffic_bytes, amount, currency, payment_link_id, renewal_amount, created_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'pending')",
            params![
                fresh.as_str(),
                account.0,
                user,
                plan,
                kind,
                days,
                traffic_bytes,
                amount,
                currency,
                link,
                renewal_amount,
                time_to_sql(now),
            ],
        )?;
        if introductory {
            tx.execute(
                "UPDATE introductory_offers SET order_id = ?2 WHERE mooring_user = ?1",
                params![user, fresh.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(fresh)
    }

    /// The `limit` newest orders of `account`, newest first.
    pub fn orders_of(&self, account: AccountId, limit: u32) -> Result<Vec<Order>, StoreError> {
        let rows: Vec<OrderRow> = self
            .conn
            .prepare(&format!(
                "SELECT {ORDER_COLUMNS} FROM {ORDER_TABLES} WHERE o.account_id = ?1
                 ORDER BY o.created_at DESC, o.id LIMIT ?2"
            ))?
            .query_map(params![account.0, limit], order_row)?
            .collect::<Result<_, _>>()?;
        rows.into_iter().map(OrderRow::checked).collect()
    }

    /// The order `id`, if it is one of `account`'s.
    pub fn order_of(&self, account: AccountId, id: &OrderId) -> Result<Option<Order>, StoreError> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {ORDER_COLUMNS} FROM {ORDER_TABLES}
                     WHERE o.account_id = ?1 AND o.id = ?2"
                ),
                params![account.0, id.as_str()],
                order_row,
            )
            .optional()?
            .map(OrderRow::checked)
            .transpose()
    }

    /// The subscriptions of `account` that Stripe may charge again, oldest
    /// first.
    pub fn renewals_of(&self, account: AccountId) -> Result<Vec<AutoRenewal>, StoreError> {
        let rows: Vec<(String, String)> = self
            .conn
            .prepare(&format!(
                "SELECT o.plan, COALESCE(s.state, 'active') FROM {RENEWAL_TABLES}
                 WHERE o.account_id = ?1 AND o.renewal_of IS NULL AND {RENEWS}
                 ORDER BY o.created_at, o.id"
            ))?
            .query_map([account.0], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(plan, state)| {
                Ok(AutoRenewal {
                    plan: PlanId::parse(&plan)
                        .ok_or_else(|| corrupt("order's plan", format!("{plan:?}")))?,
                    state: renewal_from_sql(&state)?,
                })
            })
            .collect()
    }
}

/// A subscription's state read back.
pub(crate) fn renewal_from_sql(text: &str) -> Result<Renewal, StoreError> {
    Renewal::parse(text).ok_or_else(|| corrupt("subscription state", format!("{text:?}")))
}
