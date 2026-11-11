//! Payments from the operator's side: what `serve` reads to take orders,
//! and the commands on orders, stray payments and Mooring's token.

use std::fs::File;
use std::io::{Read as _, Write};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use portal_core::account::Email;
use portal_core::auth::{BanDigest, SecretToken};
use portal_core::payment::stripe::{CardFingerprint, SigningSecret, StripeId};
use portal_core::payment::{
    CardVerdict, Catalogue, Currency, MAX_CATALOGUE_BYTES, OrderId, OrderTerms, ReviewReason,
};
use portal_core::time::Clock;
use portal_store::{OperatorOrder, Progress, Store, StrayPayment, StrayReason};
use portal_web::Payments;

use crate::CommandError;

/// The latest orders `orders` lists.
const LATEST_ORDERS: u32 = 20;
/// The most a file of signing secrets may hold: two secrets fit many
/// times over.
pub(crate) const MAX_SECRET_FILE_BYTES: usize = 4_096;

/// Reads the catalogue and the signing secrets `serve` was given.
///
/// The catalogue must be a file that only its owner can change: whoever
/// writes it decides what is sold, for how much, and on whose payment
/// pages.
pub(crate) fn load(catalogue: &Path, secrets: &Path) -> Result<Payments, CommandError> {
    let (file, mode) = open(catalogue)?;
    if mode & 0o022 != 0 {
        return Err(CommandError::CatalogueFileOpen {
            path: catalogue.to_owned(),
            mode,
        });
    }
    let document = read(file, catalogue, MAX_CATALOGUE_BYTES + 1)?;
    let document = String::from_utf8(document).map_err(|_| CommandError::Catalogue {
        path: catalogue.to_owned(),
        source: portal_core::payment::CatalogueError::Malformed(
            portal_core::json::Malformation::Syntax,
        ),
    })?;
    let catalogue_read = Catalogue::parse(&document).map_err(|source| CommandError::Catalogue {
        path: catalogue.to_owned(),
        source,
    })?;
    Payments::new(catalogue_read, signing_secrets(secrets)?).map_err(|source| {
        CommandError::SecretCount {
            path: secrets.to_owned(),
            source,
        }
    })
}

/// Opens the file at `path`, and gives it with its permission bits.
fn open(path: &Path) -> Result<(File, u32), CommandError> {
    let read_error = |source| CommandError::Read {
        path: path.to_owned(),
        source,
    };
    let file = File::open(path).map_err(read_error)?;
    let mode = file.metadata().map_err(read_error)?.permissions().mode();
    Ok((file, mode & 0o7777))
}

/// Reads at most `limit` bytes of `file`, which was opened at `path`.
fn read(file: File, path: &Path, limit: usize) -> Result<Vec<u8>, CommandError> {
    let mut bytes = Vec::new();
    file.take(limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| CommandError::Read {
            path: path.to_owned(),
            source,
        })?;
    Ok(bytes)
}

/// The signing secrets in the file at `path`, a line each. The file must
/// be its owner's alone, as the database is: whoever reads a secret can
/// tell the portal that anything was paid. A file longer than any file
/// of secrets is refused rather than read in part.
fn signing_secrets(path: &Path) -> Result<Vec<SigningSecret>, CommandError> {
    let (file, mode) = open(path)?;
    if mode & 0o077 != 0 {
        return Err(CommandError::SecretFileOpen {
            path: path.to_owned(),
            mode,
        });
    }
    let bytes = read(file, path, MAX_SECRET_FILE_BYTES + 1)?;
    if bytes.len() > MAX_SECRET_FILE_BYTES {
        return Err(CommandError::SecretFileTooLarge {
            path: path.to_owned(),
        });
    }
    let text = String::from_utf8(bytes).map_err(|_| CommandError::SecretFileNotText {
        path: path.to_owned(),
    })?;
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .map(|(line, secret)| {
            SigningSecret::parse(secret).map_err(|source| CommandError::SigningSecret {
                path: path.to_owned(),
                line,
                source,
            })
        })
        .collect()
}

