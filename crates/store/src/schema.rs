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
