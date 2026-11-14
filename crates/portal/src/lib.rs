//! The `mooring-portal` command: `serve` runs the portal, and the operator
//! commands manage accounts, bindings, snapshot and CFM profile imports,
//! the CFM release on offer, what payments need of the operator, and the
//! Mac rental bookings, in the same database file, while the server runs
//! or not.

mod cli;
mod error;
mod operator;
mod payments;
mod rentals;
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
        Command::BoundUsers => with_store(&mut |store| operator::bound_users(store, out)),
        Command::Bind { account, user } => {
            with_store(&mut |store| operator::bind(store, clock, &account, &user, out))
        }
        Command::Unbind { account } => {
            with_store(&mut |store| operator::unbind(store, &account, out))
        }
        Command::GrantOperator { account } => {
            with_store(&mut |store| operator::grant_operator(store, clock, &account, out))
        }
        Command::RevokeOperator { account } => {
            with_store(&mut |store| operator::revoke_operator(store, &account, out))
        }
        Command::Import { file } => {
            with_store(&mut |store| operator::import(store, clock, &file, out))
        }
        Command::ImportProfiles { file } => {
            with_store(&mut |store| operator::import_profiles(store, clock, &file, out))
        }
        Command::ImportProfile { account, file } => {
            with_store(&mut |store| operator::import_profile(store, clock, &account, &file, out))
        }
        Command::ImportRelease { file } => {
            with_store(&mut |store| operator::import_release(store, clock, &file, out))
        }
        Command::DeleteAccount { account } => {
            with_store(&mut |store| operator::delete_account(store, &account, out))
        }
        Command::Ban { account, cards } => {
            with_store(&mut |store| operator::ban(store, clock, &account, &cards, out))
        }
        Command::BanCards { email, cards } => {
            with_store(&mut |store| payments::ban_cards(store, &email, &cards, out))
        }
        Command::Unban { email } => with_store(&mut |store| operator::unban(store, &email, out)),
        Command::Orders => with_store(&mut |store| payments::orders(store, out)),
        Command::ApproveOrder { order } => {
            with_store(&mut |store| payments::approve_order(store, clock, &order, out))
        }
        Command::DismissPayment {
            id,
            subscription_ended,
        } => with_store(&mut |store| {
            payments::dismiss_payment(store, id, subscription_ended.as_ref(), out)
        }),
        Command::DiscardTestPayments => {
            with_store(&mut |store| payments::discard_test_payments(store, out))
        }
        Command::Bookings => with_store(&mut |store| rentals::bookings(store, out)),
        Command::CloseBooking { id } => {
            with_store(&mut |store| rentals::close_booking(store, id, out))
        }
        Command::PullToken => with_store(&mut |store| payments::pull_token(store, clock, out)),
        Command::RevokePullToken => {
            with_store(&mut |store| payments::revoke_pull_token(store, out))
        }
        Command::EraseDeleted => with_store(&mut |store| operator::erase_deleted(store, out)),
    }
}

#[cfg(test)]
mod tests;