/// Makes Mooring's pull token and prints it, once.
pub(crate) fn pull_token(
    store: &mut Store,
    clock: &dyn Clock,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let token = SecretToken::generate()?;
    store.set_pull_token(&token.digest(), clock.now()?)?;
    writeln!(
        out,
        "Mooring's pull token, shown only this once (any token made before no longer works):"
    )?;
    writeln!(out, "{}", token.encode())?;
    Ok(())
}

/// Removes Mooring's pull token.
pub(crate) fn revoke_pull_token(
    store: &mut Store,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    if !store.revoke_pull_token()? {
        return Err(CommandError::NoPullToken);
    }
    writeln!(
        out,
        "revoked Mooring's pull token; Mooring cannot pull until a new one is made"
    )?;
    Ok(())
}

/// Lets an order through the one thing it waits for, and says where that
/// leaves it.
pub(crate) fn approve_order(
    store: &mut Store,
    clock: &dyn Clock,
    order: &OrderId,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let approved = store.approve_order(order, clock.now()?)?;
    writeln!(
        out,
        "approved order {order} of account {} ({})",
        approved.account,
        approved.waived.map_or(
            "Stripe has not told of its card, which is passed unchecked",
            why
        )
    )?;
    match approved.standing {
        Progress::Granted { .. } => writeln!(out, "it is entered for Mooring to grant")?,
        Progress::Review(reason) => writeln!(
            out,
            "it is not granted: it waits for you again, because {} (`orders` says what to do)",
            why(reason)
        )?,
        _ => writeln!(
            out,
            "it is not granted yet: it waits for Stripe to tell of its card (`orders` shows it)"
        )?,
    }
    Ok(())
}

/// Removes a rehearsal's orders and payments, and erases their copies.
pub(crate) fn discard_test_payments(
    store: &mut Store,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let discarded = store.discard_test_payments()?;
    crate::operator::erase_then_report(store, out, |out| {
        writeln!(
            out,
            "discarded {} order(s) and {} payment(s) that were no order's, all of Stripe's test \
             mode, with what Stripe said of them; the accounts, the ban list and Mooring's pull \
             token stay",
            discarded.orders, discarded.stray_payments
        )
    })
}

/// Takes a stray payment off the list. Its row named a payment at Stripe,
/// so its copies are erased as an account's are.
pub(crate) fn dismiss_payment(
    store: &mut Store,
    id: u64,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    if !store.dismiss_stray(id)? {
        return Err(CommandError::NoSuchPayment(id));
    }
    crate::operator::erase_then_report(store, out, |out| {
        writeln!(out, "took payment #{id} off the list")
    })
}

/// Adds cards to an existing ban.
pub(crate) fn ban_cards(
    store: &mut Store,
    email: &Email,
    cards: &[CardFingerprint],
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let digests: Vec<_> = cards.iter().map(CardFingerprint::digest).collect();
    if !store.ban_cards(&BanDigest::of(email)?, &digests)? {
        return Err(CommandError::NotBanned);
    }
    writeln!(
        out,
        "kept {} card fingerprint(s) with the ban of that email address: payments made with \
         them will not be granted",
        cards.len()
    )?;
    Ok(())
}

/// An amount with its currency's code: `8.00 gbp`.
fn amount(hundredths: u64, currency: Currency) -> String {
    format!(
        "{}.{:02} {}",
        hundredths / 100,
        hundredths % 100,
        currency.as_str()
    )
}

/// An order's plan and price.
fn bought(terms: &OrderTerms) -> String {
    format!("{}  {}", terms.plan, amount(terms.amount, terms.currency))
}

/// Why an order waits for the operator.
const fn why(reason: ReviewReason) -> &'static str {
    match reason {
        ReviewReason::Unpaid => "its checkout was completed without a payment",
        ReviewReason::Link => "it was paid on another payment page than its plan's",
        ReviewReason::Mode => {
            "its checkout is not the kind its plan asks for: one payment, or a subscription"
        }
        ReviewReason::Amount => "what was paid is not its price, or not in its currency",
        ReviewReason::Terms => {
            "it was paid as ordered, but its plan is no longer sold on the terms it was ordered on"
        }
        ReviewReason::CardBanned => "it was paid with a card of a banned account",
        ReviewReason::CardUnchecked => {
            "it was paid from a wallet or by another method, so its card could not be checked \
             against the ban list"
        }
        ReviewReason::Binding => "its account is not bound to the Mooring user it names",
    }
}

