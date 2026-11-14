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
    // 6: a first order can promise a different amount for renewals.
    // Orders already on disk have one amount for both, as before.
    "ALTER TABLE orders ADD COLUMN renewal_amount INTEGER
         CHECK (renewal_amount IS NULL OR (
             renewal_amount BETWEEN 1 AND 10000000
             AND plan_kind = 'month_auto' AND renewal_of IS NULL
         ));",
    // 7: a private, one-use test offer belongs to a Mooring user, not a
    // portal account or billing email. Keep the use marker through rebinding.
    "CREATE TABLE introductory_offers (
         mooring_user TEXT PRIMARY KEY,
         link_url TEXT NOT NULL,
         link_id TEXT NOT NULL UNIQUE,
         order_id TEXT UNIQUE REFERENCES orders (id) ON DELETE SET NULL,
         claimed_at INTEGER,
         customer TEXT,
         paid_intent TEXT,
         paid_at INTEGER,
         schedule TEXT UNIQUE,
         term_end INTEGER,
         schedule_state TEXT CHECK (schedule_state IN ('open','ended')),
         schedule_ends INTEGER CHECK (schedule_ends IN (0,1)),
         schedule_at INTEGER,
         requested_at INTEGER,
         CHECK ((schedule IS NULL) = (term_end IS NULL)),
         CHECK ((schedule IS NULL) = (schedule_state IS NULL)),
         CHECK ((schedule IS NULL) = (schedule_ends IS NULL)),
         CHECK ((schedule IS NULL) = (schedule_at IS NULL))
     ) STRICT;
     CREATE TABLE intro_paid_intents (
         payment TEXT PRIMARY KEY, customer TEXT NOT NULL, paid_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE intro_schedule_notices (
         order_id TEXT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
         schedule TEXT NOT NULL, customer TEXT NOT NULL, subscription TEXT,
         starts INTEGER NOT NULL, ends INTEGER NOT NULL, noticed INTEGER NOT NULL,
         state TEXT NOT NULL CHECK (state IN ('open','ended')), stops INTEGER NOT NULL CHECK (stops IN (0,1)),
         PRIMARY KEY (order_id, schedule)
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
