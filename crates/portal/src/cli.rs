//! The command line.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use portal_core::account::{AccountCode, AccountCodeError, Email, EmailError};
use portal_core::payment::stripe::{
    CardFingerprint, CardFingerprintError, StripeId, StripeIdError,
};
use portal_core::payment::{OrderId, OrderIdError};
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
    /// Import a CFM profile bundle that Mooring exported for one user
    /// (`mooringctl user export-profiles --user <id>`), for the account
    /// bound to that user. No other account's profile changes.
    ImportProfile {
        /// The account's code; it must be bound to the bundle's user.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
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
    /// Delete an account with its sessions, binding, record, profile,
    /// link, orders and Mac bookings. Refused while Stripe may still
    /// charge a subscription of it, or an order of it is not settled.
    DeleteAccount {
        /// The account's code.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
    },
    /// Ban an account caught abusing the service: delete it as
    /// `delete-account` does, without waiting for its payments to be
    /// settled, and keep its email address from registering again and
    /// its cards from paying. Revoke its Mooring user in Mooring as well.
    Ban {
        /// The account's code.
        #[arg(long, value_name = "CODE", value_parser = account_code)]
        account: AccountCode,
        /// The fingerprint Stripe gives a card the account paid with, as
        /// Stripe's Dashboard shows it; repeat for each card.
        #[arg(long = "card", value_name = "FINGERPRINT", value_parser = card_fingerprint)]
        cards: Vec<CardFingerprint>,
    },
    /// Add cards to the ban of an email address already banned.
    BanCards {
        /// The banned address, as it was registered.
        #[arg(long, value_name = "ADDRESS", value_parser = email_address)]
        email: Email,
        /// A card's fingerprint, as Stripe's Dashboard shows it; repeat
        /// for each card.
        #[arg(
            long = "card",
            value_name = "FINGERPRINT",
            value_parser = card_fingerprint,
            required = true
        )]
        cards: Vec<CardFingerprint>,
    },
    /// Let a banned email address register again, and the cards banned
    /// with it pay again.
    Unban {
        /// The address, as it was registered (case does not matter).
        #[arg(long, value_name = "ADDRESS", value_parser = email_address)]
        email: Email,
    },
    /// Show what about payments waits for the operator or for Stripe,
    /// the subscriptions Stripe may charge again, the latest orders, and
    /// how far Mooring has pulled.
    Orders,
    /// Let an order through the one thing it waits for, which `orders`
    /// shows. An order in review for its checkout is judged by its card
    /// next; one whose card could not be checked, or was never told of,
    /// is entered for Mooring to grant. One paid with a banned account's
    /// card is never approved.
    ApproveOrder {
        /// The order's id, as `orders` lists it.
        #[arg(long, value_name = "ID", value_parser = order_id)]
        order: OrderId,
    },
    /// Take a payment that is no order's off the list `orders` shows.
    /// One whose subscription no order began stays while Stripe may
    /// charge that subscription again: cancel it in Stripe and wait for
    /// Stripe to report that it ended. A full refund then takes the
    /// payment off the list by itself.
    DismissPayment {
        /// The payment's number, as `orders` lists it.
        #[arg(long, value_name = "NUMBER")]
        id: u64,
        /// Say that the payment's subscription has ended, when Stripe's
        /// report of that never came: only after seeing at Stripe that it
        /// has. If it has not, its next charge is listed again.
        #[arg(long, value_name = "SUBSCRIPTION", value_parser = stripe_id)]
        subscription_ended: Option<StripeId>,
    },
    /// Remove a rehearsal before taking real payments: every order,
    /// every payment that is no order's and all that Stripe said, from a
    /// database that has only been served with a catalogue of Stripe's
    /// test mode. Refused for a database of real payments.
    DiscardTestPayments,
    /// List the Mac rental bookings, oldest first, with the account that
    /// made each and the address to write to its owner at.
    Bookings,
    /// Close a Mac rental booking once it is dealt with (its machine is
    /// ordered, or its owner has said no): delete it, and erase it from
    /// the database files.
    CloseBooking {
        /// The booking's number, as `bookings` lists it.
        #[arg(long, value_name = "NUMBER")]
        id: u64,
    },
    /// Make the token Mooring pulls the entitlements with, in place of
    /// any made before. It is printed once and never stored.
    PullToken,
    /// Remove the token Mooring pulls with.
    RevokePullToken,
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
    /// The catalogue: what the portal sells and on which of Stripe's
    /// payment pages (`docs/payments.md`). Only its owner may be able to
    /// change it. Without it the portal sells nothing, and does not start
    /// over a database with payments that are not settled.
    #[arg(long, value_name = "FILE", requires = "stripe_webhook_secret")]
    pub(crate) payments: Option<PathBuf>,
    /// A file holding the signing secret of the portal's webhook endpoint
    /// at Stripe (`whsec_...`), or two, a line each, while one is being
    /// rolled. Only its owner may be able to read it.
    #[arg(long, value_name = "FILE", requires = "payments")]
    pub(crate) stripe_webhook_secret: Option<PathBuf>,
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

fn order_id(text: &str) -> Result<OrderId, OrderIdError> {
    OrderId::parse(text)
}

fn card_fingerprint(text: &str) -> Result<CardFingerprint, CardFingerprintError> {
    CardFingerprint::parse(text)
}

fn stripe_id(text: &str) -> Result<StripeId, StripeIdError> {
    StripeId::parse(text)
}