/// Why a payment is no order's.
const fn astray(reason: StrayReason) -> &'static str {
    match reason {
        StrayReason::NoOrder => "its checkout names no order of the portal",
        StrayReason::PaidAgain => "its checkout names an order that another checkout had paid",
        StrayReason::NoSubscription => "it renews a subscription that no order began",
        StrayReason::OtherInvoice => {
            "it is an invoice that neither began a subscription nor renewed one"
        }
    }
}

/// Where an order stands, in a word.
const fn standing(progress: Progress) -> &'static str {
    match progress {
        Progress::Pending => "unpaid",
        Progress::Checking => "checking",
        Progress::Granted { pulled: false } => "granted",
        Progress::Granted { pulled: true } => "pulled",
        Progress::Review(_) => "review",
        Progress::Returned => "returned",
    }
}

/// The ids Stripe's Dashboard finds a payment by, those there are.
fn at_stripe(ids: [(&str, Option<&StripeId>); 4]) -> String {
    let known: Vec<String> = ids
        .iter()
        .filter_map(|(what, id)| id.map(|id| format!("{what} {id}")))
        .collect();
    known.join(", ")
}

fn order_at_stripe(listed: &OperatorOrder) -> String {
    at_stripe([
        ("checkout", listed.session.as_ref()),
        ("payment", listed.payment_intent.as_ref()),
        ("invoice", listed.invoice.as_ref()),
        ("subscription", listed.subscription.as_ref()),
    ])
}

/// What approving an order in review for its checkout would lead to: its
/// card is judged next.
const fn after_approval(card: Option<CardVerdict>) -> &'static str {
    match card {
        Some(CardVerdict::Clear) => {
            "its card has passed the ban list: let through, it is entered for Mooring to grant"
        }
        Some(CardVerdict::Banned) => {
            "it was also paid with a card of a banned account: let through, it would wait for \
             you again, and such a payment is never granted"
        }
        Some(CardVerdict::Unchecked) => {
            "it was also paid from a wallet or by another method: let through, it will wait for \
             you again, for its card"
        }
        None => "Stripe has not told of its card: let through, it will wait for that",
    }
}

/// What to do about an order that waits, and for whom it waits.
fn waiting(listed: &OperatorOrder, out: &mut dyn Write) -> std::io::Result<()> {
    let order = &listed.order;
    let id = order.id;
    writeln!(
        out,
        "  order {id}  account {}  {}",
        listed.account,
        bought(&order.terms)
    )?;
    writeln!(out, "    at Stripe: {}", order_at_stripe(listed))?;
    // Whatever else it waited for: a disputed payment is neither approved
    // nor, at Stripe, refunded.
    if order.disputed {
        return writeln!(
            out,
            "    its payer disputes the payment, which was never granted: answer the dispute at \
             Stripe; there is nothing to do here"
        );
    }
    match order.progress {
        Progress::Review(reason) => {
            writeln!(out, "    waits for you: {}", why(reason))?;
            match reason {
                ReviewReason::CardBanned => writeln!(
                    out,
                    "    refund it in full at Stripe: such a payment is never granted"
                ),
                ReviewReason::CardUnchecked => writeln!(
                    out,
                    "    refund it in full at Stripe, or pass its card unchecked with \
                     `approve-order --order {id}`"
                ),
                ReviewReason::Binding => writeln!(
                    out,
                    "    refund it in full at Stripe, or bind the account to that user again and \
                     then let it through with `approve-order --order {id}`"
                ),
                ReviewReason::Unpaid
                | ReviewReason::Link
                | ReviewReason::Mode
                | ReviewReason::Amount
                | ReviewReason::Terms => {
                    writeln!(out, "    {}", after_approval(listed.card))?;
                    writeln!(
                        out,
                        "    refund it in full at Stripe, or let it through with `approve-order \
                         --order {id}`"
                    )
                }
            }
        }
        _ => {
            if listed.payment_intent.is_none() {
                writeln!(
                    out,
                    "    waits for Stripe to tell which payment paid its invoice (is \
                     invoice_payment.paid among the endpoint's events?)"
                )?;
            } else {
                writeln!(
                    out,
                    "    waits for Stripe to tell of its charge (is charge.succeeded among the \
                     endpoint's events?)"
                )?;
            }
            writeln!(
                out,
                "    Stripe's Dashboard can send an event again; if it never comes, \
                 `approve-order --order {id}` passes the card unchecked"
            )
        }
    }
}

