//! Why a command failed.

use std::io;
use std::path::PathBuf;

use portal_core::snapshot::SnapshotError;
use portal_core::time::TimeError;
use portal_store::{BindError, DeleteError, ImportError, StoreError, UnbindError};
use portal_web::{OriginError, StartupError};

/// Why a command failed.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A binding was refused.
    #[error(transparent)]
    Bind(#[from] BindError),
    /// An unbinding was refused.
    #[error(transparent)]
    Unbind(#[from] UnbindError),
    /// An import was refused.
    #[error(transparent)]
    Import(#[from] ImportError),
    /// A deletion was refused.
    #[error(transparent)]
    Delete(#[from] DeleteError),
    /// The snapshot file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// What failed.
        #[source]
        source: io::Error,
    },
    /// The snapshot file is not a valid snapshot.
    #[error("{path}: {source}")]
    Snapshot {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        #[source]
        source: SnapshotError,
    },
    /// The origin given to `serve` is unusable.
    #[error(transparent)]
    Origin(#[from] OriginError),
    /// The clock is unusable.
    #[error(transparent)]
    Time(#[from] TimeError),
    /// The web layer could not start.
    #[error(transparent)]
    Startup(#[from] StartupError),
    /// Logging could not be set up.
    #[error("cannot set up logging: {0}")]
    Logging(String),
    /// The server failed to start or stopped with an error.
    #[error("the server failed: {0}")]
    Server(#[source] io::Error),
    /// Writing the command's output failed.
    #[error("cannot write the output: {0}")]
    Output(#[from] io::Error),
}
