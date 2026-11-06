//! SQLite persistence of the Mooring portal.
//!
//! One file holds the accounts, their sessions, the bindings of accounts to
//! Mooring users, and the latest snapshot record of each bound user. The
//! server and the operator's commands open the same file: SQLite's write
//! lock serialises them (WAL journal, five-second busy timeout), and every
//! operation that reads before it writes runs in an immediate transaction,
//! so its checks still hold when it writes.
//!
//! Values are checked again when read back: a row that no longer parses as
//! its domain type is reported as [`StoreError::Corrupt`], never replaced
//! by a default.

#[cfg(not(unix))]
compile_error!("the portal store supports Unix hosts only (it sets file modes)");

mod accounts;
mod bindings;
mod error;
mod file;
mod rows;
mod schema;
mod sessions;
mod subscriptions;

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

pub use accounts::{Account, AccountSummary, Credentials, DeleteError, NewAccount, RegisterError};
pub use bindings::{BindError, Binding, UnbindError};
pub use error::{StoreError, UnknownAccount};
pub use sessions::{MAX_SESSIONS_PER_ACCOUNT, NewSession};
pub use subscriptions::{ImportError, ImportReport, MAX_CLOCK_SKEW_SECS, Subscription};

/// How long a writer waits for another's lock before failing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// An account's row id: internal, never shown or accepted from a browser.
/// Every table refers to accounts by it, so it lives here rather than in
/// any one table's module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountId(pub(crate) i64);

/// The portal's database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

/// Whether [`Store::open`] may create the database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Create it, owner-only, when it is missing: the server's first start.
    CreateIfMissing,
    /// It must exist already: the operator's commands, which would
    /// otherwise act on a new, empty database when a path is mistyped.
    Existing,
}

impl Store {
    /// Opens the database at `path` and brings its schema up to date,
    /// creating the file with mode 0600 when `mode` allows. Refuses a file
    /// that others may read or write.
    pub fn open(path: &Path, mode: OpenMode) -> Result<Self, StoreError> {
        file::prepare(path, mode)?;
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // rusqlite already waits five seconds by default; set the portal's
        // own timeout before anything else so that it governs every
        // statement, the switch of a new file to WAL included.
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        Self::migrated(conn)
    }

    /// A private database in memory, for tests. No other connection can
    /// lock it, so it needs no busy timeout.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::migrated(Connection::open_in_memory()?)
    }

    /// Turns on foreign keys and brings the schema up to date.
    fn migrated(mut conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }
}

#[cfg(test)]
mod tests;
