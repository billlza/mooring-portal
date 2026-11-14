//! What every page shows a signed-in account besides its content: whether
//! it may use the operator pages and whether a reply waits unread, and,
//! for an operator, how much waits for it.

use crate::orders::ORDER_TABLES;
use crate::payment_review::WAITING;
use crate::rows::count_from_sql;
use crate::{AccountId, Store, StoreError};

/// What every page shows a signed-in account besides its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    /// The account may use the operator pages.
    pub operator: bool,
    /// The operator wrote to the account after its owner last read the
    /// conversation.
    pub unread_reply: bool,
    /// How many conversations wait for a reply; counted for an operator
    /// alone, and 0 for anyone else.
    pub awaiting_reply: u64,
    /// How many payments wait: the orders that wait for the operator or
    /// for word of their card ([`Store::orders_waiting`]) but those whose
    /// payment is disputed, which wait for nothing the portal follows, and
    /// the payments that are no order's ([`Store::stray_payments`]);
    /// counted for an operator alone, and 0 for anyone else.
    pub payments_waiting: u64,
}

impl Signals {
    /// Everything that waits for an operator: conversations and payments.
    #[must_use]
    pub const fn waiting(&self) -> u64 {
        self.awaiting_reply.saturating_add(self.payments_waiting)
    }
}

impl Store {
    /// What every page shows `account` besides its content.
    pub fn signals(&self, account: AccountId) -> Result<Signals, StoreError> {
        let (operator, unread_reply): (bool, bool) = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM operators WHERE account_id = ?1),
                 EXISTS (SELECT 1 FROM feedback
                     WHERE account_id = ?1 AND from_operator = 1 AND seen_at IS NULL)",
            [account.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if !operator {
            return Ok(Signals {
                unread_reply,
                ..Signals::default()
            });
        }
        let (awaiting, payments): (i64, i64) = self.conn.query_row(
            &format!(
                "SELECT
                     (SELECT count(*) FROM feedback last
                      WHERE last.from_operator = 0 AND last.id =
                          (SELECT max(f.id) FROM feedback f WHERE f.account_id = last.account_id)),
                     (SELECT count(*) FROM {ORDER_TABLES}
                      WHERE {WAITING} AND p.disputed_amount IS NULL)
                         + (SELECT count(*) FROM stray_payments)"
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(Signals {
            operator,
            unread_reply,
            awaiting_reply: count_from_sql(awaiting, "feedback count")?,
            payments_waiting: count_from_sql(payments, "payment count")?,
        })
    }
}
