//! Why a command failed.

use std::io;
use std::path::PathBuf;

use portal_core::auth::{HashError, RandomError};
use portal_core::payment::CatalogueError;
use portal_core::payment::stripe::SecretError;
use portal_core::profile::ProfileError;
use portal_core::release::ReleaseError;
use portal_core::snapshot::SnapshotError;
use portal_core::time::TimeError;
use portal_store::{
    AccountProfileError, ApproveError, BanError, BindError, DeleteError, DiscardError,
    DismissError, ImportError, OperatorError, ProfileImportError, StoreError, UnbindError,
};
use portal_web::{OriginError, SecretCount, SignalPathError, StartupError};

/// Why the host's translator gave no answer to a question of `translate`.
#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
    /// Nothing listens on its socket, or the socket may not be opened.
    #[error("it cannot be reached: {0}")]
    Unreachable(#[source] io::Error),
    /// It did not take the question, or did not answer in time.
    #[error("it gave no answer: {0}")]
    Silent(#[source] io::Error),
    /// Its answer is longer than any answer to the question.
    #[error("its answer is too long")]
    TooLong,
    /// What follows the first line of its answer is not as many bytes as
    /// that line says: the answer was cut short, or counted wrongly.
    #[error("its answer is not as long as it says")]
    Incomplete,
    /// Its answer is not text.
    #[error("its answer is not UTF-8 text")]
    NotText,
    /// Its answer begins with neither `ok` and a number of bytes nor
    /// `none`, or names no language where one was asked for. What it said
    /// is not repeated: it may hold a message's words.
    #[error("it answered neither `ok` with what was asked for nor `none`")]
    Unexpected,
}

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
    /// A profile import was refused.
    #[error(transparent)]
    ProfileImport(#[from] ProfileImportError),
    /// A profile import for one account was refused.
    #[error(transparent)]
    AccountProfile(#[from] AccountProfileError),
    /// An account's operator rights did not change.
    #[error(transparent)]
    Operator(#[from] OperatorError),
    /// A deletion was refused.
    #[error(transparent)]
    Delete(#[from] DeleteError),
    /// A ban was refused.
    #[error(transparent)]
    Ban(#[from] BanError),
    /// The address could not be hashed for the ban list.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// `unban` or `ban-cards` named an address that is not banned, most
    /// likely mistyped.
    #[error("that email address is not banned; nothing changed")]
    NotBanned,
    /// An approval was refused.
    #[error(transparent)]
    Approve(#[from] ApproveError),
    /// A rehearsal was not discarded.
    #[error(transparent)]
    Discard(#[from] DiscardError),
    /// A stray payment was not dismissed.
    #[error(transparent)]
    Dismiss(#[from] DismissError),
    /// `revoke-pull-token` found no token.
    #[error("there is no pull token; nothing changed")]
    NoPullToken,
    /// `close-booking` named a booking that is not there: mistyped, or
    /// withdrawn or changed by its owner since it was listed.
    #[error("there is no booking #{0}; nothing changed (`bookings` lists them)")]
    NoBooking(u64),
    /// The operating system gave no random bytes for a token.
    #[error(transparent)]
    Random(#[from] RandomError),
    /// The catalogue file is not a catalogue.
    #[error("{path}: {source}")]
    Catalogue {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        #[source]
        source: CatalogueError,
    },
    /// The catalogue file can be changed by other users of the host.
    #[error(
        "the catalogue file {path} has mode {mode:o}, so other users may change what is sold \
         and where it is paid for; restrict it with `chmod go-w`"
    )]
    CatalogueFileOpen {
        /// The file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
    /// The file of signing secrets is open to other users of the host.
    #[error(
        "the signing secret file {path} has mode {mode:o}, so other users may read it; \
         restrict it with `chmod 600`"
    )]
    SecretFileOpen {
        /// The file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
    /// The file of signing secrets holds something that is not one. What
    /// it holds is not repeated.
    #[error("{path}, line {line}: {source}")]
    SigningSecret {
        /// The file.
        path: PathBuf,
        /// The line, counted from one.
        line: usize,
        /// Why it was refused.
        #[source]
        source: SecretError,
    },
    /// The file of signing secrets is longer than one can be. It is not
    /// read in part.
    #[error(
        "the signing secret file {path} is larger than {} bytes; it holds one secret, or two \
         while one is being rolled, a line each",
        crate::payments::MAX_SECRET_FILE_BYTES
    )]
    SecretFileTooLarge {
        /// The file.
        path: PathBuf,
    },
    /// The file of signing secrets is not text.
    #[error("{path} is not text: a signing secret is written whsec_ and letters, digits and _")]
    SecretFileNotText {
        /// The file.
        path: PathBuf,
    },
    /// The file of signing secrets holds none, or more than two.
    #[error("{path}: {source}")]
    SecretCount {
        /// The file.
        path: PathBuf,
        /// How many it holds.
        #[source]
        source: SecretCount,
    },
    /// An input file could not be read.
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
    /// The bundle file is not a valid profile bundle.
    #[error("{path}: {source}")]
    Profiles {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        #[source]
        source: ProfileError,
    },
    /// The file is not a release the download page can follow.
    #[error("{path}: {source}")]
    Release {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        #[source]
        source: ReleaseError,
    },
    /// The origin given to `serve` is unusable.
    #[error(transparent)]
    Origin(#[from] OriginError),
    /// The path given to `serve` for the new-message signal names no file.
    #[error(transparent)]
    SignalPath(#[from] SignalPathError),
    /// The new-message signal file cannot be where it was put: its
    /// directory is missing, or it is a directory itself.
    #[error("cannot use {path} as the new-message signal file: {source}")]
    Signal {
        /// The file.
        path: PathBuf,
        /// Why not.
        #[source]
        source: io::Error,
    },
    /// `translate` could not have some messages examined. They wait for
    /// the next run.
    #[error("the translator at {socket} left {failed} message(s) for the next run: {source}")]
    Translator {
        /// Its socket.
        socket: PathBuf,
        /// How many messages it was asked about in vain.
        failed: u32,
        /// Why the first of them had no answer.
        #[source]
        source: TranslatorError,
    },
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
