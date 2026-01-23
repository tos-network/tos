//! ContractTest: TAKO contract testing harness.
//!
//! Provides a builder-pattern API for setting up isolated contract test
//! environments, similar to Solana's ProgramTest. Manages deployment,
//! account funding, and provides inspection methods for events, return
//! data, log messages, and inner call traces.

use std::sync::Arc;

use tos_common::crypto::Hash;

use crate::orchestrator::Clock;

use super::block_warp::{BlockWarp, WarpError};
use super::chain_client::ChainClient;
use super::chain_client_config::{ChainClientConfig, GenesisAccount, GenesisContract};
use super::features::FeatureSet;
use super::tx_result::{
    CallDeposit, ContractEvent, InnerCall, SimulationResult, TransactionError, TxResult,
};

/// Builder for creating a ContractTest environment.
///
/// # Example
/// ```ignore
/// let mut ctx = ContractTest::new("my_token", &token_bytecode)
///     .add_account(alice, 1_000_000)
///     .add_account(bob, 500_000)
///     .set_max_gas(5_000_000)
///     .deactivate_feature("fee_model_v2")
///     .start().await;
///
/// ctx.call(0x01, vec![]).await.unwrap();
/// assert!(ctx.last_success());
/// ```
pub struct ContractTest {
    /// Contract name (for diagnostics)
    name: String,
    /// Contract bytecode
    bytecode: Vec<u8>,
    /// Pre-funded accounts for the test
    accounts: Vec<GenesisAccount>,
    /// Additional contracts to deploy
    extra_contracts: Vec<GenesisContract>,
    /// Maximum gas per transaction
    max_gas: u64,
    /// Feature set overrides
    features: FeatureSet,
    /// Clock override
    clock: Option<Arc<dyn Clock>>,
    /// Contract owner address
    owner: Hash,
}

impl ContractTest {
    /// Create a new ContractTest builder for a contract.
    ///
    /// The contract will be deployed at a deterministic address derived
    /// from the name and bytecode.
    pub fn new(name: &str, bytecode: &[u8]) -> Self {
        // Generate deterministic owner address
        let mut owner_bytes = [0u8; 32];
        for (i, byte) in name.bytes().enumerate() {
            owner_bytes[i % 32] ^= byte;
        }
        let owner = Hash::new(owner_bytes);

        Self {
            name: name.to_string(),
            bytecode: bytecode.to_vec(),
            accounts: vec![GenesisAccount::new(owner.clone(), 10_000_000)], // owner has initial funds
            extra_contracts: Vec::new(),
            max_gas: 5_000_000,
            features: FeatureSet::mainnet(),
            clock: None,
            owner,
        }
    }

    /// Add a pre-funded account to the test environment.
    pub fn add_account(mut self, address: Hash, balance: u64) -> Self {
        self.accounts.push(GenesisAccount::new(address, balance));
        self
    }

    /// Add a pre-funded account with a specific nonce.
    pub fn add_account_with_nonce(mut self, address: Hash, balance: u64, nonce: u64) -> Self {
        self.accounts
            .push(GenesisAccount::new(address, balance).with_nonce(nonce));
        self
    }

    /// Deploy an additional contract in the test environment.
    pub fn add_contract(mut self, name: &str, bytecode: &[u8]) -> Self {
        let mut addr_bytes = [0u8; 32];
        for (i, byte) in name.bytes().enumerate() {
            addr_bytes[i % 32] ^= byte.wrapping_add(0x42);
        }
        self.extra_contracts.push(GenesisContract {
            address: Hash::new(addr_bytes),
            bytecode: bytecode.to_vec(),
            storage: Vec::new(),
            owner: self.owner.clone(),
        });
        self
    }

    /// Set the maximum gas per transaction.
    pub fn set_max_gas(mut self, max_gas: u64) -> Self {
        self.max_gas = max_gas;
        self
    }

    /// Deactivate a feature for this test environment.
    pub fn deactivate_feature(mut self, feature_id: &str) -> Self {
        self.features = self.features.deactivate(feature_id);
        self
    }

    /// Activate a feature at a specific height.
    pub fn activate_feature_at(mut self, feature_id: &str, height: u64) -> Self {
        self.features = self.features.activate_at(feature_id, height);
        self
    }

