use std::sync::Arc;
use anyhow::{Result, Context};
use tos_common::{
    crypto::{Hash, Address},
    transaction::Reference,
};
use crate::daemon_api::DaemonAPI;

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
    pub async fn get_nonce(&self, address: &Address) -> Result<u64> {
        let result = self.daemon.get_nonce(address).await
            .context("Failed to get nonce from daemon")?;
        Ok(result.version.get_nonce())
    }

    /// Get reference block for transaction (query on-demand from daemon)
    /// Returns the current stable topoheight and top block hash
    pub async fn get_reference_block(&self) -> Result<Reference> {
        let info = self.daemon.get_info().await
            .context("Failed to get chain info from daemon")?;
        Ok(Reference {
            topoheight: info.topoheight,
            hash: info.top_block_hash,
        })
    }

    /// Get balance for asset (query on-demand from daemon)
    pub async fn get_balance(&self, address: &Address, asset: &Hash) -> Result<u64> {
        let result = self.daemon.get_balance(address, asset).await
            .context("Failed to get balance from daemon")?;
        Ok(result.balance)
    }
}
