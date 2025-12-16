use crate::daemon_api::DaemonAPI;
use anyhow::{Context, Result};
use std::sync::Arc;
use tos_common::{
    crypto::{Address, Hash},
    transaction::Reference,
};

/// Lightweight API client for light wallet mode
/// Queries blockchain state on-demand from daemon instead of maintaining local sync
pub struct LightAPI {
    daemon: Arc<DaemonAPI>,
}

impl LightAPI {
    /// Create a new LightAPI instance
    pub fn new(daemon: Arc<DaemonAPI>) -> Self {
        Self { daemon }
    }

    /// Get current nonce for account (query on-demand from daemon)
    /// Returns 0 for fresh accounts that haven't made any transactions yet
    pub async fn get_nonce(&self, address: &Address) -> Result<u64> {
        match self.daemon.get_nonce(address).await {
            Ok(result) => Ok(result.version.get_nonce()),
            Err(e) => {
                let error_msg = format!("{:#}", e);
                // Fresh accounts (no transactions) return "Data not found" error
                // In this case, default nonce is 0
                if error_msg.contains("Data not found") {
                    Ok(0)
                } else {
                    Err(e).context(format!(
                        "Failed to get nonce from daemon for address {}",
                        address
                    ))
                }
            }
        }
    }

    /// Get reference block for transaction (query on-demand from daemon)
    /// Returns the current stable topoheight and top block hash
    pub async fn get_reference_block(&self) -> Result<Reference> {
        let info = self
            .daemon
            .get_info()
            .await
            .context("Failed to get chain info from daemon")?;
        Ok(Reference {
            topoheight: info.topoheight,
            hash: info.top_block_hash,
        })
    }

    /// Get balance for asset (query on-demand from daemon)
    /// Returns 0 for fresh accounts that haven't received any assets yet
    pub async fn get_balance(&self, address: &Address, asset: &Hash) -> Result<u64> {
        match self.daemon.get_balance(address, asset).await {
            Ok(result) => Ok(result.balance),
            Err(e) => {
                let error_msg = format!("{:#}", e);
                // Fresh accounts (no balance) return various errors:
                // - "Data not found" - general not found error
                // - "No account found" - account has never received any funds
                // In these cases, default balance is 0
                if error_msg.contains("Data not found") || error_msg.contains("No account found") {
                    Ok(0)
                } else {
                    Err(e).context("Failed to get balance from daemon")
                }
            }
        }
    }
}