fn stray(payment: &StrayPayment, out: &mut dyn Write) -> std::io::Result<()> {
    let paid = match (payment.amount, payment.currency) {
        (Some(hundredths), Some(currency)) => amount(hundredths, currency),
        (Some(hundredths), None) => format!("{hundredths} hundredths of another currency"),
        (None, _) => "an amount Stripe did not say".to_owned(),
    };
    writeln!(
        out,
        "  payment #{}  noticed {}  {paid}",
        payment.id, payment.noticed_at
    )?;
    writeln!(
        out,
        "    at Stripe: {}",
        at_stripe([
            ("checkout", payment.session.as_ref()),
            ("payment", payment.payment_intent.as_ref()),
            ("invoice", payment.invoice.as_ref()),
            ("subscription", payment.subscription.as_ref()),
        ])
    )?;
    writeln!(out, "    no order's: {}", astray(payment.reason))?;
    if payment.card == Some(CardVerdict::Banned) {
        writeln!(out, "    it was paid with a card of a banned account")?;
    }
    if payment.refunded > 0 {
        writeln!(
            out,
            "    {} hundredths of it are refunded",
            payment.refunded
        )?;
    }
    writeln!(
        out,
        "    refund it in full at Stripe, which takes it off this list, or take it off with \
         `dismiss-payment --id {}`",
        payment.id
    )
}

/// `orders`: everything about payments that the operator may have to act
/// on, then the latest orders and how far Mooring has pulled.
pub(crate) fn orders(store: &Store, out: &mut dyn Write) -> Result<(), CommandError> {
    let waiting_orders = store.orders_waiting()?;
    let strays = store.stray_payments()?;
    writeln!(
        out,
        "Waiting ({} order(s), {} payment(s) that are no order's)",
        waiting_orders.len(),
        strays.len()
    )?;
    for listed in &waiting_orders {
        waiting(listed, out)?;
    }
    for payment in &strays {
        stray(payment, out)?;
    }
    let renewals = store.open_renewals()?;
    writeln!(
        out,
        "Subscriptions Stripe may charge again ({})",
        renewals.len()
    )?;
    for renewal in &renewals {
        writeln!(
            out,
            "  {}  account {}  {}  {}",
            renewal.subscription,
            renewal.account,
            renewal.plan,
            renewal.state.as_str()
        )?;
    }
    let unmatched = store.unmatched_payments()?;
    writeln!(
        out,
        "Refunds and disputes of payments no order names ({})",
        unmatched.len()
    )?;
    for payment in &unmatched {
        writeln!(
            out,
            "  payment {}  {} hundredths refunded{}",
            payment.payment_intent,
            payment.refunded,
            payment
                .disputed
                .map_or_else(String::new, |disputed| format!(
                    ", {disputed} hundredths disputed"
                ))
        )?;
    }
    let latest = store.recent_orders(LATEST_ORDERS)?;
    writeln!(out, "Latest orders ({})", latest.len())?;
    for listed in &latest {
        let order = &listed.order;
        writeln!(
            out,
            "  {}  {}  {}  {:<8}  {}",
            order.id,
            listed.account,
            order.created_at,
            standing(order.progress),
            bought(&order.terms)
        )?;
    }
    let pull = store.pull_state()?;
    match pull.token_created_at {
        Some(made) => writeln!(out, "Mooring's pull token was made {made}")?,
        None => writeln!(
            out,
            "Mooring has no pull token and cannot pull; make one with `pull-token`"
        )?,
    }
    writeln!(
        out,
        "Mooring has pulled through entry {} of {}{}",
        pull.pulled_through,
        pull.last_seq,
        pull.pulled_at.map_or_else(
            || ", and has never pulled".to_owned(),
            |at| format!(", last at {at}")
        )
    )?;
    Ok(())
}
