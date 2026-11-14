//! The schema and its migrations, numbered by SQLite's `user_version`.
//!
//! The missing migrations run in one immediate transaction with the version
//! bump, so a database is always at exactly one version, and the version is
//! read inside that transaction, so two openers of a new file cannot both
//! apply a migration: the second waits for the first's lock, then finds the
//! schema current. A database newer than this build is refused rather than
//! read.

use rusqlite::{Connection, TransactionBehavior};

use crate::StoreError;
use crate::rows::corrupt;

/// The migrations in order; the database's `user_version` counts those
/// applied.
pub(crate) const MIGRATIONS: &[&str] = &[
    // 1: accounts, sessions, bindings and subscription records.
    "CREATE TABLE accounts (
         id INTEGER PRIMARY KEY,
         code TEXT NOT NULL UNIQUE,
         email TEXT NOT NULL UNIQUE,
         password_hash TEXT NOT NULL,
         created_at INTEGER NOT NULL,
         password_changed_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE sessions (
         token_digest BLOB PRIMARY KEY CHECK (length(token_digest) = 32),
         account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
         created_at INTEGER NOT NULL,
         expires_at INTEGER NOT NULL
     ) STRICT;
     CREATE INDEX sessions_by_account ON sessions (account_id);
     CREATE TABLE bindings (
         account_id INTEGER PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
         mooring_user TEXT NOT NULL UNIQUE,
         bound_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE subscription_records (
         mooring_user TEXT PRIMARY KEY REFERENCES bindings (mooring_user) ON DELETE CASCADE,
         document TEXT NOT NULL
     ) STRICT;",
    // 2: CFM profiles, the last imported bundle's time, and subscription
    // links. Profiles and links refer to the binding, so unbinding (or
    // deleting the account) removes both.
    "CREATE TABLE cfm_profiles (
         mooring_user TEXT PRIMARY KEY REFERENCES bindings (mooring_user) ON DELETE CASCADE,
         document TEXT NOT NULL
     ) STRICT;
     CREATE TABLE profile_bundle (
         id INTEGER PRIMARY KEY CHECK (id = 1),
         generated_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE subscription_links (
         account_id INTEGER PRIMARY KEY REFERENCES bindings (account_id) ON DELETE CASCADE,
         token_digest BLOB NOT NULL UNIQUE CHECK (length(token_digest) = 32),
         created_at INTEGER NOT NULL
     ) STRICT;",
    // 3: the ban list. A ban deletes the account; the digest of its email
    // address stays, so that the address cannot register again.
    "CREATE TABLE banned_emails (
         email_digest BLOB PRIMARY KEY CHECK (length(email_digest) = 32),
         banned_at INTEGER NOT NULL
     ) STRICT;",
    // 4: the CFM release the download page offers, and when it was last
    // found to be the latest. One row, replaced whole.
    "CREATE TABLE cfm_release (
         id INTEGER PRIMARY KEY CHECK (id = 1),
         version TEXT NOT NULL,
         published_at INTEGER NOT NULL,
         dmg_sha256 TEXT NOT NULL CHECK (length(dmg_sha256) = 64),
         checked_at INTEGER NOT NULL
     ) STRICT;",
    // 5: payment (docs/payments.md). An order and its entries in the
    // journal Mooring pulls go with their account. What Stripe has said
    // is kept by Stripe's own ids, because its events come in no order:
    // a payment's card, refunds and dispute by the payment, an invoice's
    // payment by the invoice, a subscription's state by the subscription.
    // Payments that are no order's wait for the operator. The cards of a
    // banned account go with its ban. A database holds the payments of
    // one mode, Stripe's test mode or real money, and says which.
    "CREATE TABLE orders (
         id TEXT PRIMARY KEY CHECK (length(id) = 20),
         account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
         mooring_user TEXT NOT NULL,
         plan TEXT NOT NULL,
         plan_kind TEXT NOT NULL,
         days INTEGER NOT NULL,
         traffic_bytes INTEGER NOT NULL,
         amount INTEGER NOT NULL,
         currency TEXT NOT NULL,
         payment_link_id TEXT NOT NULL,
         created_at INTEGER NOT NULL,
         state TEXT NOT NULL CHECK (state IN ('pending', 'paid', 'review')),
         review_reason TEXT,
         card_passed INTEGER NOT NULL DEFAULT 0 CHECK (card_passed IN (0, 1)),
         paid_at INTEGER,
         session TEXT UNIQUE,
         payment_intent TEXT UNIQUE,
         invoice TEXT UNIQUE,
         subscription TEXT,
         renewal_of TEXT REFERENCES orders (id) ON DELETE CASCADE,
         CHECK ((state = 'review') = (review_reason IS NOT NULL)),
         CHECK ((state = 'pending') = (paid_at IS NULL))
     ) STRICT;
     CREATE INDEX orders_by_account ON orders (account_id);
     CREATE INDEX orders_by_subscription ON orders (subscription);
     CREATE UNIQUE INDEX orders_pending_once ON orders (account_id, plan)
         WHERE state = 'pending';
     CREATE TABLE entitlements (
         seq INTEGER PRIMARY KEY AUTOINCREMENT,
         order_id TEXT NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
         kind TEXT NOT NULL CHECK (kind IN ('grant', 'refund', 'dispute')),
         amount INTEGER,
         in_full INTEGER CHECK (in_full IN (0, 1)),
         recorded_at INTEGER NOT NULL,
         CHECK ((kind = 'grant') = (amount IS NULL)),
         CHECK ((kind = 'refund') = (in_full IS NOT NULL))
     ) STRICT;
     CREATE INDEX entitlements_by_order ON entitlements (order_id);
     CREATE UNIQUE INDEX entitlements_once ON entitlements (order_id, kind)
         WHERE kind <> 'refund';
     CREATE TABLE pull_access (
         id INTEGER PRIMARY KEY CHECK (id = 1),
         token_digest BLOB CHECK (token_digest IS NULL OR length(token_digest) = 32),
         token_created_at INTEGER,
         pulled_through INTEGER NOT NULL DEFAULT 0,
         pulled_at INTEGER
     ) STRICT;
     CREATE TABLE stripe_events (
         id TEXT PRIMARY KEY,
         received_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE stripe_payments (
         payment_intent TEXT PRIMARY KEY,
         card TEXT CHECK (card IN ('clear', 'banned', 'unchecked')),
         refunded_amount INTEGER NOT NULL DEFAULT 0,
         refunded_in_full INTEGER NOT NULL DEFAULT 0 CHECK (refunded_in_full IN (0, 1)),
         disputed_amount INTEGER,
         noticed_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE stripe_invoice_payments (
         invoice TEXT PRIMARY KEY,
         payment_intent TEXT NOT NULL,
         noticed_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE stripe_subscriptions (
         subscription TEXT PRIMARY KEY,
         state TEXT NOT NULL CHECK (state IN ('active', 'ending', 'ended')),
         state_at INTEGER NOT NULL,
         noticed_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE stray_payments (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         noticed_at INTEGER NOT NULL,
         made_at INTEGER NOT NULL,
         reason TEXT NOT NULL,
         session TEXT UNIQUE,
         invoice TEXT UNIQUE,
         payment_intent TEXT,
         subscription TEXT,
         amount INTEGER,
         currency TEXT,
         CHECK (session IS NOT NULL OR invoice IS NOT NULL)
     ) STRICT;
     CREATE TABLE banned_cards (
         card_digest BLOB NOT NULL CHECK (length(card_digest) = 32),
         email_digest BLOB NOT NULL REFERENCES banned_emails (email_digest) ON DELETE CASCADE,
         PRIMARY KEY (card_digest, email_digest)
     ) STRICT;
     CREATE TABLE payment_mode (
         id INTEGER PRIMARY KEY CHECK (id = 1),
         livemode INTEGER NOT NULL CHECK (livemode IN (0, 1))
     ) STRICT;",
    // 6: Mac rental bookings (docs/rentals.md): what an account asked the
    // operator for, on the terms the offer had then. One booking per
    // account and machine; it goes with its account. Numbers are never
    // reused, so the operator cannot close a booking made after they
    // listed another under its number.
    "CREATE TABLE mac_bookings (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
         machine TEXT NOT NULL,
         chip TEXT NOT NULL,
         memory_gb INTEGER NOT NULL,
         storage_gb INTEGER NOT NULL,
         period TEXT NOT NULL CHECK (period IN ('day', 'week', 'month', 'quarter', 'year')),
         price INTEGER NOT NULL,
         first_price INTEGER NOT NULL,
         placed_at INTEGER NOT NULL,
         UNIQUE (account_id, machine)
     ) STRICT;",
];

/// Applies the migrations the database lacks.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let applied = usize::try_from(found)
        .map_err(|_| corrupt("schema version", format!("{found} is negative")))?;
    let Some(missing) = MIGRATIONS.get(applied..) else {
        return Err(StoreError::SchemaTooNew {
            found,
            supported: version(MIGRATIONS.len()),
        });
    };
    if !missing.is_empty() {
        for sql in missing {
            tx.execute_batch(sql)?;
        }
        tx.pragma_update(None, "user_version", version(MIGRATIONS.len()))?;
    }
    tx.commit()?;
    Ok(())
}

/// A migration count as SQLite's `user_version`. There are a handful of
/// migrations, so the conversion is exact.
const fn version(count: usize) -> i64 {
    count as i64
}
