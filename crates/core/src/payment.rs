//! Payment: what the portal sells, the orders users place for it, what
//! Stripe tells the portal about their payment, and what the portal tells
//! Mooring to grant.
//!
//! The portal takes no payment itself and holds no key that could. A user
//! places an order and is sent, with the order's id, to a payment page the
//! operator made at Stripe (a Payment Link); Stripe then tells the portal,
//! in signed events, what was paid; and Mooring pulls the paid orders and
//! grants them. Three contracts meet here:
//!
//! - the operator's catalogue ([`Catalogue`]): the plans, their prices
//!   and their payment pages;
//! - Stripe's events ([`stripe`]): signed with a secret the portal shares
//!   with Stripe, and read only for the handful of values the portal acts
//!   on;
//! - the entitlements Mooring pulls ([`feed`]).
//!
//! Stripe's events carry a payer's name, email address and card details.
//! None of it is read: every type here holds only identifiers, amounts and
//! states, so nothing else can reach the database or a log.

mod catalogue;
pub mod feed;
mod order;
pub mod stripe;
mod terms;

pub use catalogue::{
    Catalogue, CatalogueError, Currency, INTRO_PERIOD_SECS, INTRO_PERIODS, MAX_CATALOGUE_BYTES,
    Plan, PlanId, PlanKind,
};
pub use order::{OrderId, OrderIdError};
pub use terms::{CardVerdict, OrderTerms, ReviewReason};
