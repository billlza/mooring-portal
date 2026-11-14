//! CFM profiles, one per bound Mooring user, imported from bundles.
//!
//! The operator names every bound account's Mooring user when exporting a
//! bundle, and Mooring delivers a profile to each named user or withholds
//! the user (paused, revoked, or not yet agreed to the current user
//! agreement). An import replaces every stored profile with the bundle's
//! profiles of bound users, so a withheld user stops being served. What
//! becomes of the account's subscription link depends on why: a user
//! withheld for a while (suspended, or not yet agreed) keeps the link,
//! which serves nothing until a bundle delivers to the user again and then
//! works as before; a revoked user's link ends for good, so that the user
//! being served again (as a new user) does not bring an old link back.
//!
//! A bundle that does not name a bound account's user at all is refused
//! whole: that is a slip in the operator's list, not a decision to stop
//! serving the account, so it must not end the account's link.
//!
//! The operator can also import a bundle Mooring exported for one user,
//! for the account bound to that user ([`Store::import_profile_for`]). It
//! does to that account what a full import would, and leaves every other
//! account as it was. Each profile keeps the time of the bundle it came
//! in, so the others' times stay true.
//!
//! Imports of either kind keep their order: a bundle from the future, or
//! older than the last one imported, is refused whole, so an old bundle
//! never brings back a profile that a newer one replaced.
//!
//! A bound account whose user a bundle withholds keeps that bundle's time
//! until a bundle delivers to the user again. A payment made since then
//! waits for a profile Mooring has yet to make ([`Store::cfm_delivery`]).

use portal_core::account::AccountCode;
use portal_core::profile::{CfmProfile, Named, ProfileBundle, WithheldReason};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::bindings::{BoundAccount, account_by_code, bound_accounts};
use crate::rows::{corrupt, time_from_sql, time_to_sql};
use crate::subscriptions::MAX_CLOCK_SKEW_SECS;
use crate::{AccountId, Store, StoreError, UnknownAccount};

/// What a profile import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileImportReport {
    /// The bound users whose profiles were stored, in id order.
    pub stored: Vec<MooringUserId>,
    /// Bound accounts whose Mooring user the bundle withholds, in user id
    /// order.
    pub withheld: Vec<Withheld>,
    /// Users the bundle delivers a profile to that no account is bound to,
    /// in id order: their profiles were not kept, but their credentials
    /// passed through this host.
    pub unbound_profiles: Vec<MooringUserId>,
    /// Users the bundle withholds that no account is bound to, in id order.
    pub unbound_withheld: Vec<MooringUserId>,
}

/// A bound account whose Mooring user a bundle withholds: it has no
/// profile now, and no subscription link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withheld {
    /// The account and its Mooring user.
    pub account: BoundAccount,
    /// Why Mooring withholds the user.
    pub reason: WithheldReason,
    /// What became of the account's subscription link.
    pub link: WithheldLink,
}

/// What an import did to a withheld account's subscription link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithheldLink {
    /// The account has no link.
    NoLink,
    /// The link stays, serving nothing until a bundle delivers to the user
    /// again: the user is withheld for a passing reason.
    Paused,
    /// The link ended for good: the user is revoked.
    Ended,
}

/// What an import for one account did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountProfileImport {
    /// The bundle delivers the user's profile: the account has it now, in
    /// place of any it had.
    Stored(BoundAccount),
    /// The bundle withholds the user: the account has no profile now.
    Withheld(Withheld),
}

/// Why a bundle's time keeps it out.
#[derive(Debug, thiserror::Error)]
pub enum BundleTimeError {
    /// Generated later than this host's clock allows.
    #[error(
        "the bundle was generated at {generated_at}, more than {MAX_CLOCK_SKEW_SECS} s after \
         this host's clock ({now}); check both clocks"
    )]
    FromTheFuture {
        /// When the bundle says it was generated.
        generated_at: UnixTime,
        /// This host's time.
        now: UnixTime,
    },
    /// Older than the last bundle imported.
    #[error(
        "the last bundle imported was generated at {stored}, after this one ({incoming}); \
         import a newer bundle"
    )]
    Older {
        /// When the last imported bundle was generated.
        stored: UnixTime,
        /// When the refused bundle was generated.
        incoming: UnixTime,
    },
}