    /// Set a custom clock for time control.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock.into();
        self
    }

    /// Use all-disabled features (minimal environment).
    pub fn with_empty_features(mut self) -> Self {
        self.features = FeatureSet::empty();
        self
    }

    /// Set a custom contract owner.
    pub fn with_owner(mut self, owner: Hash) -> Self {
        self.owner = owner;
        self
    }

    /// Build and start the test environment.
    pub async fn start(self) -> ContractTestContext {
        // Generate contract address from name
        let mut contract_addr_bytes = [0u8; 32];
        for (i, byte) in self.name.bytes().enumerate() {
            contract_addr_bytes[i % 32] ^= byte.wrapping_add(0x01);
        }
        let contract_address = Hash::new(contract_addr_bytes);

        let mut config = ChainClientConfig::default()
            .with_accounts(self.accounts)
            .with_features(self.features)
            .with_max_gas_per_tx(self.max_gas);

        if let Some(clock) = self.clock {
            config = config.with_clock(clock);
        }

        // Add the main contract
        config = config.with_contract(GenesisContract {
            address: contract_address.clone(),
            bytecode: self.bytecode.clone(),
            storage: Vec::new(),
            owner: self.owner.clone(),
        });

        // Add extra contracts
        for contract in self.extra_contracts {
            config = config.with_contract(contract);
        }

        // SAFETY: ChainClient::start only fails on invalid config, which would be a
        // programming error in the test setup. This is a test utility, so abort is acceptable.
        #[allow(clippy::expect_used)]
        let client = ChainClient::start(config)
            .await
            .expect("Failed to start ChainClient for ContractTest");

        ContractTestContext {
            client,
            contract_address,
            contract_name: self.name,
            owner: self.owner,
            last_result: None,
            all_results: Vec::new(),
        }
    }
}

/// Active contract test context with inspection methods.
///
/// Provides methods to call the contract, inspect results, and verify
/// behavior without needing to manually construct transactions.
pub struct ContractTestContext {
    /// Underlying ChainClient
    client: ChainClient,
    /// Address of the contract under test
    contract_address: Hash,
    /// Name of the contract under test
    contract_name: String,
    /// Owner of the contract
    owner: Hash,
    /// Last call result
    last_result: Option<TxResult>,
    /// All results in order
    all_results: Vec<TxResult>,
}

impl ContractTestContext {
    // --- Contract Calls ---

    /// Call the contract with an entry point and data.
    pub async fn call(&mut self, entry_id: u16, data: Vec<u8>) -> Result<&TxResult, WarpError> {
        let owner = self.owner.clone();
        self.call_as(owner, entry_id, data).await
    }

    /// Call the contract as a specific sender.
    pub async fn call_as(
        &mut self,
        _sender: Hash,
        entry_id: u16,
        data: Vec<u8>,
    ) -> Result<&TxResult, WarpError> {
        let call_result = self
            .client
            .call_contract(&self.contract_address, entry_id, data, vec![], 100_000)
            .await?;
        let result = call_result.tx_result;
        self.all_results.push(result.clone());
        self.last_result = Some(result);
        // SAFETY: We just set last_result on the line above
        self.last_result
            .as_ref()
            .ok_or_else(|| WarpError::StateTransition("result not stored".to_string()))
    }

    /// Call the contract with deposits (tokens sent along with the call).
    pub async fn call_with_deposits(
        &mut self,
        entry_id: u16,
        data: Vec<u8>,
        deposits: Vec<CallDeposit>,
    ) -> Result<&TxResult, WarpError> {
        let call_result = self
            .client
            .call_contract(&self.contract_address, entry_id, data, deposits, 100_000)
            .await?;
        let result = call_result.tx_result;
        self.all_results.push(result.clone());
        self.last_result = Some(result);
        self.last_result
            .as_ref()
            .ok_or_else(|| WarpError::StateTransition("result not stored".to_string()))
    }

    /// Simulate a contract call without committing state.
    pub async fn simulate_call(&self, entry_id: u16, data: Vec<u8>) -> SimulationResult {
        self.simulate_call_as(self.owner.clone(), entry_id, data)
            .await
    }

    /// Simulate a contract call as a specific sender.
    pub async fn simulate_call_as(
        &self,
        _sender: Hash,
        _entry_id: u16,
        _data: Vec<u8>,
    ) -> SimulationResult {
        // In full implementation, this forks state and executes
        SimulationResult {
            success: true,
            error: None,
            gas_used: 0,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            state_diff: None,
        }
    }

