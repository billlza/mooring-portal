//! Password hashing, and the ban list's digests of new addresses, off the
//! async runtime, with bounded concurrency and a bounded queue.
//!
//! Each Argon2id hash holds about 19 MiB for tens of milliseconds, so at
//! most [`CONCURRENT_HASHES`] run at once. A request that will hash first
//! takes a [`HashTicket`]; at most [`TICKETS`] requests hold one, and a
//! request that finds none left is refused at once as busy, before it reads
//! anything or counts against a limit. A flood of logins therefore costs
//! bounded memory and a bounded wait, never a queue that grows without end.

use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use portal_core::account::{Email, NewPassword, PasswordAttempt};
use portal_core::auth::{
    DigestedEmail, HashError, PasswordHasher, StoredPasswordHash, Verification,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::error::AppError;

/// How many hashes run at once.
const CONCURRENT_HASHES: usize = 2;
/// How many requests may hold a ticket at once, hashing or waiting to: at
/// the recommended cost, sixteen logins waiting for two places wait well
/// under a second, and sixteen sign-ups, which hash twice, about a second
/// at most.
const TICKETS: usize = CONCURRENT_HASHES + 16;

/// Hashes and verifies passwords on blocking threads.
#[derive(Debug, Clone)]
pub(crate) struct Hashing {
    hasher: Arc<PasswordHasher>,
    running: Arc<Semaphore>,
    tickets: Arc<Semaphore>,
    /// Hashes and verifications finished, for tests to observe.
    #[cfg(test)]
    finished: Arc<AtomicUsize>,
}

/// A request's place among those that may hash, held until it is dropped.
#[derive(Debug)]
pub(crate) struct HashTicket {
    hashing: Hashing,
    _place: OwnedSemaphorePermit,
}

impl Hashing {
    pub(crate) fn new(hasher: PasswordHasher) -> Self {
        Self {
            hasher: Arc::new(hasher),
            running: Arc::new(Semaphore::new(CONCURRENT_HASHES)),
            tickets: Arc::new(Semaphore::new(TICKETS)),
            #[cfg(test)]
            finished: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// A ticket, or `None` when [`TICKETS`] requests hold one already.
    pub(crate) fn ticket(&self) -> Result<Option<HashTicket>, AppError> {
        match Arc::clone(&self.tickets).try_acquire_owned() {
            Ok(place) => Ok(Some(HashTicket {
                hashing: self.clone(),
                _place: place,
            })),
            Err(TryAcquireError::NoPermits) => Ok(None),
            Err(TryAcquireError::Closed) => {
                Err(AppError::internal_message("the hashing queue is closed"))
            }
        }
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&PasswordHasher) -> Result<T, HashError> + Send + 'static,
    ) -> Result<T, AppError> {
        let permit = Arc::clone(&self.running)
            .acquire_owned()
            .await
            .map_err(AppError::internal)?;
        let hasher = Arc::clone(&self.hasher);
        let done = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work(&hasher)
        })
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal);
        #[cfg(test)]
        self.finished.fetch_add(1, Ordering::SeqCst);
        done
    }

    /// How many hashes and verifications have finished.
    #[cfg(test)]
    pub(crate) fn finished(&self) -> usize {
        self.finished.load(Ordering::SeqCst)
    }

    /// How many tickets are free.
    #[cfg(test)]
    pub(crate) fn free_tickets(&self) -> usize {
        self.tickets.available_permits()
    }

    /// Takes every place to hash, so that ticket holders wait until the
    /// permits are dropped.
    #[cfg(test)]
    pub(crate) async fn occupy(&self) -> Vec<OwnedSemaphorePermit> {
        let mut permits = Vec::new();
        for _ in 0..CONCURRENT_HASHES {
            permits.push(Arc::clone(&self.running).acquire_owned().await.unwrap());
        }
        permits
    }
}

impl HashTicket {
    /// `email` with the ban list's digest of it, as slow as a password
    /// hash.
    pub(crate) async fn digest_email(&self, email: Email) -> Result<DigestedEmail, AppError> {
        self.hashing
            .blocking(move |_| DigestedEmail::new(email))
            .await
    }

    /// Hashes a new password.
    pub(crate) async fn hash(&self, password: NewPassword) -> Result<StoredPasswordHash, AppError> {
        self.hashing
            .blocking(move |hasher| hasher.hash(&password))
            .await
    }

    /// Checks `attempt` against `stored`, or, without an account, spends
    /// the same time and reports a mismatch.
    pub(crate) async fn verify(
        &self,
        attempt: PasswordAttempt,
        stored: Option<StoredPasswordHash>,
    ) -> Result<Verification, AppError> {
        self.hashing
            .blocking(move |hasher| match stored {
                Some(stored) => hasher.verify(&attempt, &stored),
                None => hasher
                    .verify_without_account(&attempt)
                    .map(|()| Verification::Mismatch),
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use portal_core::auth::HashCost;

    use super::*;

    fn hashing() -> Hashing {
        Hashing::new(PasswordHasher::new(HashCost::MINIMUM_FOR_TESTS).unwrap())
    }

    #[tokio::test]
    async fn hashes_verify_and_unknown_accounts_mismatch() {
        let hashing = hashing();
        let ticket = hashing.ticket().unwrap().unwrap();
        let email = Email::parse("a@qq.com").unwrap();
        let password = NewPassword::parse("correct horse battery".to_owned(), &email).unwrap();
        let stored = ticket.hash(password).await.unwrap();
        let attempt = || PasswordAttempt::new("correct horse battery".to_owned());
        assert_eq!(
            ticket
                .verify(attempt(), Some(stored.clone()))
                .await
                .unwrap(),
            Verification::Match
        );
        assert_eq!(
            ticket.verify(attempt(), None).await.unwrap(),
            Verification::Mismatch
        );
        let broken = StoredPasswordHash::from_stored("garbage".to_owned());
        assert!(matches!(
            ticket.verify(attempt(), Some(broken)).await,
            Err(AppError::Internal(_))
        ));
        assert_eq!(hashing.finished(), 4);
    }

    #[tokio::test]
    async fn tickets_run_out_and_come_back() {
        let hashing = hashing();
        let held: Vec<HashTicket> = (0..TICKETS)
            .map(|_| hashing.ticket().unwrap().unwrap())
            .collect();
        assert_eq!(hashing.free_tickets(), 0);
        assert!(hashing.ticket().unwrap().is_none());
        drop(held);
        assert_eq!(hashing.free_tickets(), TICKETS);
        assert!(hashing.ticket().unwrap().is_some());
    }

    #[tokio::test]
    async fn closed_semaphores_are_errors() {
        let hashing = hashing();
        let ticket = hashing.ticket().unwrap().unwrap();
        hashing.running.close();
        assert!(matches!(
            ticket
                .verify(PasswordAttempt::new(String::new()), None)
                .await,
            Err(AppError::Internal(_))
        ));
        hashing.tickets.close();
        assert!(matches!(hashing.ticket(), Err(AppError::Internal(_))));
    }
}
