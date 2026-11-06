//! The `mooring-portal` command: `serve` runs the portal, and the operator
//! commands manage accounts, bindings, and snapshot and CFM profile imports
//! in the same database file, while the server runs or not.

mod cli;
mod error;
mod operator;
mod serve;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser as _;
use portal_core::time::Clock;
use portal_store::{OpenMode, Store};

pub use error::CommandError;

use cli::{Cli, Command};

/// Runs the command line `args` (the program name first), writing results
/// to `out` and failures to `err`. Exit status 0 is success, 1 a failure
/// and 2 a usage error.
pub fn run(
    args: impl IntoIterator<Item = impl Into<OsString> + Clone>,
    clock: &dyn Clock,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(usage) => {
            let (text, code) = if usage.use_stderr() {
                (usage.render().to_string(), 2)
            } else {
                // `--help` and `--version` are answers, not failures.
                (usage.render().to_string(), 0)
            };
            let sink: &mut dyn Write = if code == 0 { out } else { err };
            return match sink.write_all(text.as_bytes()) {
                Ok(()) => ExitCode::from(code),
                Err(_) => ExitCode::FAILURE,
            };
        }
    };
    match execute(cli, clock, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Every error's message already names its cause. If even stderr
            // fails there is nowhere left to report to; the exit status
            // still says the command failed.
            let _ = writeln!(err, "mooring-portal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli, clock: &dyn Clock, out: &mut dyn Write) -> Result<(), CommandError> {
    let db = cli.db;
    let with_store = |work: &mut dyn FnMut(&mut Store) -> Result<(), CommandError>| {
        let mut store = Store::open(&db, OpenMode::Existing)?;
        work(&mut store)
    };
    match cli.command {
        Command::Serve(args) => serve::serve(&db, &args),
        Command::Accounts => with_store(&mut |store| operator::accounts(store, out)),
        Command::Bind { account, user } => {
            with_store(&mut |store| operator::bind(store, clock, &account, &user, out))
        }
        Command::Unbind { account } => {
            with_store(&mut |store| operator::unbind(store, &account, out))
        }
        Command::Import { file } => {
            with_store(&mut |store| operator::import(store, clock, &file, out))
        }
        Command::ImportProfiles { file } => {
            with_store(&mut |store| operator::import_profiles(store, clock, &file, out))
        }
        Command::DeleteAccount { account } => {
            with_store(&mut |store| operator::delete_account(store, &account, out))
        }
    }
}

#[cfg(test)]
mod tests;
