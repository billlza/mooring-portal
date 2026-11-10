//! The command line.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use portal_core::account::{AccountCode, AccountCodeError, Email, EmailError};
use portal_core::snapshot::{MooringUserId, MooringUserIdError};

/// The Mooring portal: its server, and the operator's commands on its
/// database.
#[derive(Debug, Parser)]
#[command(name = "mooring-portal", version)]
pub(crate) struct Cli {
    /// The portal's SQLite database. `serve` creates it, owner-only, if it
    /// is missing; every other command needs it to exist.
    #[arg(long, value_name = "FILE")]
    pub(crate) db: PathBuf,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Run the portal's web server until interrupted.
    Serve(ServeArgs),
    /// List every account with its binding.
    Accounts,
    /// Print `--user <id>` for every bound account's Mooring user, the
    /// names Mooring's exports must be given.
    BoundUsers,
    /// Bind an account to a Mooring user.
    Bind {
        /// The account's code, as its owner reads it from their account page.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
        /// The Mooring user id (10 lowercase base32 characters).
        #[arg(long, value_name = "ID", value_parser = mooring_user)]
        user: MooringUserId,
    },
    /// Unbind an account; its subscription record, CFM profile and
    /// subscription link go with the binding.
    Unbind {
        /// The account's code.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
    },
    /// Import a portal snapshot exported by Mooring (contract version 1).
    Import {
        /// The snapshot file.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Import a CFM profile bundle exported by Mooring (contract version 2).
    /// It must name every bound account's Mooring user (`bound-users`
    /// prints them), and it replaces every stored profile.
    ImportProfiles {
        /// The bundle file.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Put GitHub's latest CFM release on the download page. The file is
    /// GitHub's description of the release, as its API answers for
    /// `releases/latest`; the host fetches it on a timer.
    ImportRelease {
        /// The description file.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Delete an account with its sessions, binding, record, profile and
    /// link.
    DeleteAccount {
        /// The account's code.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
    },
    /// Ban an account caught abusing the service: delete it as
    /// `delete-account` does, and keep its email address from registering
    /// again. Revoke its Mooring user in Mooring as well.
    Ban {
        /// The account's code.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
    },
    /// Let a banned email address register again.
    Unban {
        /// The address, as it was registered (case does not matter).
        #[arg(long, value_name = "ADDRESS", value_parser = email_address)]
        email: Email,
    },
    /// Erase the copies of deleted rows that the database files may still
    /// hold. Every command that deletes or overwrites rows does it itself;
    /// run this when one of them says it could not.
    EraseDeleted,
}

/// How the server runs.
#[derive(Debug, Args)]
pub(crate) struct ServeArgs {
    /// The address to listen on.
    #[arg(long, value_name = "ADDRESS", default_value = "127.0.0.1:8080")]
    pub(crate) listen: SocketAddr,
    /// The origin browsers use, such as `https://portal.example`; plain http
    /// only for localhost.
    #[arg(long, value_name = "URL")]
    pub(crate) origin: String,
    /// The server sits behind a reverse proxy on this host, which appends
    /// the client's address to X-Forwarded-For.
    #[arg(long)]
    pub(crate) behind_proxy: bool,
    /// Registration closes once the portal holds this many accounts.
    #[arg(long, value_name = "N", default_value_t = 200)]
    pub(crate) max_accounts: u32,
}

fn account_code(text: &str) -> Result<AccountCode, AccountCodeError> {
    AccountCode::parse(text)
}

fn mooring_user(text: &str) -> Result<MooringUserId, MooringUserIdError> {
    MooringUserId::parse(text)
}

fn email_address(text: &str) -> Result<Email, EmailError> {
    Email::parse(text)
}
