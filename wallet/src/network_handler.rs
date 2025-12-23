use crate::{
    config::AUTO_RECONNECT_INTERVAL,
    daemon_api::DaemonAPI,
    wallet::{Event, Wallet},
};
use anyhow::Error;
use log::{debug, error, info, trace, warn};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tos_common::{
    tokio::{
        select, spawn_task,
        sync::Mutex,
        task::{JoinError, JoinHandle},
        time::sleep,
    },
    utils::sanitize_ws_address,
};

// NetworkHandler must be behind a Arc to be accessed from Wallet (to stop it) or from tokio task
pub type SharedNetworkHandler = Arc<NetworkHandler>;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("network handler is already running")]
    AlreadyRunning,
    #[error("network handler is not running")]
    NotRunning,
    #[error(transparent)]
    TaskError(#[from] JoinError),
    #[error(transparent)]
    DaemonAPIError(#[from] Error),
    #[error("Network mismatch")]
    NetworkMismatch,
}

pub struct NetworkHandler {
    // tokio task
    task: Mutex<Option<JoinHandle<Result<(), Error>>>>,
    // wallet for event propagation
    wallet: Arc<Wallet>,
    // api to communicate with daemon
    // It is behind a Arc to be shared across several wallets
    // in case someone make a custom service and don't want to create a new connection
    api: Arc<DaemonAPI>,
}

impl NetworkHandler {
    // Create a new network handler with a wallet and a daemon address
    // This will create itself a DaemonAPI and verify if connection is possible
    pub async fn new<S: ToString>(
        wallet: Arc<Wallet>,
        daemon_address: S,
    ) -> Result<SharedNetworkHandler, Error> {
        let s = daemon_address.to_string();
        let api = DaemonAPI::new(format!("{}/json_rpc", sanitize_ws_address(s.as_str()))).await?;
        Self::with_api(wallet, Arc::new(api)).await
    }

    // Create a new network handler with an already created daemon API
    pub async fn with_api(
        wallet: Arc<Wallet>,
        api: Arc<DaemonAPI>,
    ) -> Result<SharedNetworkHandler, Error> {
        // check that we can correctly get version from daemon
        let version = api.get_version().await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!("Connected to daemon running version {}", version);
        }

        Ok(Arc::new(Self {
            task: Mutex::new(None),
            wallet,
            api,
        }))
    }

    // Start the network handler - maintains WebSocket connection and propagates events
    // For stateless wallet: only establishes connection, no block syncing
    pub async fn start(self: &Arc<Self>, auto_reconnect: bool) -> Result<(), NetworkError> {
        trace!("Starting network handler");

        if self.is_running().await {
            return Err(NetworkError::AlreadyRunning);
        }

        if !self.api.is_online() {
            debug!("API is offline, trying to reconnect #1");
            if !self.api.reconnect().await? {
                error!("Couldn't reconnect to server");
                return Err(NetworkError::NotRunning);
            }
        }

        let zelf = Arc::clone(&self);
        *self.task.lock().await = Some(spawn_task("network-handler", async move {
            loop {
                // Notify that we are online
                zelf.wallet.propagate_event(Event::Online).await;

                let res = zelf.maintain_connection().await;
                if let Err(e) = res.as_ref() {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Error while maintaining connection: {}", e);
                    }
                    zelf.wallet
                        .propagate_event(Event::SyncError {
                            message: e.to_string(),
                        })
                        .await;
                }

                // Notify that we are offline
                zelf.wallet.propagate_event(Event::Offline).await;

                if !auto_reconnect {
                    // Turn off the websocket connection
                    if let Err(e) = zelf.api.disconnect().await {
                        if log::log_enabled!(log::Level::Error) {
                            error!("Error while closing websocket connection: {}", e);
                        }
                    }

                    break res;
                } else {
                    if !zelf.api.is_online() {
                        debug!("API is offline, trying to reconnect #2");
                        if !zelf.api.reconnect().await? {
                            if log::log_enabled!(log::Level::Error) {
                                error!(
                                    "Couldn't reconnect to server, trying again in {} seconds",
                                    AUTO_RECONNECT_INTERVAL
                                );
                            }
                            sleep(Duration::from_secs(AUTO_RECONNECT_INTERVAL)).await;
                        }
                    } else {
                        if log::log_enabled!(log::Level::Warn) {
                            warn!(
                                "Connection lost, trying again in {} seconds",
                                AUTO_RECONNECT_INTERVAL
                            );
                        }
                        sleep(Duration::from_secs(AUTO_RECONNECT_INTERVAL)).await;
                    }
                }
            }
        }));

        Ok(())
    }

    // Stop the internal loop to stop syncing
    pub async fn stop(&self, api: bool) -> Result<(), NetworkError> {
        trace!("Stopping network handler");
        if let Some(handle) = self.task.lock().await.take() {
            if handle.is_finished() {
                debug!("Network handler is already finished");
                // We are already finished, which mean the event got triggered
                handle.await??;
            } else {
                debug!("Network handler is running, stopping it");
                handle.abort();

                // Notify that we are offline
                self.wallet.propagate_event(Event::Offline).await;
            }
        }

        // Disconnect API if requested (for both running and stateless wallet modes)
        if api {
            debug!("Disconnecting API connection");
            if let Err(e) = self.api.disconnect().await {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Error while closing websocket connection: {}", e);
                }
            }
        }

        Ok(())
    }

    // Retrieve the daemon API used
    pub fn get_api(&self) -> &DaemonAPI {
        &self.api
    }

    // check if the network handler is running (that we have a task and its not finished)
    pub async fn is_running(&self) -> bool {
        let task = self.task.lock().await;
        if let Some(handle) = task.as_ref() {
            !handle.is_finished() && self.api.is_online()
        } else {
            false
        }
    }

    // Maintain WebSocket connection and subscribe to events
    // For stateless wallet: no block syncing, only connection management
    async fn maintain_connection(self: &Arc<Self>) -> Result<(), Error> {
        debug!("Maintaining WebSocket connection");

        // Verify network compatibility
        let info = self.api.get_info().await?;
        let network = self.wallet.get_network();
        if info.network != *network {
            if log::log_enabled!(log::Level::Error) {
                error!(
                    "Network mismatch! Our network is {} while daemon is {}",
                    network, info.network
                );
            }
            return Err(NetworkError::NetworkMismatch.into());
        }

        // Subscribe to network events for connection monitoring
        let mut on_connection = self.api.on_connection().await;
        let mut on_connection_lost = self.api.on_connection_lost().await;

        if log::log_enabled!(log::Level::Info) {
            info!("WebSocket connection established, monitoring network events");
        }

        loop {
            select! {
                biased;
                // Detect network events
                res = on_connection.recv() => {
                    trace!("on_connection");
                    res?;
                    self.wallet.propagate_event(Event::Online).await;
                },
                res = on_connection_lost.recv() => {
                    trace!("on_connection_lost");
                    res?;
                    self.wallet.propagate_event(Event::Offline).await;
                    // Connection lost, return to trigger reconnect
                    return Ok(());
                }
            }
        }
    }
}

