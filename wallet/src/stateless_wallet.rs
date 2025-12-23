//! Stateless Wallet Implementation
//!
//! This module provides a stateless wallet that queries all state from the daemon
//! instead of maintaining local storage (sled DB). This eliminates sync issues
//! and reduces wallet storage to just the encrypted private key.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────┐    ┌─────────────┐    ┌────────┐
//! │ CLI Cmd  │───▶│  DaemonAPI  │───▶│ Daemon │
//! └──────────┘    │  (direct)   │    │  (RPC) │
//!                 └─────────────┘    └────────┘
//! ```
//!
//! ## Benefits
//!
//! - No sync issues - always uses latest chain state
//! - No nonce mismatch - queries before each TX
//! - No balance desync - real-time balance from daemon
//! - Smaller wallet - KB instead of GB
//! - Faster startup - no sync wait

use std::sync::Arc;

use anyhow::{Context, Result};
use log::{debug, trace};
use tos_common::{
    api::daemon::{
        GetBalanceResult, GetInfoResult, GetMempoolCacheResult, GetMultisigResult, GetNonceResult,
        MultisigState,
    },
    asset::RPCAssetData,
    config::TOS_ASSET,
    crypto::{Address, Hash, Hashable, KeyPair, PublicKey},
    network::Network,
    transaction::{
        builder::{FeeBuilder, TransactionBuilder, TransactionTypeBuilder},
        Reference, Transaction, TxVersion,
    },
};

use crate::{
    daemon_api::DaemonAPI, error::WalletError, transaction_builder::TransactionBuilderState,
};

/// Stateless wallet that queries all state from daemon
///
/// This wallet does not maintain any local blockchain state.
/// All balance, nonce, and transaction history queries go directly to the daemon.
pub struct StatelessWallet {
    /// Keypair for signing transactions
    keypair: KeyPair,
    /// Network (mainnet/testnet/devnet)
    network: Network,
    /// Daemon API client for RPC calls
    daemon_api: Arc<DaemonAPI>,
    /// Transaction version to use
    tx_version: TxVersion,
}

impl StatelessWallet {
    /// Create a new stateless wallet with existing keypair
    pub fn new(
        keypair: KeyPair,
        network: Network,
        daemon_api: Arc<DaemonAPI>,
        tx_version: TxVersion,
    ) -> Self {
        Self {
            keypair,
            network,
            daemon_api,
            tx_version,
        }
    }

    /// Get the public key
    pub fn get_public_key(&self) -> PublicKey {
        self.keypair.get_public_key().compress()
    }

    /// Get the address for this wallet
    pub fn get_address(&self) -> Address {
        self.get_public_key().to_address(self.network.is_mainnet())
    }

    /// Get the keypair
    pub fn get_keypair(&self) -> &KeyPair {
        &self.keypair
    }

    /// Get the network
    pub fn get_network(&self) -> &Network {
        &self.network
    }

    /// Get the daemon API
    pub fn get_daemon_api(&self) -> &Arc<DaemonAPI> {
        &self.daemon_api
    }

    /// Get the transaction version
    pub fn get_tx_version(&self) -> TxVersion {
        self.tx_version
    }

    /// Set the transaction version
    pub fn set_tx_version(&mut self, version: TxVersion) {
        self.tx_version = version;
    }

    // ========================================================================
    // Daemon Query Methods - All state from daemon
    // ========================================================================

    /// Get chain info from daemon
    pub async fn get_info(&self) -> Result<GetInfoResult> {
        trace!("get_info from daemon");
        self.daemon_api.get_info().await
    }

    /// Get current nonce from daemon
    pub async fn get_nonce(&self) -> Result<u64> {
        trace!("get_nonce from daemon");
        let address = self.get_address();
        let result = self.daemon_api.get_nonce(&address).await?;
        Ok(result.version.get_nonce())
    }

    /// Get nonce result with version info
    pub async fn get_nonce_result(&self) -> Result<GetNonceResult> {
        trace!("get_nonce_result from daemon");
        let address = self.get_address();
        self.daemon_api.get_nonce(&address).await
    }

    /// Get balance for a specific asset from daemon
    pub async fn get_balance(&self, asset: &Hash) -> Result<u64> {
        trace!("get_balance from daemon");
        let address = self.get_address();
        let result = self.daemon_api.get_balance(&address, asset).await?;
        Ok(result.balance)
    }

    /// Get balance result with version info
    pub async fn get_balance_result(&self, asset: &Hash) -> Result<GetBalanceResult> {
        trace!("get_balance_result from daemon");
        let address = self.get_address();
        self.daemon_api.get_balance(&address, asset).await
    }

    /// Get TOS balance from daemon
    pub async fn get_tos_balance(&self) -> Result<u64> {
        self.get_balance(&TOS_ASSET).await
    }

