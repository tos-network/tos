use std::{borrow::Cow, collections::HashMap};

use crate::{
    account::Nonce,
    block::{Block, BlockVersion},
    contract::{
        AssetChanges, ChainState, ContractCache, ContractEventTracker, ContractOutput,
        ContractProvider,
    },
    crypto::{elgamal::CompressedPublicKey, Hash},
    transaction::{ContractDeposit, MultiSigPayload, Reference, Transaction},
};
use async_trait::async_trait;
use indexmap::IndexMap;
use tos_kernel::Environment;
use tos_kernel::Module;

/// This trait is used by the batch verification function.
/// It is intended to represent a virtual snapshot of the current blockchain
/// state, where the transactions can get applied in order.
#[async_trait]
pub trait BlockchainVerificationState<'a, E> {
    // This is giving a "implementation is not general enough"
    // We replace it by a generic type in the trait definition
    // See: https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=aaa6065daaab514e638b2333703765c7
    // type Error;

    /// Pre-verify the TX
    async fn pre_verify_tx<'b>(&'b mut self, tx: &Transaction) -> Result<(), E>;

    /// Get the balance for a receiver account (plaintext u64)
    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, E>;

    /// Get the balance used for verification of funds for the sender account (plaintext u64)
    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a CompressedPublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, E>;

    /// Apply new output to a sender account (plaintext u64)
    async fn add_sender_output(
        &mut self,
        account: &'a CompressedPublicKey,
        asset: &'a Hash,
        output: u64,
    ) -> Result<(), E>;

    /// Get the nonce of an account
    async fn get_account_nonce(&mut self, account: &'a CompressedPublicKey) -> Result<Nonce, E>;

    /// Apply a new nonce to an account
    async fn update_account_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        new_nonce: Nonce,
    ) -> Result<(), E>;

    /// Atomically compare and swap nonce to prevent race conditions
    /// Returns true if the nonce matched expected value and was updated
    /// Returns false if the current nonce didn't match expected value
    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, E>;

    /// Get the block version in which TX is executed
    fn get_block_version(&self) -> BlockVersion;

    /// Get the timestamp to use for verification
    ///
    /// For block validation (consensus): returns the block timestamp
    /// For mempool verification: returns current system time
    ///
    /// This ensures deterministic consensus validation while allowing
    /// flexibility for mempool operations.
    fn get_verification_timestamp(&self) -> u64;

    /// Set the multisig state for an account
    async fn set_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), E>;

    /// Set the multisig state for an account
    async fn get_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<&MultiSigPayload>, E>;

    /// Get the environment
    async fn get_environment(&mut self) -> Result<&Environment, E>;

    /// Set the contract module
    async fn set_contract_module(&mut self, hash: &Hash, module: &'a Module) -> Result<(), E>;

    /// Load in the cache the contract module
    /// This is called before `get_contract_module_with_environment`
    /// Returns true if the module is available
    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, E>;

    /// Get the contract module with the environment
    /// This is used to verify that all parameters are correct
    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), E>;
}

pub struct ContractEnvironment<'a, P: ContractProvider> {
    // Environment with the embed stdlib
    pub environment: &'a Environment,
    // Module to execute
    pub module: &'a Module,
    // Provider for the contract
    pub provider: &'a mut P,
}

#[async_trait]
pub trait BlockchainApplyState<'a, P: ContractProvider, E>:
    BlockchainVerificationState<'a, E>
{
    /// Add burned Tos
    async fn add_burned_coins(&mut self, amount: u64) -> Result<(), E>;

    /// Add fee Tos
    async fn add_gas_fee(&mut self, amount: u64) -> Result<(), E>;

    /// Get the hash of the block
    fn get_block_hash(&self) -> &Hash;

    /// Get the block
    fn get_block(&self) -> &Block;

    /// Is mainnet network
    fn is_mainnet(&self) -> bool;

    /// Get the network type
    fn get_network(&self) -> crate::network::Network;

    /// Track the contract outputs
    async fn set_contract_outputs(
        &mut self,
        tx_hash: &'a Hash,
        outputs: Vec<ContractOutput>,
    ) -> Result<(), E>;

    /// Get the contract environment
    /// Implementation should take care of deposits by applying them
    /// to the chain state
    async fn get_contract_environment_for<'b>(
        &'b mut self,
        contract: &'b Hash,
        deposits: &'b IndexMap<Hash, ContractDeposit>,
        tx_hash: &'b Hash,
    ) -> Result<(ContractEnvironment<'b, P>, ChainState<'b>), E>;

    /// Merge the contract cache with the stored one
    async fn merge_contract_changes(
        &mut self,
        hash: &Hash,
        cache: ContractCache,
        tracker: ContractEventTracker,
        assets: HashMap<Hash, Option<AssetChanges>>,
    ) -> Result<(), E>;

    /// Remove the contract module
    /// This will mark the contract
    /// as a None version
    async fn remove_contract_module(&mut self, hash: &Hash) -> Result<(), E>;

    /// Get the energy resource for an account
    async fn get_energy_resource(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<crate::account::EnergyResource>, E>;

    /// Set the energy resource for an account
    async fn set_energy_resource(
        &mut self,
        account: &'a CompressedPublicKey,
        energy_resource: crate::account::EnergyResource,
    ) -> Result<(), E>;

    /// Get the AI mining state
    async fn get_ai_mining_state(&mut self) -> Result<Option<crate::ai_mining::AIMiningState>, E>;

    /// Set the AI mining state
    async fn set_ai_mining_state(
        &mut self,
        state: &crate::ai_mining::AIMiningState,
    ) -> Result<(), E>;

    /// Get the contract executor for executing contracts
    /// This returns an Arc to the executor implementation (TOS Kernel(TAKO), legacy VM, etc.)
    /// that will be used to execute contract bytecode.
    /// Using Arc avoids borrow conflicts when executor is used alongside mutable state access.
    fn get_contract_executor(&self) -> std::sync::Arc<dyn crate::contract::ContractExecutor>;

    /// Add contract events emitted during execution (LOG0-LOG4 syscalls)
    /// These events will be indexed and stored for later querying
    async fn add_contract_events(
        &mut self,
        events: Vec<crate::contract::ContractEvent>,
        contract: &Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), E>;

    // ===== Referral System Operations =====

    /// Bind a referrer to a user
    /// This operation is one-time only - once bound, cannot be changed
    ///
    /// # Arguments
    /// * `user` - The user binding the referrer
    /// * `referrer` - The referrer's public key
    /// * `tx_hash` - The transaction hash
    ///
    /// # Errors
    /// * `AlreadyBound` - User already has a referrer
    /// * `SelfReferral` - Cannot set self as referrer
    /// * `CircularReference` - Would create a circular reference chain
    async fn bind_referrer(
        &mut self,
        user: &'a CompressedPublicKey,
        referrer: &'a CompressedPublicKey,
        tx_hash: &'a Hash,
    ) -> Result<(), E>;

    /// Distribute referral rewards to uplines
    ///
    /// # Arguments
    /// * `from_user` - The user whose uplines will receive rewards
    /// * `asset` - The asset to distribute
    /// * `total_amount` - Total amount to distribute
    /// * `ratios` - Reward ratios for each level (basis points, 100 = 1%)
    ///
    /// # Returns
    /// * Distribution result with details of each transfer made
    async fn distribute_referral_rewards(
        &mut self,
        from_user: &'a CompressedPublicKey,
        asset: &'a Hash,
        total_amount: u64,
        ratios: &[u16],
    ) -> Result<crate::referral::DistributionResult, E>;
}
