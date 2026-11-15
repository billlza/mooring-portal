//! What every request handler shares.

use std::sync::Arc;

use axum::http::header::InvalidHeaderValue;
use portal_core::auth::{HashCost, HashError, PasswordHasher};
use portal_core::time::{Clock, UnixTime};
use portal_store::{ModeError, Store, StoreError};

use crate::assets::Assets;
use crate::config::{Payments, WebConfig};
use crate::db::Database;
use crate::error::AppError;
use crate::security::hashing::Hashing;
use crate::security::headers::ChatPolicy;
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
    chat_policy: ChatPolicy,
}

/// Why the web layer could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The password hasher rejected its cost parameters.
    #[error("cannot set up password hashing: {0}")]
    Hasher(#[from] HashError),
    /// The database holds payments of the other mode than the
    /// catalogue's: Stripe's test mode, or real money.
    #[error(transparent)]
    PaymentMode(#[from] ModeError),
    /// The server was started without payments over a database with
    /// money in the air: Stripe would be heard by no one, and Mooring
    /// could not fetch what it is owed.
    #[error(
        "the database has subscriptions that Stripe may charge again, or orders that are not \
         settled (`orders` lists them), and the server was started without payments: start it \
         with `--payments` and `--stripe-webhook-secret`; a rehearsal's orders are removed with \
         `discard-test-payments`"
    )]
    PaymentsOutstanding,
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The origin could not be written into the chat pages' content
    /// security policy.
    #[error("cannot write the chat pages' content security policy for the origin: {0}")]
    ChatPolicy(#[from] InvalidHeaderValue),
}

impl AppState {
    /// The state of a portal backed by `store`, hashing passwords at
    /// `cost` and reading the time from `clock`.
    ///
    /// A portal that sells makes `store` hold the payments of its
    /// catalogue's mode, and does not start over one that holds the
    /// other's. A portal that does not sell does not start over a store
    /// with money in the air.
    pub fn new(
        mut store: Store,
        config: WebConfig,
        cost: HashCost,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, StartupError> {
        match &config.payments {
            Some(payments) => store.enter_payment_mode(payments.catalogue().livemode())?,
            None if store.payments_outstanding()? => {
                return Err(StartupError::PaymentsOutstanding);
            }
            None => {}
        }
        let chat_policy = ChatPolicy::at(&config.origin)?;
        Ok(Self(Arc::new(Inner {
            database: Database::new(store),
            hashing: Hashing::new(PasswordHasher::new(cost)?),
            limits: Limits::new(),
            clock,
            config,
            assets: Assets::build(),
            chat_policy,
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

    /// The policy of the pages that run the chat script.
    pub(crate) fn chat_policy(&self) -> &ChatPolicy {
        &self.0.chat_policy
    }

    /// Tells the host, if the operator asked for it, that a user's message
    /// was stored at `now` ([`crate::NewMessageSignal`]). A failure is
    /// logged as an error and goes no further: the message is stored by
    /// then, and the operator reads it on the operator pages whether or
    /// not the host passes the word on, so the user is told it was sent.
    /// Failing the request instead would tell them otherwise, and they
    /// would send it again.
    pub(crate) async fn signal_new_message(&self, now: UnixTime) {
        let Some(signal) = self.0.config.new_message_signal.clone() else {
            return;
        };
        let path = signal.path().display().to_string();
        match tokio::task::spawn_blocking(move || signal.raise(now)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::error!(%path, %error, "telling the host of a new message failed");
            }
            Err(error) => {
                tracing::error!(%path, %error, "telling the host of a new message failed");
            }
        }
    }

    /// What the portal sells, if it sells anything.
    pub(crate) fn payments(&self) -> Option<&Payments> {
        self.0.config.payments.as_ref()
    }

    /// What the portal sells, for an address that only a portal that
    /// sells has: the router gives a portal that sells nothing no such
    /// address, and this answers as for none were one ever reached.
    pub(crate) fn selling(&self) -> Result<&Payments, AppError> {
        self.payments().ok_or(AppError::NotFound)
    }
}