    /// Get asset info from daemon
    pub async fn get_asset(&self, asset: &Hash) -> Result<RPCAssetData<'static>> {
        trace!("get_asset from daemon");
        self.daemon_api.get_asset(asset).await
    }

    /// Get all assets for this account from daemon
    pub async fn get_account_assets(&self) -> Result<std::collections::HashSet<Hash>> {
        trace!("get_account_assets from daemon");
        let address = self.get_address();
        self.daemon_api
            .get_account_assets(&address, None, None)
            .await
    }

    /// Get mempool cache (pending transactions) from daemon
    pub async fn get_mempool_cache(&self) -> Result<GetMempoolCacheResult> {
        trace!("get_mempool_cache from daemon");
        let address = self.get_address();
        self.daemon_api.get_mempool_cache(&address).await
    }

    /// Check if this account is registered on chain
    pub async fn is_registered(&self, in_stable_height: bool) -> Result<bool> {
        trace!("is_registered from daemon");
        let address = self.get_address();
        self.daemon_api
            .is_account_registered(&address, in_stable_height)
            .await
    }

    /// Get multisig state from daemon
    pub async fn get_multisig(&self) -> Result<GetMultisigResult> {
        trace!("get_multisig from daemon");
        let address = self.get_address();
        self.daemon_api.get_multisig(&address).await
    }

    /// Check if this account has multisig
    pub async fn has_multisig(&self) -> Result<bool> {
        trace!("has_multisig from daemon");
        let address = self.get_address();
        self.daemon_api.has_multisig(&address).await
    }

    // ========================================================================
    // Transaction Building Methods
    // ========================================================================

    /// Get reference block for transaction building
    async fn get_reference(&self) -> Result<Reference> {
        let info = self.daemon_api.get_info().await?;
        Ok(Reference {
            topoheight: info.topoheight,
            hash: info.top_block_hash,
        })
    }

    /// Build a transaction with daemon state
    ///
    /// This method:
    /// 1. Queries nonce from daemon
    /// 2. Queries balances for used assets from daemon
    /// 3. Gets reference block from daemon
    /// 4. Builds and signs the transaction
    pub async fn build_transaction(
        &self,
        transaction_type: TransactionTypeBuilder,
        fee: FeeBuilder,
    ) -> Result<Transaction, WalletError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("build_transaction with daemon state");
        }

        // Query nonce from daemon
        let nonce = self.get_nonce().await.map_err(|e| {
            WalletError::Any(anyhow::anyhow!("Failed to query nonce from daemon: {}", e))
        })?;

        // Query reference from daemon
        let reference = self.get_reference().await.map_err(|e| {
            WalletError::Any(anyhow::anyhow!(
                "Failed to query reference from daemon: {}",
                e
            ))
        })?;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Building transaction with nonce={}, reference.topoheight={}",
                nonce, reference.topoheight
            );
        }

        // Create transaction builder state
        let mut state = TransactionBuilderState::new(self.network.is_mainnet(), reference, nonce);

        // Query balances for all used assets
        let used_assets = transaction_type.used_assets();
        for asset in used_assets {
            let balance = self.get_balance(&asset).await.map_err(|e| {
                WalletError::Any(anyhow::anyhow!(
                    "Failed to query balance for asset {} from daemon: {}",
                    asset,
                    e
                ))
            })?;

            if log::log_enabled!(log::Level::Debug) {
                debug!("Asset {} balance: {}", asset, balance);
            }

            state.add_balance(asset.clone(), crate::transaction_builder::Balance::new(balance));
        }

        // Get multisig threshold if applicable
        let threshold = match self.has_multisig().await {
            Ok(true) => match self.get_multisig().await {
                Ok(multisig) => match multisig.state {
                    MultisigState::Active { threshold, .. } => Some(threshold),
                    MultisigState::Deleted => None,
                },
                Err(_) => None,
            },
            _ => None,
        };

        // Create the transaction builder
        let builder = TransactionBuilder::new(
            self.tx_version,
            self.get_public_key(),
            threshold,
            transaction_type,
            fee,
        );

        // Build the final transaction
        let transaction = builder
            .build(&mut state, self.get_keypair())
            .map_err(|e| WalletError::Any(e.into()))?;

        let tx_hash = transaction.hash();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Transaction built: {} with nonce {} and reference {}",
                tx_hash,
                transaction.get_nonce(),
                transaction.get_reference()
            );
        }

        Ok(transaction)
    }

    /// Submit a transaction to the daemon
    pub async fn submit_transaction(&self, transaction: &Transaction) -> Result<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("submit_transaction {}", transaction.hash());
        }
        self.daemon_api
            .submit_transaction(transaction)
            .await
            .context("Failed to submit transaction to daemon")
    }

    /// Build and submit a transaction in one step
    pub async fn send_transaction(
        &self,
        transaction_type: TransactionTypeBuilder,
        fee: FeeBuilder,
    ) -> Result<Hash, WalletError> {
        let transaction = self.build_transaction(transaction_type, fee).await?;
        let hash = transaction.hash();

        self.submit_transaction(&transaction).await.map_err(|e| {
            WalletError::Any(anyhow::anyhow!("Failed to submit transaction: {}", e))
        })?;

        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    // Tests will be added when we have a mock DaemonAPI
}
