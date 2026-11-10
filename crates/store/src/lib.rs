//! SQLite persistence of the Mooring portal.
//!
//! One file holds the accounts, their sessions, the bindings of accounts to
//! Mooring users, the latest snapshot record and CFM profile of each bound
//! user, the accounts' subscription links, the ban list (digests of
//! banned accounts' email addresses), and the CFM release the download
//! page offers. The
//! server and the operator's commands open the same file: SQLite's write
//! lock serialises them (WAL journal, five-second busy timeout), and every
//! operation that reads before it writes runs in an immediate transaction,
//! so its checks still hold when it writes.
//!
//! Every connection zeroes most of what it deletes (secure deletion);
//! [`Store::erase_deleted`] removes every copy that is left, in the
//! database file and in SQLite's write-ahead log.
//!
//! Values are checked again when read back: a row that no longer parses as
//! its domain type is reported as [`StoreError::Corrupt`], never replaced
//! by a default.

#[cfg(not(unix))]
compile_error!("the portal store supports Unix hosts only (it sets file modes)");

mod accounts;
mod bans;
mod bindings;
mod error;
mod file;
mod links;
mod profiles;
mod releases;
mod rows;
mod schema;
mod sessions;
mod subscriptions;

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

pub use accounts::{Account, AccountSummary, Credentials, DeleteError, NewAccount, RegisterError};
pub use bans::{Ban, BanError};
pub use bindings::{BindError, Binding, BoundAccount, UnbindError};
pub use error::{StoreError, UnknownAccount};
pub use links::{CfmDelivery, LinkError, LinkedProfile, RevokeError};
pub use profiles::{
    ProfileImportError, ProfileImportReport, StoredProfile, Withheld, WithheldLink,
};
pub use releases::{OfferedRelease, ReleaseChange};
pub use sessions::{MAX_SESSIONS_PER_ACCOUNT, NewSession, PasswordChange};
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

    /// Turns on foreign keys, secure deletion and in-memory temporary
    /// storage, and brings the schema up to date.
    fn migrated(mut conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        // Zero what this connection deletes, in the pages it changes and in
        // the pages it frees ("fast" would leave the freed pages as they
        // were). This covers most deletions as they happen; only
        // [`Store::erase_deleted`] covers them all.
        conn.pragma_update(None, "secure_delete", true)?;
        // Keep temporary databases, such as the copy `VACUUM` builds, in
        // memory rather than in files the host's temporary directory keeps.
        conn.pragma_update(None, "temp_store", "memory")?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Erases every copy of deleted rows that the database files still
    /// hold, waiting for other connections' reads and writes as long as for
    /// any lock.
    ///
    /// Secure deletion is not enough. A page that SQLite rebuilds while it
    /// rebalances the tree keeps, in the space it no longer uses, the bytes
    /// of cells that moved, and deleting such a row later zeroes only the
    /// cell where it lives; and the write-ahead log keeps the earlier
    /// versions of every page it changed, in place for as long as the
    /// server runs. So this rebuilds the database from its live rows
    /// (`VACUUM`), then copies the whole log into the database file and
    /// truncates the log to nothing. Account ids are kept: they are integer
    /// primary keys. Session rowids, which only break ties between sessions
    /// started in the same second, keep their order.
    pub fn erase_deleted(&mut self) -> Result<(), StoreError> {
        self.conn
            .execute_batch("VACUUM")
            .map_err(|error| match error.sqlite_error_code() {
                Some(rusqlite::ErrorCode::DatabaseBusy) => StoreError::EraseBusy,
                _ => StoreError::Sqlite(error),
            })?;
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
        if busy != 0 {
            return Err(StoreError::EraseBusy);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
