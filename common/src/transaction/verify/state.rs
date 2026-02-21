use std::{borrow::Cow, collections::HashMap};

use crate::{
    account::{AgentAccountMeta, Nonce, SessionKey},
    block::{Block, BlockVersion},
    contract::{
        AssetChanges, ChainState, ContractCache, ContractEventTracker, ContractOutput,
        ContractProvider,
    },
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash,
    },
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
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
        reference: &Reference,
    ) -> Result<&'b mut u64, E>;

    /// Apply new output to a sender account (plaintext u64)
    async fn add_sender_output(
        &mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
        output: u64,
    ) -> Result<(), E>;

    // ===== UNO (Privacy Balance) Methods =====

    /// Get the UNO (encrypted) balance for a receiver account
    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, E>;

    /// Get the UNO (encrypted) balance used for verification of funds for the sender account
    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        account: &'a CompressedPublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut Ciphertext, E>;

    /// Apply new output ciphertext to a sender's UNO account
    async fn add_sender_uno_output(
        &mut self,
        account: &'a CompressedPublicKey,
        asset: &'a Hash,
        output: Ciphertext,
    ) -> Result<(), E>;

    /// Get the nonce of an account
    async fn get_account_nonce(&mut self, account: &'a CompressedPublicKey) -> Result<Nonce, E>;

    /// Check if an account exists (registered on-chain)
    async fn account_exists(&mut self, account: &'a CompressedPublicKey) -> Result<bool, E>;

    /// Apply a new nonce to an account
    async fn update_account_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        new_nonce: Nonce,
    ) -> Result<(), E>;

    /// Atomically compare and swap nonce to prevent race conditions
    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, E>;

    // ===== Agent Account Methods =====

    async fn get_agent_account_meta(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<AgentAccountMeta>, E> {
        Ok(None)
    }

    async fn set_agent_account_meta(
        &mut self,
        _account: &'a CompressedPublicKey,
        _meta: &AgentAccountMeta,
    ) -> Result<(), E> {
        Ok(())
    }

    async fn delete_agent_account_meta(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<(), E> {
        Ok(())
    }

    async fn get_session_key(
        &mut self,
        _account: &'a CompressedPublicKey,
        _key_id: u64,
    ) -> Result<Option<SessionKey>, E> {
        Ok(None)
    }

    async fn set_session_key(
        &mut self,
        _account: &'a CompressedPublicKey,
        _session_key: &SessionKey,
    ) -> Result<(), E> {
        Ok(())
    }

    async fn delete_session_key(
        &mut self,
        _account: &'a CompressedPublicKey,
        _key_id: u64,
    ) -> Result<(), E> {
        Ok(())
    }

    async fn get_session_keys_for_account(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Vec<SessionKey>, E> {
        Ok(Vec::new())
    }

    /// Get the block version in which TX is executed
    fn get_block_version(&self) -> BlockVersion;

    /// Get the timestamp to use for verification
    fn get_verification_timestamp(&self) -> u64;

    /// Get the topoheight to use for verification
    fn get_verification_topoheight(&self) -> u64;

    /// Get the recyclable TOS amount from expired freeze records
    async fn get_recyclable_tos(&mut self, account: &'a CompressedPublicKey) -> Result<u64, E>;

    /// Set the multisig state for an account
    async fn set_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), E>;

    /// Get the multisig state for an account
    async fn get_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<&MultiSigPayload>, E>;

    /// Get the environment
    async fn get_environment(&mut self) -> Result<&Environment, E>;

    /// Get the network type (for chain_id validation)
    fn get_network(&self) -> crate::network::Network;

    /// Set the contract module
    async fn set_contract_module(&mut self, hash: &Hash, module: &'a Module) -> Result<(), E>;

    /// Load in the cache the contract module
    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, E>;

    /// Get the contract module with the environment
    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), E>;

    // ===== TNS (TOS Name Service) Verification Methods =====

    /// Check if a TNS name hash is registered
    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, E>;

    /// Check if an account already has a registered TNS name
    async fn account_has_name(&self, account: &'a CompressedPublicKey) -> Result<bool, E>;

    /// Get the TNS name hash for an account
    async fn get_account_name_hash(
        &self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, E>;
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

    /// Track the contract outputs
    async fn set_contract_outputs(
        &mut self,
        tx_hash: &'a Hash,
        outputs: Vec<ContractOutput>,
    ) -> Result<(), E>;

    /// Get the contract environment
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
    async fn remove_contract_module(&mut self, hash: &Hash) -> Result<(), E>;

    /// Get the contract executor for executing contracts
    fn get_contract_executor(&self) -> std::sync::Arc<dyn crate::contract::ContractExecutor>;

    /// Add contract events emitted during execution (LOG0-LOG4 syscalls)
    async fn add_contract_events(
        &mut self,
        events: Vec<crate::contract::ContractEvent>,
        contract: &Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), E>;

    // ===== TNS (TOS Name Service) Apply Methods =====

    /// Register a TNS name for an account
    async fn register_name(
        &mut self,
        name_hash: Hash,
        owner: &'a CompressedPublicKey,
    ) -> Result<(), E>;
}
