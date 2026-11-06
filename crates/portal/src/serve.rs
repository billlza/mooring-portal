//! Running the web server.

use std::io;
use std::path::Path;
use std::sync::Arc;

use portal_core::auth::HashCost;
use portal_core::time::SystemClock;
use portal_store::{OpenMode, Store};
use portal_web::{AppState, ClientIpSource, PublicOrigin, WebConfig};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use crate::CommandError;
use crate::cli::ServeArgs;

/// The configuration `args` describe.
pub(crate) fn config(args: &ServeArgs) -> Result<WebConfig, CommandError> {
    Ok(WebConfig {
        origin: PublicOrigin::parse(&args.origin)?,
        client_ip: if args.behind_proxy {
            ClientIpSource::ForwardedFor
        } else {
            ClientIpSource::Peer
        },
        max_accounts: args.max_accounts,
    })
}

/// `serve`: runs until SIGINT or SIGTERM, then finishes the requests in
/// flight.
pub(crate) fn serve(db: &Path, args: &ServeArgs) -> Result<(), CommandError> {
    let config = config(args)?;
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_target(false)
        .try_init()
        .map_err(|error| CommandError::Logging(error.to_string()))?;
    let state = AppState::new(
        Store::open(db, OpenMode::CreateIfMissing)?,
        config.clone(),
        HashCost::RECOMMENDED,
        Arc::new(SystemClock),
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(CommandError::Server)?;
    runtime.block_on(async {
        let listener = TcpListener::bind(args.listen)
            .await
            .map_err(CommandError::Server)?;
        // Both handlers are installed before the server says it is serving,
        // so a signal sent as soon as it does never meets the default action.
        let mut terminate = signal(SignalKind::terminate()).map_err(CommandError::Server)?;
        let mut interrupt = signal(SignalKind::interrupt()).map_err(CommandError::Server)?;
        tracing::info!(
            address = %listener.local_addr().map_err(CommandError::Server)?,
            origin = %config.origin,
            "serving the portal"
        );
        let shutdown = async move {
            // Either ends the wait; a stream ends only with the runtime.
            let received = tokio::select! {
                _ = terminate.recv() => "SIGTERM",
                _ = interrupt.recv() => "SIGINT",
            };
            tracing::info!(
                signal = received,
                "stopping: finishing the requests in flight"
            );
        };
        portal_web::serve(listener, state, shutdown)
            .await
            .map_err(CommandError::Server)
    })
}