// REMOVED: Block synchronization methods for stateless wallet refactor
// =====================================================================
// The following methods have been removed as they are no longer needed:
//
// - process_block() - Processed blocks and updated local storage
//   * Scanned transactions for wallet address
//   * Updated balances in local database
//   * Stored transaction history
//   * Detected mined blocks
//
// - has_tx_stored() - Checked if transaction exists in local storage
//
// - get_balance_and_transactions() - Scanned balance history from chain
//   * Iterated through balance versions
//   * Fetched and processed historical blocks
//   * Updated local balance cache
//
// - locate_sync_topoheight_and_clean() - Detected chain reorganizations
//   * Validated block hashes against daemon
//   * Cleaned up orphaned transactions
//   * Maintained sync checkpoint
//
// - sync_head_state() - Synced latest balances/nonces to local storage
//   * Fetched current balances for all tracked assets
//   * Updated nonce from daemon
//   * Wrote to local database
//
// - sync() - Orchestrated complete synchronization
//   * Called locate_sync_topoheight_and_clean()
//   * Called sync_head_state()
//   * Called sync_new_blocks()
//   * Handled reorg events
//
// - start_syncing() - Main event loop with block processing
//   * Subscribed to new block events
//   * Processed incoming blocks
//   * Handled transaction orphaned events
//   * Managed contract transfer events
//
// - sync_new_blocks() - Scanned historical blocks for transactions
//   * Iterated through tracked assets
//   * Fetched balance history
//   * Processed historical transactions
//
// Stateless wallet architecture:
// ==============================
// Instead of maintaining a local synchronized copy of balances and transactions,
// the stateless wallet queries the daemon on-demand via DaemonAPI:
//
// - get_balance() - Fetch current balance from daemon
// - get_nonce() - Fetch current nonce from daemon
// - get_transaction() - Fetch transaction details from daemon
// - submit_transaction() - Submit transaction to mempool
//
// Benefits:
// - No local database synchronization overhead
// - No storage of historical data
// - Faster wallet startup (no sync required)
// - Always up-to-date with daemon state
// - Simpler codebase and fewer edge cases
