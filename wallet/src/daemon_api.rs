use std::{borrow::Cow, collections::HashSet, sync::Arc, time::Duration};

use anyhow::Result;
use log::{debug, trace};
use serde::Serialize;
use serde_json::Value;
use tos_common::{
    account::VersionedBalance,
    api::daemon::*,
    asset::RPCAssetData,
    contract::Module,
    crypto::{Address, Hash},
    rpc::client::{
        EventReceiver, JsonRPCResult, WebSocketJsonRPCClient, WebSocketJsonRPCClientImpl,
    },
    serializer::Serializer,
    tokio::sync::broadcast,
    transaction::Transaction,
};

pub struct DaemonAPI {
    client: WebSocketJsonRPCClient<NotifyEvent>,
    capacity: usize,
}

impl DaemonAPI {
    pub async fn new(daemon_address: String) -> Result<Self> {
        Self::with(daemon_address, None, 64).await
    }

    pub async fn with(
        daemon_address: String,
        timeout: Option<Duration>,
        capacity: usize,
    ) -> Result<Self> {
        let client = if let Some(timeout) = timeout {
            WebSocketJsonRPCClientImpl::with(daemon_address, timeout).await?
        } else {
            WebSocketJsonRPCClientImpl::new(daemon_address).await?
        };

        Ok(Self { client, capacity })
    }

    pub fn get_client(&self) -> &WebSocketJsonRPCClient<NotifyEvent> {
        &self.client
    }

