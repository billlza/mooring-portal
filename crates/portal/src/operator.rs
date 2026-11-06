//! The operator's commands on the portal database.

use std::fs::File;
use std::io::{Read as _, Write};
use std::path::Path;

use portal_core::account::AccountCode;
use portal_core::snapshot::{MAX_DOCUMENT_BYTES, MooringUserId, Snapshot};
use portal_core::time::Clock;
use portal_store::Store;

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
        "unbound account {account} from Mooring user {user}; its subscription record was deleted"
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
    let document = read_bounded(file)?;
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
    for (account, user) in &report.missing {
        writeln!(
            out,
            "warning: account {account} is bound to Mooring user {user}, which this snapshot does not list"
        )?;
    }
    Ok(())
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
        "deleted account {account} with its sessions, binding and subscription record"
    )?;
    Ok(())
}

/// One byte more than a snapshot may have: reading that many tells an
/// oversized file apart without reading it all. A mebibyte fits any
/// `u64`, so the conversion is exact.
const READ_LIMIT: u64 = MAX_DOCUMENT_BYTES as u64 + 1;

/// Reads at most [`READ_LIMIT`] bytes of `file`; `Snapshot::parse` refuses
/// a document that fills them.
fn read_bounded(file: &Path) -> Result<Vec<u8>, CommandError> {
    let read_error = |source| CommandError::Read {
        path: file.to_owned(),
        source,
    };
    let mut document = Vec::new();
    File::open(file)
        .map_err(read_error)?
        .take(READ_LIMIT)
        .read_to_end(&mut document)
        .map_err(read_error)?;
    Ok(document)
}