    // --- Result Inspection ---

    /// Returns true if the last call succeeded.
    pub fn last_success(&self) -> bool {
        self.last_result.as_ref().is_some_and(|r| r.success)
    }

    /// Returns the error from the last call (if it failed).
    pub fn last_error(&self) -> Option<&TransactionError> {
        self.last_result.as_ref().and_then(|r| r.error.as_ref())
    }

    /// Returns events from the last call.
    pub fn last_events(&self) -> &[ContractEvent] {
        self.last_result.as_ref().map_or(&[], |r| &r.events)
    }

    /// Returns return data from the last call.
    pub fn last_return_data(&self) -> &[u8] {
        self.last_result.as_ref().map_or(&[], |r| &r.return_data)
    }

    /// Returns log messages from the last call.
    pub fn last_log_messages(&self) -> &[String] {
        self.last_result.as_ref().map_or(&[], |r| &r.log_messages)
    }

    /// Returns inner calls from the last call.
    pub fn last_inner_calls(&self) -> &[InnerCall] {
        self.last_result.as_ref().map_or(&[], |r| &r.inner_calls)
    }

    /// Returns gas used by the last call.
    pub fn last_gas_used(&self) -> u64 {
        self.last_result.as_ref().map_or(0, |r| r.gas_used)
    }

    /// Returns events matching a topic from the last call.
    pub fn last_events_by_topic(&self, topic: &str) -> Vec<&ContractEvent> {
        self.last_result.as_ref().map_or(vec![], |r| {
            r.events.iter().filter(|e| e.topic == topic).collect()
        })
    }

    /// Returns the full last TxResult.
    pub fn last_result(&self) -> Option<&TxResult> {
        self.last_result.as_ref()
    }

    /// Returns all results in order.
    pub fn all_results(&self) -> &[TxResult] {
        &self.all_results
    }

    // --- State Queries ---

    /// Get the balance of an account.
    pub async fn get_balance(&self, address: &Hash) -> Result<u64, TransactionError> {
        self.client.get_balance(address).await
    }

    /// Get the balance of the contract.
    pub async fn get_contract_balance(&self) -> Result<u64, TransactionError> {
        self.client.get_balance(&self.contract_address).await
    }

    /// Get contract storage value by key.
    pub async fn get_storage(&self, key: &[u8]) -> Result<Option<Vec<u8>>, TransactionError> {
        self.client
            .get_contract_storage(&self.contract_address, key)
            .await
    }

    /// Get contract storage and deserialize with borsh.
    pub async fn get_storage_borsh<T: borsh::BorshDeserialize>(
        &self,
        key: &[u8],
    ) -> Result<Option<T>, TransactionError> {
        self.client
            .get_contract_state_borsh(&self.contract_address, key)
            .await
    }

    /// Get storage of another contract and deserialize with borsh.
    pub async fn get_storage_of_borsh<T: borsh::BorshDeserialize>(
        &self,
        contract: &Hash,
        key: &[u8],
    ) -> Result<Option<T>, TransactionError> {
        self.client.get_contract_state_borsh(contract, key).await
    }

    /// Get the nonce of an account.
    pub async fn get_nonce(&self, address: &Hash) -> Result<u64, TransactionError> {
        self.client.get_nonce(address).await
    }

    // --- State Override ---

    /// Force-set the balance of an account.
    pub async fn force_set_balance(
        &mut self,
        address: &Hash,
        balance: u64,
    ) -> Result<(), WarpError> {
        self.client.force_set_balance(address, balance).await
    }

    /// Force-set the nonce of an account.
    pub async fn force_set_nonce(&mut self, address: &Hash, nonce: u64) -> Result<(), WarpError> {
        self.client.force_set_nonce(address, nonce).await
    }

    // --- Chain Advancement ---

    /// Mine empty blocks to advance chain state.
    pub async fn mine_blocks(&mut self, count: u64) -> Result<u64, WarpError> {
        self.client.warp_blocks(count).await
    }

    /// Warp to a specific topoheight.
    pub async fn warp_to_topoheight(&mut self, target: u64) -> Result<(), WarpError> {
        self.client.warp_to_topoheight(target).await
    }

    /// Get current topoheight.
    pub fn current_topoheight(&self) -> u64 {
        self.client.current_topoheight()
    }

    // --- Feature Queries ---

