//! The bridge from async handlers to the synchronous store.
//!
//! The store's SQLite connection is used from one blocking thread at a
//! time; handlers hand it a closure and await the result. A panicked
//! blocking task or a poisoned lock is an internal error, never ignored.

use std::sync::{Arc, Mutex};

use portal_store::Store;

use crate::error::AppError;

/// The portal's database, shared by every request. Only the handlers reach
/// it, so every use passes their checks.
#[derive(Debug, Clone)]
pub(crate) struct Database(Arc<Mutex<Store>>);

impl Database {
    /// Shares `store`.
    pub(crate) fn new(store: Store) -> Self {
        Self(Arc::new(Mutex::new(store)))
    }

    /// Runs `work` on the store on a blocking thread.
    pub(crate) async fn run<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Store) -> R + Send + 'static,
    ) -> Result<R, AppError> {
        let shared = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || {
            let mut store = shared
                .lock()
                .map_err(|_| AppError::internal_message("the database lock is poisoned"))?;
            Ok(work(&mut store))
        })
        .await
        .map_err(AppError::internal)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn work_runs_on_the_store() {
        let database = Database::new(Store::open_in_memory().unwrap());
        let accounts = database
            .run(|store| store.accounts())
            .await
            .unwrap()
            .unwrap();
        assert!(accounts.is_empty());
    }

    #[tokio::test]
    async fn a_panic_and_the_poisoned_lock_it_leaves_are_errors() {
        let database = Database::new(Store::open_in_memory().unwrap());
        let panicked = database
            .run(|_| -> () { std::panic::panic_any("boom") })
            .await;
        assert!(matches!(panicked, Err(AppError::Internal(_))));
        let poisoned = database.run(|store| store.accounts()).await;
        assert!(
            matches!(&poisoned, Err(AppError::Internal(error)) if error.to_string().contains("poisoned")),
            "{poisoned:?}"
        );
    }
}
