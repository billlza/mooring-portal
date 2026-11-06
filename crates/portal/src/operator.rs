//! The operator's commands on the portal database.

use std::fs::File;
use std::io::{Read as _, Write};
use std::path::Path;

use portal_core::account::{AccountCode, Email};
use portal_core::profile::{MAX_BUNDLE_BYTES, ProfileBundle};
use portal_core::snapshot::{MAX_DOCUMENT_BYTES, MooringUserId, Snapshot};
use portal_core::time::Clock;
use portal_store::{Store, WithheldLink};

use crate::CommandError;

/// Lists every account, oldest first.
pub(crate) fn accounts(store: &Store, out: &mut dyn Write) -> Result<(), CommandError> {
    let accounts = store.accounts()?;
    writeln!(
        out,
        "{:<14}  {:<20}  {:<12}  EMAIL",
        "CODE", "CREATED", "MOORING USER"
    )?;
    for account in &accounts {
        let user = account
            .binding
            .as_ref()
            .map_or("-", |binding| binding.user.as_str());
        writeln!(
            out,
            "{:<14}  {:<20}  {:<12}  {}",
            account.code.to_string(),
            account.created_at.to_string(),
            user,
            account.email.as_str()
        )?;
    }
    writeln!(out, "{} account(s)", accounts.len())?;
    Ok(())
}

/// Binds an account to a Mooring user.
pub(crate) fn bind(
    store: &mut Store,
    clock: &dyn Clock,
    account: &AccountCode,
    user: &MooringUserId,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    store.bind(account, user, clock.now()?)?;
    writeln!(out, "bound account {account} to Mooring user {user}")?;
    Ok(())
}

/// Unbinds an account.
pub(crate) fn unbind(
    store: &mut Store,
    account: &AccountCode,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let user = store.unbind(account)?;
    writeln!(
        out,
        "unbound account {account} from Mooring user {user}; its subscription record, CFM \
         profile and subscription link were deleted"
    )?;
    Ok(())
}

/// Imports a snapshot file.
pub(crate) fn import(
    store: &mut Store,
    clock: &dyn Clock,
    file: &Path,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let document = read_bounded(file, SNAPSHOT_READ_LIMIT)?;
    let snapshot = Snapshot::parse(&document).map_err(|source| CommandError::Snapshot {
        path: file.to_owned(),
        source,
    })?;
    let report = store.import(&snapshot, clock.now()?)?;
    let stored: Vec<&str> = report.stored.iter().map(MooringUserId::as_str).collect();
    writeln!(
        out,
        "snapshot generated at {}: stored {} record(s){}{}",
        snapshot.generated_at(),
        stored.len(),
        if stored.is_empty() { "" } else { ": " },
        stored.join(", ")
    )?;
    if report.unbound > 0 {
        writeln!(
            out,
            "not kept: {} user(s) no account is bound to",
            report.unbound
        )?;
    }
    for missing in &report.missing {
        writeln!(
            out,
            "warning: account {} is bound to Mooring user {}, which this snapshot does not list",
            missing.account, missing.user
        )?;
    }
    Ok(())
}