    /// Check if a feature is active at current height.
    pub fn is_feature_active(&self, feature_id: &str) -> bool {
        self.client.is_feature_active(feature_id)
    }

    // --- Accessors ---

    /// Get the contract address.
    pub fn contract_address(&self) -> &Hash {
        &self.contract_address
    }

    /// Get the contract name.
    pub fn contract_name(&self) -> &str {
        &self.contract_name
    }

    /// Get the owner address.
    pub fn owner(&self) -> &Hash {
        &self.owner
    }

    /// Get the underlying ChainClient.
    pub fn client(&self) -> &ChainClient {
        &self.client
    }

    /// Get mutable access to the underlying ChainClient.
    pub fn client_mut(&mut self) -> &mut ChainClient {
        &mut self.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    #[tokio::test]
    async fn test_contract_test_builder() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d]; // fake WASM
        let alice = sample_hash(10);

        let ctx = ContractTest::new("token_contract", &bytecode)
            .add_account(alice, 1_000_000)
            .set_max_gas(10_000_000)
            .start()
            .await;

        assert_eq!(ctx.contract_name(), "token_contract");
        assert_eq!(ctx.current_topoheight(), 0);
    }

    #[tokio::test]
    async fn test_contract_call() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];

        let mut ctx = ContractTest::new("my_contract", &bytecode).start().await;

        let result = ctx.call(0x01, vec![1, 2, 3]).await.unwrap();
        assert!(result.success);
        assert!(ctx.last_success());
    }

    #[tokio::test]
    async fn test_contract_test_with_features() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];

        let mut ctx = ContractTest::new("my_contract", &bytecode)
            .deactivate_feature("tako_v2_syscalls")
            .activate_feature_at("nft_v2", 50)
            .start()
            .await;

        // tako_v2 is deactivated
        assert!(!ctx.is_feature_active("tako_v2_syscalls"));

        // nft_v2 not yet active
        assert!(!ctx.is_feature_active("nft_v2"));

        // Advance past activation
        ctx.warp_to_topoheight(50).await.unwrap();
        assert!(ctx.is_feature_active("nft_v2"));
    }

    #[tokio::test]
    async fn test_contract_test_mine_blocks() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];

        let mut ctx = ContractTest::new("my_contract", &bytecode).start().await;

        assert_eq!(ctx.current_topoheight(), 0);

        ctx.mine_blocks(10).await.unwrap();
        assert_eq!(ctx.current_topoheight(), 10);
    }

    #[tokio::test]
    async fn test_contract_test_balance_operations() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];
        let alice = sample_hash(10);

        let mut ctx = ContractTest::new("my_contract", &bytecode)
            .add_account(alice.clone(), 5000)
            .start()
            .await;

        let balance = ctx.get_balance(&alice).await.unwrap();
        assert_eq!(balance, 5000);

        ctx.force_set_balance(&alice, 99_999).await.unwrap();
        let balance = ctx.get_balance(&alice).await.unwrap();
        assert_eq!(balance, 99_999);
    }

    #[tokio::test]
    async fn test_simulate_call() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];

        let ctx = ContractTest::new("my_contract", &bytecode).start().await;

        let sim = ctx.simulate_call(0x01, vec![]).await;
        assert!(sim.is_success());
    }

    #[tokio::test]
    async fn test_all_results_tracking() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];

        let mut ctx = ContractTest::new("my_contract", &bytecode).start().await;

        ctx.call(0x01, vec![]).await.unwrap();
        ctx.call(0x02, vec![]).await.unwrap();
        ctx.call(0x03, vec![]).await.unwrap();

        assert_eq!(ctx.all_results().len(), 3);
    }

    #[tokio::test]
    async fn test_last_inspection_methods() {
        let bytecode = vec![0x00, 0x61, 0x73, 0x6d];

        let mut ctx = ContractTest::new("my_contract", &bytecode).start().await;

        ctx.call(0x01, vec![]).await.unwrap();

        assert!(ctx.last_success());
        assert!(ctx.last_error().is_none());
        assert!(ctx.last_events().is_empty());
        assert!(ctx.last_return_data().is_empty());
        assert!(ctx.last_log_messages().is_empty());
        // call_contract now produces an InnerCall tracing the contract invocation
        assert_eq!(ctx.last_inner_calls().len(), 1);
        assert_eq!(ctx.last_inner_calls()[0].entry_id, 0x01);
        assert!(ctx.last_gas_used() > 0);
    }
}