/// Why a bundle was not imported.
#[derive(Debug, thiserror::Error)]
pub enum ProfileImportError {
    /// Its time keeps it out.
    #[error(transparent)]
    Time(#[from] BundleTimeError),
    /// The bundle neither delivers to nor withholds the Mooring users of
    /// some bound accounts: most likely the operator left them out of the
    /// export. Nothing was imported.
    #[error(
        "the bundle does not name the Mooring user of {}; nothing was imported. Export the \
         bundle again naming every bound account's user. Unbind an account only if it is not \
         to be served any more: unbinding deletes its subscription link",
        list(.accounts)
    )]
    LeavesOut {
        /// The accounts left out, in user id order; never empty.
        accounts: Vec<BoundAccount>,
    },
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Why a bundle for one account was not imported. Nothing changed.
#[derive(Debug, thiserror::Error)]
pub enum AccountProfileError {
    /// The bundle names more than one user: it is not one Mooring
    /// exported for one user.
    #[error(
        "the bundle names {count} Mooring users; nothing was imported. Export it for the \
         account's user alone (`mooringctl user export-profiles --user <id>`), or import a \
         bundle that names every bound account's user with `import-profiles`"
    )]
    NamesMany {
        /// How many users it names.
        count: usize,
    },
    /// No account has the code.
    #[error(transparent)]
    UnknownAccount(#[from] UnknownAccount),
    /// The account is not bound to a Mooring user.
    #[error("account {0} is not bound to a Mooring user; nothing was imported")]
    NotBound(AccountCode),
    /// The bundle names another user than the account's: the wrong file,
    /// or the wrong account.
    #[error(
        "the bundle names Mooring user {named}, but account {account} is bound to Mooring \
         user {bound}; nothing was imported"
    )]
    OtherUser {
        /// The account.
        account: AccountCode,
        /// Its Mooring user.
        bound: MooringUserId,
        /// The user the bundle names.
        named: MooringUserId,
    },
    /// Its time keeps it out.
    #[error(transparent)]
    Time(#[from] BundleTimeError),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `accounts`, comma-separated.
fn list(accounts: &[BoundAccount]) -> String {
    let named: Vec<String> = accounts.iter().map(BoundAccount::to_string).collect();
    named.join(", ")
}

impl From<rusqlite::Error> for ProfileImportError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

impl From<rusqlite::Error> for AccountProfileError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// A bound account's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProfile {
    /// The document CFM imports.
    pub profile: CfmProfile,
    /// When Mooring generated the bundle it came in.
    pub generated_at: UnixTime,
}

impl Store {
    /// Replaces the stored profiles with those of the bound users in
    /// `bundle`, which must name every bound account's Mooring user.
    pub fn import_profiles(
        &mut self,
        bundle: &ProfileBundle,
        now: UnixTime,
    ) -> Result<ProfileImportReport, ProfileImportError> {
        not_from_the_future(bundle.generated_at(), now)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        not_older::<ProfileImportError>(&tx, bundle.generated_at())?;
        let bound = bound_accounts(&tx)?;
        let left_out: Vec<BoundAccount> = bound
            .values()
            .filter(|(_, account)| !bundle.names(&account.user))
            .map(|(_, account)| account.clone())
            .collect();
        if !left_out.is_empty() {
            return Err(ProfileImportError::LeavesOut { accounts: left_out });
        }
        tx.execute("DELETE FROM cfm_profiles", [])?;
        let mut stored = Vec::new();
        let mut unbound_profiles = Vec::new();
        for (user, profile) in bundle.profiles() {
            if !bound.contains_key(user) {
                unbound_profiles.push(user.clone());
                continue;
            }
            store_profile(&tx, user, profile, bundle.generated_at())?;
            stored.push(user.clone());
        }
        let mut withheld = Vec::new();
        let mut unbound_withheld = Vec::new();
        for (user, reason) in bundle.withheld() {
            let Some((id, account)) = bound.get(user) else {
                unbound_withheld.push(user.clone());
                continue;
            };
            withheld.push(Withheld {
                account: account.clone(),
                reason,
                link: withhold(&tx, *id, reason, bundle.generated_at())?,
            });
        }
        record_bundle_time(&tx, bundle.generated_at())?;
        tx.commit()?;
        Ok(ProfileImportReport {
            stored,
            withheld,
            unbound_profiles,
            unbound_withheld,
        })
    }

    /// Imports `bundle`, which Mooring exported for one user, for the
    /// account with `code`, which must be bound to that user. It does to
    /// the account what [`Self::import_profiles`] would, and leaves every
    /// other account's profile and link as they were.
    pub fn import_profile_for(
        &mut self,
        code: &AccountCode,
        bundle: &ProfileBundle,
        now: UnixTime,
    ) -> Result<AccountProfileImport, AccountProfileError> {
        let Some((named, what)) = bundle.sole_user() else {
            return Err(AccountProfileError::NamesMany {
                count: bundle.named_count(),
            });
        };
        not_from_the_future(bundle.generated_at(), now)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((id, bound)) = account_by_code(&tx, code)? else {
            return Err(UnknownAccount(*code).into());
        };
        let Some(bound) = bound else {
            return Err(AccountProfileError::NotBound(*code));
        };
        if bound != *named {
            return Err(AccountProfileError::OtherUser {
                account: *code,
                bound,
                named: named.clone(),
            });
        }
        not_older::<AccountProfileError>(&tx, bundle.generated_at())?;
        let account = BoundAccount {
            account: *code,
            user: bound,
        };
        let imported = match what {
            Named::Delivered(profile) => {
                store_profile(&tx, &account.user, profile, bundle.generated_at())?;
                AccountProfileImport::Stored(account)
            }
            Named::Withheld(reason) => {
                tx.execute(
                    "DELETE FROM cfm_profiles WHERE mooring_user = ?1",
                    [account.user.as_str()],
                )?;
                let link = withhold(&tx, id, reason, bundle.generated_at())?;
                AccountProfileImport::Withheld(Withheld {
                    account,
                    reason,
                    link,
                })
            }
        };
        record_bundle_time(&tx, bundle.generated_at())?;
        tx.commit()?;
        Ok(imported)
    }