/// Imports a CFM profile bundle file.
pub(crate) fn import_profiles(
    store: &mut Store,
    clock: &dyn Clock,
    file: &Path,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let document = read_bounded(file, BUNDLE_READ_LIMIT)?;
    let bundle = ProfileBundle::parse(&document).map_err(|source| CommandError::Profiles {
        path: file.to_owned(),
        source,
    })?;
    let report = store.import_profiles(&bundle, clock.now()?)?;
    let stored: Vec<&str> = report.stored.iter().map(MooringUserId::as_str).collect();
    writeln!(
        out,
        "profile bundle generated at {}: stored {} profile(s){}{}",
        bundle.generated_at(),
        stored.len(),
        if stored.is_empty() { "" } else { ": " },
        stored.join(", ")
    )?;
    for withheld in &report.withheld {
        writeln!(
            out,
            "withheld by Mooring ({}): account {} has no profile now{}",
            withheld.reason,
            withheld.account,
            match withheld.link {
                WithheldLink::NoLink => "",
                WithheldLink::Paused => {
                    ", and its subscription link is paused until Mooring delivers to the user again"
                }
                WithheldLink::Ended => ", and its subscription link has ended for good",
            }
        )?;
    }
    if !report.unbound_profiles.is_empty() {
        writeln!(
            out,
            "warning: no account is bound to {}: their profiles were not kept, but their \
             credentials passed through this host. Delete the bundle file now, and leave these \
             users out of the next export",
            ids(&report.unbound_profiles)
        )?;
    }
    if !report.unbound_withheld.is_empty() {
        writeln!(
            out,
            "warning: no account is bound to {}, whom the bundle withholds; leave these users out \
             of the next export",
            ids(&report.unbound_withheld)
        )?;
    }
    Ok(())
}

/// Prints, on one line in id order, `--user <id>` for every bound account's
/// Mooring user: the arguments that name them to Mooring's exports. Ids
/// are lowercase base32, so the line is safe to paste or substitute
/// unquoted.
pub(crate) fn bound_users(store: &Store, out: &mut dyn Write) -> Result<(), CommandError> {
    let mut users: Vec<MooringUserId> = store
        .accounts()?
        .into_iter()
        .filter_map(|account| account.binding.map(|binding| binding.user))
        .collect();
    users.sort();
    let arguments: Vec<String> = users.iter().map(|user| format!("--user {user}")).collect();
    writeln!(out, "{}", arguments.join(" "))?;
    Ok(())
}

/// `users`, comma-separated.
fn ids(users: &[MooringUserId]) -> String {
    let ids: Vec<&str> = users.iter().map(MooringUserId::as_str).collect();
    ids.join(", ")
}

/// Deletes an account.
pub(crate) fn delete_account(
    store: &mut Store,
    account: &AccountCode,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    store.delete_account(account)?;
    writeln!(
        out,
        "deleted account {account} with its sessions, binding, subscription record, CFM \
         profile and subscription link"
    )?;
    Ok(())
}

/// Bans an account.
pub(crate) fn ban(
    store: &mut Store,
    clock: &dyn Clock,
    account: &AccountCode,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let ban = store.ban(account, clock.now()?)?;
    writeln!(
        out,
        "banned account {account}: deleted it with its sessions, binding, subscription record, \
         CFM profile and subscription link{}; its email address cannot register again",
        if ban.link_ended {
            " (the link has ended for good)"
        } else {
            ""
        }
    )?;
    if let Some(user) = &ban.user {
        writeln!(
            out,
            "it was bound to Mooring user {user}: revoke that user in Mooring too, since the \
             nodes serve it until then"
        )?;
    }
    Ok(())
}

/// Lets a banned address register again.
pub(crate) fn unban(
    store: &mut Store,
    email: &Email,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    if !store.unban(email)? {
        return Err(CommandError::NotBanned);
    }
    writeln!(out, "the email address can register again")?;
    Ok(())
}

/// One byte more than a snapshot may have: reading that many tells an
/// oversized file apart without reading it all. A mebibyte fits any
/// `u64`, so the conversion is exact.
const SNAPSHOT_READ_LIMIT: u64 = MAX_DOCUMENT_BYTES as u64 + 1;
/// One byte more than a profile bundle may have, for the same reason.
const BUNDLE_READ_LIMIT: u64 = MAX_BUNDLE_BYTES as u64 + 1;

/// Reads at most `limit` bytes of `file`; the parser refuses a document
/// that fills them.
fn read_bounded(file: &Path, limit: u64) -> Result<Vec<u8>, CommandError> {
    let read_error = |source| CommandError::Read {
        path: file.to_owned(),
        source,
    };
    let mut document = Vec::new();
    File::open(file)
        .map_err(read_error)?
        .take(limit)
        .read_to_end(&mut document)
        .map_err(read_error)?;
    Ok(document)
}