    // is the websocket connection alive
    pub fn is_online(&self) -> bool {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is_online");
        }
        self.client.is_online()
    }

    // Disconnect by closing the connection with node RPC
    // This will only disconnect if there are no more references to the daemon API
    pub async fn disconnect(self: &Arc<Self>) -> Result<bool> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("disconnect");
        }
        let count = Arc::strong_count(self);
        if count > 1 {
            if log::log_enabled!(log::Level::Debug) {
                debug!("There are still {} references to the daemon API", count);
            }
            return Ok(false);
        }
        self.client.disconnect().await?;
        Ok(true)
    }

    // Disconnect by closing the connection with node RPC
    pub async fn disconnect_force(&self) -> Result<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("disconnect_force");
        }
        self.client.disconnect().await
    }

    // Try to reconnect using the same client
    pub async fn reconnect(&self) -> Result<bool> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("reconnect");
        }
        self.client.reconnect().await
    }

    // On connection event
    pub async fn on_connection(&self) -> broadcast::Receiver<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_connection");
        }
        self.client.on_connection().await
    }

    // On reconnect event
    pub async fn on_reconnect(&self) -> broadcast::Receiver<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_reconnect");
        }
        self.client.on_reconnect().await
    }

    // On connection lost
    pub async fn on_connection_lost(&self) -> broadcast::Receiver<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_connection_lost");
        }
        self.client.on_connection_lost().await
    }

    pub async fn call<P: Serialize>(&self, method: &String, params: &P) -> JsonRPCResult<Value> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("call: {}", method);
        }
        self.client.call_with(method.as_str(), params).await
    }

    pub async fn on_new_block_event(&self) -> Result<EventReceiver<NewBlockEvent>> {
        trace!("on_new_block_event");
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::NewBlock, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn on_block_ordered_event(&self) -> Result<EventReceiver<BlockOrderedEvent<'_>>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_block_ordered_event");
        }
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::BlockOrdered, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn on_transaction_orphaned_event(
        &self,
    ) -> Result<EventReceiver<TransactionOrphanedEvent>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_transaction_orphaned_event");
        }
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::TransactionOrphaned, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn on_stable_height_changed_event(
        &self,
    ) -> Result<EventReceiver<StableHeightChangedEvent>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_stable_height_changed_event");
        }
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::StableHeightChanged, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn on_transaction_added_in_mempool_event(
        &self,
    ) -> Result<EventReceiver<TransactionAddedInMempoolEvent>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_transaction_added_in_mempool_event");
        }
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::TransactionAddedInMempool, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn on_stable_topoheight_changed_event(
        &self,
    ) -> Result<EventReceiver<StableTopoHeightChangedEvent>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_stable_topoheight_changed_event");
        }
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::StableTopoHeightChanged, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn on_contract_transfer_event(
        &self,
        address: Address,
    ) -> Result<EventReceiver<ContractTransferEvent<'_>>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("on_contract_transfer_event");
        }
        let receiver = self
            .client
            .subscribe_event(NotifyEvent::ContractTransfer { address }, self.capacity)
            .await?;
        Ok(receiver)
    }

    pub async fn get_version(&self) -> Result<String> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_version");
        }
        let version = self.client.call("get_version").await?;
        Ok(version)
    }

    pub async fn get_info(&self) -> Result<GetInfoResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_info");
        }
        let info = self.client.call("get_info").await?;
        Ok(info)
    }

    pub async fn get_pruned_topoheight(&self) -> Result<Option<u64>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_pruned_topoheight");
        }
        let topoheight = self.client.call("get_pruned_topoheight").await?;
        Ok(topoheight)
    }

    pub async fn get_asset(&self, asset: &Hash) -> Result<RPCAssetData<'static>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_asset");
        }
        let assets = self
            .client
            .call_with(
                "get_asset",
                &GetAssetParams {
                    asset: Cow::Borrowed(asset),
                },
            )
            .await?;
        Ok(assets)
    }

    pub async fn get_account_assets(
        &self,
        address: &Address,
        maximum: Option<usize>,
        skip: Option<usize>,
    ) -> Result<HashSet<Hash>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_account_assets");
        }
        let assets = self
            .client
            .call_with(
                "get_account_assets",
                &GetAccountAssetsParams {
                    address: Cow::Borrowed(address),
                    maximum,
                    skip,
                },
            )
            .await?;
        Ok(assets)
    }

    pub async fn count_assets(&self) -> Result<usize> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("count_assets");
        }
        let count = self.client.call("count_assets").await?;
        Ok(count)
    }

    pub async fn get_assets(
        &self,
        skip: Option<usize>,
        maximum: Option<usize>,
        minimum_topoheight: Option<u64>,
        maximum_topoheight: Option<u64>,
    ) -> Result<Vec<RPCAssetData<'_>>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_assets");
        }
        let assets = self
            .client
            .call_with(
                "get_assets",
                &GetAssetsParams {
                    maximum,
                    skip,
                    minimum_topoheight,
                    maximum_topoheight,
                },
            )
            .await?;
        Ok(assets)
    }

    pub async fn get_balance(&self, address: &Address, asset: &Hash) -> Result<GetBalanceResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_balance");
        }
        let balance = self
            .client
            .call_with(
                "get_balance",
                &GetBalanceParams {
                    address: Cow::Borrowed(address),
                    asset: Cow::Borrowed(asset),
                },
            )
            .await?;
        Ok(balance)
    }

    pub async fn get_balance_at_topoheight(
        &self,
        address: &Address,
        asset: &Hash,
        topoheight: u64,
    ) -> Result<VersionedBalance> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_balance_at_topoheight");
        }
        let balance = self
            .client
            .call_with(
                "get_balance_at_topoheight",
                &GetBalanceAtTopoHeightParams {
                    topoheight,
                    asset: Cow::Borrowed(asset),
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(balance)
    }

    /// Get UNO (encrypted) balance for an address
    pub async fn get_uno_balance(&self, address: &Address) -> Result<GetUnoBalanceResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_uno_balance");
        }
        let balance = self
            .client
            .call_with(
                "get_uno_balance",
                &GetBalanceParams {
                    address: Cow::Borrowed(address),
                    asset: Cow::Borrowed(&tos_common::config::UNO_ASSET),
                },
            )
            .await?;
        Ok(balance)
    }

    /// Check if an address has a UNO (encrypted) balance
    pub async fn has_uno_balance(&self, address: &Address) -> Result<bool> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has_uno_balance");
        }
        let result: HasUnoBalanceResult = self
            .client
            .call_with(
                "has_uno_balance",
                &HasBalanceParams {
                    address: Cow::Borrowed(address),
                    asset: Cow::Borrowed(&tos_common::config::UNO_ASSET),
                    topoheight: None,
                },
            )
            .await?;
        Ok(result.exist)
    }

    /// Get UNO (encrypted) balance at a specific topoheight
    pub async fn get_uno_balance_at_topoheight(
        &self,
        address: &Address,
        topoheight: u64,
    ) -> Result<tos_common::account::VersionedUnoBalance> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_uno_balance_at_topoheight");
        }
        let balance = self
            .client
            .call_with(
                "get_uno_balance_at_topoheight",
                &GetBalanceAtTopoHeightParams {
                    topoheight,
                    asset: Cow::Borrowed(&tos_common::config::UNO_ASSET),
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(balance)
    }

    pub async fn get_block_at_topoheight(&self, topoheight: u64) -> Result<BlockResponse> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_block_at_topoheight");
        }
        let block = self
            .client
            .call_with(
                "get_block_at_topoheight",
                &GetBlockAtTopoHeightParams {
                    topoheight,
                    include_txs: false,
                },
            )
            .await?;
        Ok(block)
    }

    pub async fn get_block_with_txs_at_topoheight(&self, topoheight: u64) -> Result<BlockResponse> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_block_with_txs_at_topoheight");
        }
        let block = self
            .client
            .call_with(
                "get_block_at_topoheight",
                &GetBlockAtTopoHeightParams {
                    topoheight,
                    include_txs: true,
                },
            )
            .await?;
        Ok(block)
    }

    pub async fn get_transaction(&self, hash: &Hash) -> Result<Transaction> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_transaction");
        }
        let tx = self
            .client
            .call_with(
                "get_transaction",
                &GetTransactionParams {
                    hash: Cow::Borrowed(hash),
                },
            )
            .await?;
        Ok(tx)
    }

    pub async fn get_transaction_executor(
        &self,
        hash: &Hash,
    ) -> Result<GetTransactionExecutorResult<'_>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_transaction_executor");
        }
        let executor = self
            .client
            .call_with(
                "get_transaction_executor",
                &GetTransactionExecutorParams {
                    hash: Cow::Borrowed(hash),
                },
            )
            .await?;
        Ok(executor)
    }

    pub async fn submit_transaction(&self, transaction: &Transaction) -> Result<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("submit_transaction");
        }
        let _: bool = self
            .client
            .call_with(
                "submit_transaction",
                &SubmitTransactionParams {
                    data: transaction.to_hex(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn get_nonce(&self, address: &Address) -> Result<GetNonceResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_nonce");
        }
        let nonce = self
            .client
            .call_with(
                "get_nonce",
                &GetNonceParams {
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(nonce)
    }

    pub async fn is_tx_executed_in_block(&self, tx_hash: &Hash, block_hash: &Hash) -> Result<bool> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is_tx_executed_in_block");
        }
        let is_executed = self
            .client
            .call_with(
                "is_tx_executed_in_block",
                &IsTxExecutedInBlockParams {
                    tx_hash: Cow::Borrowed(tx_hash),
                    block_hash: Cow::Borrowed(block_hash),
                },
            )
            .await?;
        Ok(is_executed)
    }

    pub async fn get_mempool_cache(&self, address: &Address) -> Result<GetMempoolCacheResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_mempool_cache");
        }
        let cache = self
            .client
            .call_with(
                "get_mempool_cache",
                &GetMempoolCacheParams {
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(cache)
    }

    pub async fn is_account_registered(
        &self,
        address: &Address,
        in_stable_height: bool,
    ) -> Result<bool> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is_account_registered");
        }
        let is_registered = self
            .client
            .call_with(
                "is_account_registered",
                &IsAccountRegisteredParams {
                    address: Cow::Borrowed(address),
                    in_stable_height,
                },
            )
            .await?;
        Ok(is_registered)
    }

    pub async fn get_stable_topoheight(&self) -> Result<u64> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_stable_topoheight");
        }
        let topoheight = self.client.call("get_stable_topoheight").await?;
        Ok(topoheight)
    }

    pub async fn get_stable_balance(
        &self,
        address: &Address,
        asset: &Hash,
    ) -> Result<GetStableBalanceResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_stable_balance");
        }
        let balance = self
            .client
            .call_with(
                "get_stable_balance",
                &GetBalanceParams {
                    address: Cow::Borrowed(address),
                    asset: Cow::Borrowed(asset),
                },
            )
            .await?;
        Ok(balance)
    }

    pub async fn has_multisig(&self, address: &Address) -> Result<bool> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has_multisig");
        }
        let has_multisig = self
            .client
            .call_with(
                "has_multisig",
                &HasMultisigParams {
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(has_multisig)
    }

    pub async fn get_multisig(&self, address: &Address) -> Result<GetMultisigResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_multisig");
        }
        let multisig = self
            .client
            .call_with(
                "get_multisig",
                &GetMultisigParams {
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(multisig)
    }

    /// Get account history from daemon
    pub async fn get_account_history(
        &self,
        address: &Address,
        asset: &Hash,
        minimum_topoheight: Option<u64>,
        maximum_topoheight: Option<u64>,
    ) -> Result<Vec<AccountHistoryEntry>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_account_history");
        }
        let history: Vec<AccountHistoryEntry> = self
            .client
            .call_with(
                "get_account_history",
                &GetAccountHistoryParams {
                    address: address.clone(),
                    asset: asset.clone(),
                    minimum_topoheight,
                    maximum_topoheight,
                    incoming_flow: true,
                    outgoing_flow: true,
                },
            )
            .await?;
        Ok(history)
    }

    // Contract-related API methods

    /// Get the number of deployed contracts
    pub async fn count_contracts(&self) -> Result<usize> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("count_contracts");
        }
        let count = self.client.call("count_contracts").await?;
        Ok(count)
    }

    /// Get contract module (bytecode) by contract hash
    pub async fn get_contract_module(&self, contract: &Hash) -> Result<Module> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_contract_module");
        }
        let module = self
            .client
            .call_with(
                "get_contract_module",
                &GetContractModuleParams {
                    contract: Cow::Borrowed(contract),
                },
            )
            .await?;
        Ok(module)
    }

    /// Get contract address from a deployment transaction hash
    ///
    /// Contract address is NOT the same as the deployment TX hash.
    /// This method computes the deterministic contract address from:
    /// `blake3(0xff || deployer_pubkey || blake3(bytecode))`
    pub async fn get_contract_address_from_tx(
        &self,
        tx_hash: &Hash,
    ) -> Result<GetContractAddressFromTxResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_contract_address_from_tx");
        }
        let result = self
            .client
            .call_with(
                "get_contract_address_from_tx",
                &GetContractAddressFromTxParams {
                    transaction: Cow::Borrowed(tx_hash),
                },
            )
            .await?;
        Ok(result)
    }

    /// Get contract balance for a specific asset
    pub async fn get_contract_balance(&self, contract: &Hash, asset: &Hash) -> Result<u64> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_contract_balance");
        }
        let balance = self
            .client
            .call_with(
                "get_contract_balance",
                &GetContractBalanceParams {
                    contract: Cow::Borrowed(contract),
                    asset: Cow::Borrowed(asset),
                },
            )
            .await?;
        Ok(balance)
    }

    /// Get contract balance at a specific topoheight
    pub async fn get_contract_balance_at_topoheight(
        &self,
        contract: &Hash,
        asset: &Hash,
        topoheight: u64,
    ) -> Result<VersionedBalance> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_contract_balance_at_topoheight");
        }
        let balance = self
            .client
            .call_with(
                "get_contract_balance_at_topoheight",
                &GetContractBalanceAtTopoHeightParams {
                    contract: Cow::Borrowed(contract),
                    asset: Cow::Borrowed(asset),
                    topoheight,
                },
            )
            .await?;
        Ok(balance)
    }

    /// Get all assets held by a contract
    pub async fn get_contract_assets(
        &self,
        contract: &Hash,
        skip: Option<usize>,
        maximum: Option<usize>,
    ) -> Result<Vec<Hash>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_contract_assets");
        }
        let assets = self
            .client
            .call_with(
                "get_contract_assets",
                &GetContractBalancesParams {
                    contract: Cow::Borrowed(contract),
                    skip,
                    maximum,
                },
            )
            .await?;
        Ok(assets)
    }

    // ========== TNS (TOS Name Service) API ==========

    /// Resolve a TNS name to an address
    pub async fn resolve_name(&self, name: &str) -> Result<ResolveNameResult<'static>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("resolve_name");
        }
        let result: ResolveNameResult = self
            .client
            .call_with(
                "resolve_name",
                &ResolveNameParams {
                    name: Cow::Borrowed(name),
                },
            )
            .await?;
        Ok(result)
    }

    /// Check if a TNS name is available for registration
    pub async fn is_name_available(&self, name: &str) -> Result<IsNameAvailableResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is_name_available");
        }
        let result: IsNameAvailableResult = self
            .client
            .call_with(
                "is_name_available",
                &IsNameAvailableParams {
                    name: Cow::Borrowed(name),
                },
            )
            .await?;
        Ok(result)
    }

    /// Check if an address has a registered TNS name
    pub async fn has_registered_name(&self, address: &Address) -> Result<HasRegisteredNameResult> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has_registered_name");
        }
        let result: HasRegisteredNameResult = self
            .client
            .call_with(
                "has_registered_name",
                &HasRegisteredNameParams {
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(result)
    }

    /// Get the name hash registered by an account
    pub async fn get_account_name_hash(
        &self,
        address: &Address,
    ) -> Result<GetAccountNameHashResult<'static>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get_account_name_hash");
        }
        let result: GetAccountNameHashResult = self
            .client
            .call_with(
                "get_account_name_hash",
                &GetAccountNameHashParams {
                    address: Cow::Borrowed(address),
                },
            )
            .await?;
        Ok(result)
    }
}
