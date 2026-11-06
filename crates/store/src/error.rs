//! Errors shared by every store operation.

use std::io;
use std::path::PathBuf;

use portal_core::account::AccountCode;

/// A failure of the database itself, or a row that no longer parses.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite reported an error.
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database file could not be created or inspected.
    #[error("cannot prepare the database file {path}: {source}")]
    File {
        /// The file.
        path: PathBuf,
        /// What failed.
        #[source]
        source: io::Error,
    },
    /// The database file does not exist, and the caller may not create it.
    #[error("there is no database at {path}")]
    NoDatabase {
        /// The path given.
        path: PathBuf,
    },
    /// The database file is open to other users of the host.
    #[error(
        "the database file {path} has mode {mode:o}, so other users may read or change it; \
         restrict it with `chmod 600`"
    )]
    InsecurePermissions {
        /// The file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
    /// The database was written by a newer build.
    #[error("the database schema is version {found}, newer than this build's {supported}")]
    SchemaTooNew {
        /// The file's schema version.
        found: i64,
        /// The newest version this build knows.
        supported: i64,
    },
    /// A row the operation needs no longer exists (for example, the
    /// operator deleted the account while the request was in flight).
    #[error("the {what} no longer exists")]
    Missing {
        /// What was looked for.
        what: &'static str,
    },
    /// A stored value does not parse as its type.
    #[error("the database holds an invalid {what}: {detail}")]
    Corrupt {
        /// What was being read.
        what: &'static str,
        /// Why it is invalid.
        detail: String,
    },
}

/// No account has the code the operator gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no account has the code {0}")]
pub struct UnknownAccount(pub AccountCode);
