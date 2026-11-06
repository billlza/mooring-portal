//! What every request handler shares.

use std::sync::Arc;

use portal_core::auth::{HashCost, HashError, PasswordHasher};
use portal_core::time::{Clock, UnixTime};
use portal_store::Store;

use crate::assets::Assets;
use crate::config::WebConfig;
use crate::db::Database;
use crate::error::AppError;
use crate::security::hashing::Hashing;
use crate::security::limits::Limits;

/// The portal's shared state.
#[derive(Debug, Clone)]
pub struct AppState(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    database: Database,
    hashing: Hashing,
    limits: Limits,
    clock: Arc<dyn Clock>,
    config: WebConfig,
    assets: Assets,
}

/// Why the web layer could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The password hasher rejected its cost parameters.
    #[error("cannot set up password hashing: {0}")]
    Hasher(#[from] HashError),
}

impl AppState {
    /// The state of a portal backed by `store`, hashing passwords at
    /// `cost` and reading the time from `clock`.
    pub fn new(
        store: Store,
        config: WebConfig,
        cost: HashCost,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, StartupError> {
        Ok(Self(Arc::new(Inner {
            database: Database::new(store),
            hashing: Hashing::new(PasswordHasher::new(cost)?),
            limits: Limits::new(),
            clock,
            config,
            assets: Assets::build(),
        })))
    }

    /// The database.
    pub(crate) fn database(&self) -> &Database {
        &self.0.database
    }

    /// The current time.
    pub(crate) fn now(&self) -> Result<UnixTime, AppError> {
        self.0.clock.now().map_err(AppError::internal)
    }

    pub(crate) fn hashing(&self) -> &Hashing {
        &self.0.hashing
    }

    pub(crate) fn limits(&self) -> &Limits {
        &self.0.limits
    }

    pub(crate) fn config(&self) -> &WebConfig {
        &self.0.config
    }

    pub(crate) fn assets(&self) -> &Assets {
        &self.0.assets
    }
}