    /// The profile of `account`'s Mooring user, if the account is bound and
    /// a bundle delivered a profile to the user.
    pub fn cfm_profile(&self, account: AccountId) -> Result<Option<StoredProfile>, StoreError> {
        let found: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT p.document, p.generated_at FROM bindings b
                 JOIN cfm_profiles p ON p.mooring_user = b.mooring_user
                 WHERE b.account_id = ?1",
                [account.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((document, generated_at)) = found else {
            return Ok(None);
        };
        Ok(Some(StoredProfile {
            profile: stored_profile(&document)?,
            generated_at: time_from_sql(generated_at, "CFM profile time")?,
        }))
    }
}

/// Refuses a bundle generated later than this host's clock allows.
fn not_from_the_future(generated_at: UnixTime, now: UnixTime) -> Result<(), BundleTimeError> {
    let latest_allowed = now
        .checked_add_secs(MAX_CLOCK_SKEW_SECS)
        .unwrap_or(UnixTime::MAX);
    if generated_at > latest_allowed {
        return Err(BundleTimeError::FromTheFuture { generated_at, now });
    }
    Ok(())
}

/// Refuses, on `tx`, a bundle generated before the last one imported.
fn not_older<E>(tx: &Transaction<'_>, incoming: UnixTime) -> Result<(), E>
where
    E: From<BundleTimeError> + From<StoreError> + From<rusqlite::Error>,
{
    let last: Option<i64> = tx
        .query_row(
            "SELECT generated_at FROM profile_bundle WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(last) = last {
        let stored = time_from_sql(last, "profile bundle time")?;
        if stored > incoming {
            return Err(BundleTimeError::Older { stored, incoming }.into());
        }
    }
    Ok(())
}

/// Stores, on `tx`, `profile` as `user`'s, in place of any the user had,
/// with the time of the bundle it came in. The user is withheld no more.
fn store_profile(
    tx: &Transaction<'_>,
    user: &MooringUserId,
    profile: &CfmProfile,
    generated_at: UnixTime,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO cfm_profiles (mooring_user, document, generated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (mooring_user) DO UPDATE
         SET document = excluded.document, generated_at = excluded.generated_at",
        params![user.as_str(), profile.as_str(), time_to_sql(generated_at)],
    )?;
    tx.execute(
        "UPDATE bindings SET withheld_at = NULL WHERE mooring_user = ?1",
        [user.as_str()],
    )?;
    Ok(())
}

/// Withholds, on `tx`, the user of account `id` for `reason`, as the
/// bundle generated at `generated_at` does: records that bundle's time,
/// and says what became of the account's subscription link. A final
/// reason ends the link, a passing one keeps it, serving nothing while
/// there is no profile.
fn withhold(
    tx: &Transaction<'_>,
    id: AccountId,
    reason: WithheldReason,
    generated_at: UnixTime,
) -> rusqlite::Result<WithheldLink> {
    tx.execute(
        "UPDATE bindings SET withheld_at = ?2 WHERE account_id = ?1",
        params![id.0, time_to_sql(generated_at)],
    )?;
    let had_link = if reason.is_final() {
        tx.execute(
            "DELETE FROM subscription_links WHERE account_id = ?1",
            [id.0],
        )? > 0
    } else {
        tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM subscription_links WHERE account_id = ?1)",
            [id.0],
            |row| row.get(0),
        )?
    };
    Ok(match (had_link, reason.is_final()) {
        (false, _) => WithheldLink::NoLink,
        (true, false) => WithheldLink::Paused,
        (true, true) => WithheldLink::Ended,
    })
}

/// Records, on `tx`, the time of the bundle just imported: no bundle
/// generated before it is imported after it.
fn record_bundle_time(tx: &Transaction<'_>, generated_at: UnixTime) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO profile_bundle (id, generated_at) VALUES (1, ?1)
         ON CONFLICT (id) DO UPDATE SET generated_at = excluded.generated_at",
        [time_to_sql(generated_at)],
    )?;
    Ok(())
}

/// A stored document read back, checked by the rules an import applies.
pub(crate) fn stored_profile(document: &str) -> Result<CfmProfile, StoreError> {
    CfmProfile::parse(document).map_err(|error| corrupt("CFM profile", error.to_string()))
}
