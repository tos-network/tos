/// TOS Kernel(TAKO) Executor for TOS Blockchain
///
/// This module provides the main execution engine for TOS Kernel(TAKO) contracts within TOS blockchain.
/// It handles bytecode loading, VM creation, execution, and result processing.
///
/// # Architecture
///
/// ```text
/// TOS Transaction
///     ↓
/// TakoExecutor::execute()
///     ↓
/// 1. Validate ELF bytecode
/// 2. Create adapters (Storage, Accounts, Loader)
/// 3. Load executable with syscalls
/// 4. Create InvokeContext with TOS state
/// 5. Execute in TBPF VM
/// 6. Process execution results
/// ```
use std::sync::Arc;
use tos_common::{
    block::TopoHeight,
    contract::{ContractCache, ContractProvider, TransferOutput},
    crypto::Hash,
};
use tos_program_runtime::invoke_context::InvokeContext;
use tos_tbpf::{
    aligned_memory::AlignedMemory,
    ebpf,
    elf::Executable,
    error::ProgramResult,
    memory_region::{MemoryMapping, MemoryRegion},
    program::BuiltinProgram,
    vm::{Config, ContextObject, EbpfVm},
};

use super::{
    NoOpNftStorage, SVMFeatureSet, TakoExecutionError, TosAccountAdapter, TosContractLoaderAdapter,
    TosNativeAssetAdapter, TosNftAdapter, TosReferralAdapter, TosStorageAdapter,
};
use crate::core::storage::{NativeAssetProvider, ReferralProvider};
use crate::vrf::VrfData;
use tos_common::nft::operations::NftStorage;

/// Default compute budget for contract execution (200,000 compute units)
///
/// This matches SVM default for simple transactions. Can be adjusted
/// based on TOS's requirements.
pub const DEFAULT_COMPUTE_BUDGET: u64 = 200_000;

/// Maximum compute budget allowed (10,000,000 compute units)
///
/// Prevents excessive computation. Can be increased for complex contracts.
pub const MAX_COMPUTE_BUDGET: u64 = 10_000_000;

/// Stack size for contract execution (256KB)
///
/// Matches TBPF's VM configuration: 4KB per frame × 64 max call depth = 256KB
/// This prevents StackAccessViolation errors when contracts use deep call stacks
/// or allocate large stack variables (e.g., OpenZeppelin vesting-wallet contract)
const STACK_SIZE: usize = 256 * 1024;

/// Heap size for dynamic memory allocation (32KB)
///
/// Allows contracts to use Vec, BTreeMap, and other heap-allocated types
/// via the tos-alloc library. Can be increased up to 256KB if needed.
const HEAP_SIZE: usize = 32 * 1024;

/// TOS Kernel(TAKO) executor for TOS blockchain
///
/// This is the main entry point for executing TOS Kernel(TAKO) contracts within TOS.
/// It manages the complete lifecycle of contract execution from bytecode loading
/// to result processing.
///
/// # Example
///
/// ```no_run
/// use tos_daemon::tako_integration::TakoExecutor;
/// use tos_common::crypto::Hash;
///
/// # // Mock provider for doc-test (not actually functional)
/// # struct MockProvider;
/// # impl tos_common::contract::ContractProvider for MockProvider {
/// #     fn get_contract_balance_for_asset(&self, _: &Hash, _: &Hash, _: u64) -> Result<Option<(u64, u64)>, anyhow::Error> { Ok(None) }
/// #     fn get_account_balance_for_asset(&self, _: &tos_common::crypto::PublicKey, _: &Hash, _: u64) -> Result<Option<(u64, u64)>, anyhow::Error> { Ok(None) }
/// #     fn asset_exists(&self, _: &Hash, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn load_asset_data(&self, _: &Hash, _: u64) -> Result<Option<(u64, tos_common::asset::AssetData)>, anyhow::Error> { Ok(None) }
/// #     fn load_asset_supply(&self, _: &Hash, _: u64) -> Result<Option<(u64, u64)>, anyhow::Error> { Ok(None) }
/// #     fn account_exists(&self, _: &tos_common::crypto::PublicKey, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn load_contract_module(&self, _: &Hash, _: u64) -> Result<Option<Vec<u8>>, anyhow::Error> { Ok(None) }
/// # }
/// # impl tos_common::contract::ContractStorage for MockProvider {
/// #     fn load_data(&self, _: &Hash, _: &tos_kernel::ValueCell, _: u64) -> Result<Option<(u64, Option<tos_kernel::ValueCell>)>, anyhow::Error> { Ok(None) }
/// #     fn load_data_latest_topoheight(&self, _: &Hash, _: &tos_kernel::ValueCell, _: u64) -> Result<Option<u64>, anyhow::Error> { Ok(None) }
/// #     fn has_data(&self, _: &Hash, _: &tos_kernel::ValueCell, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn has_contract(&self, _: &Hash, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
/// # }
/// #
/// let bytecode = b"\x7FELF"; // Minimal ELF header (for demonstration)
/// let mut provider = MockProvider;
///
/// // Execute TAKO contract
/// let result = TakoExecutor::execute(
///     bytecode,
///     &mut provider,
///     100,              // topoheight
///     &Hash::zero(),    // contract_hash
///     &Hash::zero(),    // block_hash
///     1000,             // block_height
///     1700000000,       // block_timestamp
///     &Hash::zero(),    // tx_hash
///     &Hash::zero(),    // tx_sender
///     &[],              // input_data (entry point + args)
///     Some(200_000),    // compute_budget
/// );
/// ```
pub struct TakoExecutor;

/// Result of contract execution
#[derive(Debug)]
pub struct ExecutionResult {
    /// Program return value (0 = success, non-zero = error code)
    pub return_value: u64,
    /// Number of instructions executed
    pub instructions_executed: u64,
    /// Compute units consumed
    pub compute_units_used: u64,
    /// Return data set by the contract (if any)
    pub return_data: Option<Vec<u8>>,
    /// Log messages emitted by the contract during execution
    /// Format: "Program log: ...", "Program data: ...", "Program consumption: ..."
    pub log_messages: Vec<String>,
    /// Events emitted by the contract during execution (Ethereum-style)
    /// Contains indexed topics and data for off-chain indexing and monitoring
    pub events: Vec<tos_program_runtime::Event>,
    /// Transfers requested via the AccountProvider interface during execution
    pub transfers: Vec<TransferOutput>,
    /// Contract storage cache (for manual persistence in test environments)
    /// NOTE: In production, cache persistence is handled by the transaction apply phase.
    /// For testing, this cache must be manually persisted to storage after execution.
    pub cache: tos_common::contract::ContractCache,
}

impl TakoExecutor {
    /// Execute a TOS Kernel(TAKO) contract with production feature set
    ///
    /// This method uses the production feature set (V0-V3 support) by default.
    /// For custom feature flags, use `execute_with_features()`.
    ///
    /// # Arguments
    ///
    /// * `bytecode` - ELF bytecode of the contract
    /// * `provider` - TOS contract provider (for storage, accounts, etc.)
    /// * `topoheight` - Current topoheight for versioned reads
    /// * `contract_hash` - Hash of the contract being executed
    /// * `block_hash` - Current block hash
    /// * `block_height` - Current block height
    /// * `tx_hash` - Transaction hash
    /// * `tx_sender` - Transaction sender's public key
    /// * `input_data` - Input data for the contract
    /// * `compute_budget` - Maximum compute units allowed
    ///
    /// # Returns
    ///
    /// `ExecutionResult` containing return value, compute usage, and return data
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Bytecode is not valid ELF format
    /// - VM creation fails
    /// - Execution fails (out of compute, invalid memory access, etc.)
    /// - Compute budget exceeds maximum
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        bytecode: &[u8],
        provider: &mut (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,  // Using Hash type for sender (32 bytes)
        input_data: &[u8], // Contract input parameters (entry point, user data)
        compute_budget: Option<u64>,
    ) -> Result<ExecutionResult, TakoExecutionError> {
        // Use production feature set (V0-V3) by default, matching Solana
        Self::execute_with_features(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            block_timestamp,
            tx_hash,
            tx_sender,
            input_data,
            compute_budget,
            &SVMFeatureSet::production(),
        )
    }

    /// Execute a TOS Kernel(TAKO) contract with VRF (Verifiable Random Function) data
    ///
    /// This method provides access to verifiable randomness for smart contracts.
    /// Block producers sign block hashes with their VRF key, and contracts can
    /// access this verifiable random value via syscalls.
    ///
    /// # VRF Syscalls Enabled
    ///
    /// - `vrf_random` - Get VRF output + proof + derived random (500 CU)
    /// - `vrf_verify` - Verify VRF proof (3000 CU)
    /// - `vrf_public_key` - Get block producer's VRF public key (100 CU)
    ///
    /// # Arguments
    ///
    /// * `vrf_data` - Optional VRF data (public_key, output, proof) from block producer
    /// * `miner_public_key` - Block producer's compressed public key for VRF identity binding
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_vrf(
        bytecode: &[u8],
        provider: &mut (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,
        input_data: &[u8],
        compute_budget: Option<u64>,
        vrf_data: Option<&VrfData>,
        miner_public_key: Option<&[u8; 32]>,
    ) -> Result<ExecutionResult, TakoExecutionError> {
        Self::execute_with_features_and_referral(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            block_timestamp,
            tx_hash,
            tx_sender,
            input_data,
            compute_budget,
            &SVMFeatureSet::production(),
            None, // No referral provider
            vrf_data,
            miner_public_key,
        )
    }

    /// Execute a TOS Kernel(TAKO) contract with referral system access
    ///
    /// This method is similar to `execute()` but provides access to the native
    /// referral system via syscalls. Use this when executing contracts that need
    /// to query referral relationships, team sizes, or upline information.
    ///
    /// # Referral Syscalls Enabled
    ///
    /// - `tos_has_referrer` - Check if user has referrer (500 CU)
    /// - `get_referrer` - Get user's referrer address (500 CU)
    /// - `get_uplines` - Get N levels of uplines (500 + 200*N CU)
    /// - `get_direct_referrals_count` - Count of direct referrals (500 CU)
    /// - `get_team_size` - Team size from cache (500 CU)
    /// - `get_level` - User's level in referral tree (500 CU)
    /// - `tos_is_downline` - Check downline relationship (500 + 100*depth CU)
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_referral(
        bytecode: &[u8],
        provider: &mut (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,
        input_data: &[u8],
        compute_budget: Option<u64>,
        referral_provider: &mut (dyn ReferralProvider + Send + Sync),
    ) -> Result<ExecutionResult, TakoExecutionError> {
        Self::execute_with_features_and_referral(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            block_timestamp,
            tx_hash,
            tx_sender,
            input_data,
            compute_budget,
            &SVMFeatureSet::production(),
            Some(referral_provider),
            None, // VRF data not available in this method
            None, // Miner public key not available
        )
    }

    /// Execute a TOS Kernel(TAKO) contract with custom feature set
    ///
    /// This method allows specifying which TBPF versions are enabled,
    /// matching Solana's dynamic version control via SVMFeatureSet.
    ///
    /// # TBPF Version Support
    ///
    /// | Version | e_flags | Features |
    /// |---------|---------|----------|
    /// | V0 | 0 | Legacy format |
    /// | V1 | 1 | Dynamic stack frames |
    /// | V2 | 2 | Arithmetic improvements |
    /// | V3 | 3 | Static syscalls, stricter ELF |
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tos_daemon::tako_integration::{TakoExecutor, SVMFeatureSet};
    /// use tos_common::crypto::Hash;
    ///
    /// # // Mock provider for doc-test (not actually functional)
    /// # struct MockProvider;
    /// # impl tos_common::contract::ContractProvider for MockProvider {
    /// #     fn get_contract_balance_for_asset(&self, _: &Hash, _: &Hash, _: u64) -> Result<Option<(u64, u64)>, anyhow::Error> { Ok(None) }
    /// #     fn get_account_balance_for_asset(&self, _: &tos_common::crypto::PublicKey, _: &Hash, _: u64) -> Result<Option<(u64, u64)>, anyhow::Error> { Ok(None) }
    /// #     fn asset_exists(&self, _: &Hash, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
    /// #     fn load_asset_data(&self, _: &Hash, _: u64) -> Result<Option<(u64, tos_common::asset::AssetData)>, anyhow::Error> { Ok(None) }
    /// #     fn load_asset_supply(&self, _: &Hash, _: u64) -> Result<Option<(u64, u64)>, anyhow::Error> { Ok(None) }
    /// #     fn account_exists(&self, _: &tos_common::crypto::PublicKey, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
    /// #     fn load_contract_module(&self, _: &Hash, _: u64) -> Result<Option<Vec<u8>>, anyhow::Error> { Ok(None) }
    /// # }
    /// # impl tos_common::contract::ContractStorage for MockProvider {
    /// #     fn load_data(&self, _: &Hash, _: &tos_kernel::ValueCell, _: u64) -> Result<Option<(u64, Option<tos_kernel::ValueCell>)>, anyhow::Error> { Ok(None) }
    /// #     fn load_data_latest_topoheight(&self, _: &Hash, _: &tos_kernel::ValueCell, _: u64) -> Result<Option<u64>, anyhow::Error> { Ok(None) }
    /// #     fn has_data(&self, _: &Hash, _: &tos_kernel::ValueCell, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
    /// #     fn has_contract(&self, _: &Hash, _: u64) -> Result<bool, anyhow::Error> { Ok(false) }
    /// # }
    /// #
    /// let bytecode = b"\x7FELF"; // Minimal ELF header (for demonstration)
    /// let mut provider = MockProvider;
    ///
    /// // Execute with V3-only support
    /// let features = SVMFeatureSet::v3_only();
    /// let result = TakoExecutor::execute_with_features(
    ///     bytecode,
    ///     &mut provider,
    ///     100,              // topoheight
    ///     &Hash::zero(),    // contract_hash
    ///     &Hash::zero(),    // block_hash
    ///     1000,             // block_height
    ///     1700000000,       // block_timestamp
    ///     &Hash::zero(),    // tx_hash
    ///     &Hash::zero(),    // tx_sender
    ///     &[],              // input_data
    ///     Some(200_000),    // compute_budget
    ///     &features,
    /// );
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_features(
        bytecode: &[u8],
        provider: &mut (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,  // Using Hash type for sender (32 bytes)
        input_data: &[u8], // Contract input parameters (entry point, user data)
        compute_budget: Option<u64>,
        feature_set: &SVMFeatureSet,
    ) -> Result<ExecutionResult, TakoExecutionError> {
        // Execute without referral provider, VRF, or miner identity (backward compatibility)
        Self::execute_with_features_and_referral(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            block_timestamp,
            tx_hash,
            tx_sender,
            input_data,
            compute_budget,
            feature_set,
            None, // No referral provider
            None, // No VRF data
            None, // No miner public key
        )
    }

    /// Execute a TOS Kernel(TAKO) contract with custom feature set and referral provider
    ///
    /// This method allows specifying which TBPF versions are enabled and provides
    /// access to the native referral system via syscalls.
    ///
    /// # Referral System Access
    ///
    /// When `referral_provider` is provided, contracts can access the native referral
    /// system via these syscalls:
    /// - `tos_has_referrer` - Check if user has referrer
    /// - `get_referrer` - Get user's referrer address
    /// - `get_uplines` - Get N levels of uplines
    /// - `get_direct_referrals_count` - Count of direct referrals
    /// - `get_team_size` - Team size (cached)
    /// - `get_level` - User's level in referral tree
    /// - `tos_is_downline` - Check if user is in another's downline
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_features_and_referral(
        bytecode: &[u8],
        provider: &mut (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,  // Using Hash type for sender (32 bytes)
        input_data: &[u8], // Contract input parameters (entry point, user data)
        compute_budget: Option<u64>,
        feature_set: &SVMFeatureSet,
        referral_provider: Option<&mut (dyn ReferralProvider + Send + Sync)>,
        vrf_data: Option<&VrfData>, // VRF data for verifiable randomness
        miner_public_key: Option<&[u8; 32]>, // Block producer's key for VRF identity binding
    ) -> Result<ExecutionResult, TakoExecutionError> {
        Self::execute_with_all_providers::<NoOpNftStorage>(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            block_timestamp,
            tx_hash,
            tx_sender,
            input_data,
            compute_budget,
            feature_set,
            referral_provider,
            None, // No NFT provider
            None, // No native asset provider
            vrf_data,
            miner_public_key,
        )
    }

    /// Execute a TOS Kernel(TAKO) contract with all available providers
    ///
    /// This is the most comprehensive execution method, supporting all provider types:
    /// - Referral provider for accessing the native referral system
    /// - NFT provider for accessing the native NFT system
    /// - Native asset provider for accessing the native asset system
    /// - VRF data for verifiable randomness
    ///
    /// # NFT System Access
    ///
    /// When `nft_provider` is provided, contracts can access the native NFT system
    /// via syscalls for:
    /// - Collection management (create, pause)
    /// - Token operations (mint, burn, transfer)
    /// - Ownership queries (owner_of, balance_of)
    /// - Approval management (approve, set_approval_for_all)
    ///
    /// # Native Asset System Access
    ///
    /// When `native_asset_provider` is provided, contracts can access the native asset
    /// system via syscalls for:
    /// - Asset creation and management (create_asset, asset_exists)
    /// - Token operations (transfer, mint, burn)
    /// - Balance queries (balance_of, total_supply)
    /// - Approval management (approve, allowance, transfer_from)
    /// - Governance features (delegate, lock, timelock)
    /// - Role management (grant_role, revoke_role, has_role)
    /// - Advanced features (escrow, permit, agent operations)
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_all_providers<N: NftStorage>(
        bytecode: &[u8],
        provider: &mut (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,  // Using Hash type for sender (32 bytes)
        input_data: &[u8], // Contract input parameters (entry point, user data)
        compute_budget: Option<u64>,
        feature_set: &SVMFeatureSet,
        referral_provider: Option<&mut (dyn ReferralProvider + Send + Sync)>,
        nft_provider: Option<&mut N>, // NFT storage provider
        native_asset_provider: Option<&mut (dyn NativeAssetProvider + Send + Sync)>, // Native asset provider
        vrf_data: Option<&VrfData>, // VRF data for verifiable randomness
        miner_public_key: Option<&[u8; 32]>, // Block producer's key for VRF identity binding
    ) -> Result<ExecutionResult, TakoExecutionError> {
        use log::{debug, error, info, warn};

        if log::log_enabled!(log::Level::Info) {
            info!(
                "TOS Kernel(TAKO) execution starting: contract={}, compute_budget={}, bytecode_size={}, tbpf_versions={:?}..={:?}",
                contract_hash,
                compute_budget.unwrap_or(DEFAULT_COMPUTE_BUDGET),
                bytecode.len(),
                feature_set.min_tbpf_version(),
                feature_set.max_tbpf_version()
            );
        }

        // 1. Validate compute budget
        let compute_budget = compute_budget.unwrap_or(DEFAULT_COMPUTE_BUDGET);
        if compute_budget > MAX_COMPUTE_BUDGET {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "Compute budget validation failed: requested={}, maximum={}",
                    compute_budget, MAX_COMPUTE_BUDGET
                );
            }
            return Err(TakoExecutionError::ComputeBudgetExceeded {
                requested: compute_budget,
                maximum: MAX_COMPUTE_BUDGET,
            });
        }

        // 2. Check if this is a precompile instruction
        // Precompiles are identified by their program ID (first 32 bytes of bytecode in this context)
        // Note: In a real transaction, program_id would be separate from bytecode
        // For now, we use contract_hash as the program ID
        let program_id = contract_hash.as_bytes();

        // 3. Create TOS adapters (needed for both precompile and regular execution)
        let mut cache = ContractCache::default();
        let mut storage = TosStorageAdapter::new(provider, contract_hash, &mut cache, topoheight);
        let mut accounts = TosAccountAdapter::new(provider, topoheight);
        let loader_adapter = TosContractLoaderAdapter::new(provider, topoheight);

        // 3a. Create referral adapter (if provider is available)
        // Created before InvokeContext to ensure proper lifetime (adapter must outlive InvokeContext)
        let mut referral_adapter =
            referral_provider.map(|p| TosReferralAdapter::new(p, topoheight));

        // 3b. Create NFT adapter (if provider is available)
        // NFT adapter bridges TAKO's NftProvider trait with TOS's NftStorage operations
        let mut nft_adapter = nft_provider.map(TosNftAdapter::new);

        // 3c. Create native asset adapter (if provider is available)
        // Native asset adapter bridges TAKO's NativeAssetProvider trait with TOS's native asset storage
        let mut native_asset_adapter =
            native_asset_provider.map(|p| TosNativeAssetAdapter::new(p, block_height));

        // 4. Create TBPF loader with syscalls (needed for InvokeContext creation)
        // Note: JIT compilation is enabled via the "jit" feature in Cargo.toml
        // This provides 10-50x performance improvement over interpreter-only execution
        //
        // TBPF Version Configuration (aligned with Solana):
        // - enabled_tbpf_versions: Controls which ELF e_flags values are accepted
        // - aligned_memory_mapping: Controlled by stricter_abi_and_runtime_constraints
        let mut config = Config::default();
        config.max_call_depth = 64; // Standard limit
        config.enabled_tbpf_versions = feature_set.enabled_tbpf_versions();
        config.aligned_memory_mapping = feature_set.use_aligned_memory_mapping();
        let mut loader = BuiltinProgram::<InvokeContext>::new_loader(config.clone());
        tos_syscalls::register_syscalls(&mut loader).map_err(|e| {
            TakoExecutionError::SyscallRegistrationFailed {
                reason: "Syscall registration error".to_string(),
                error_details: format!("{:?}", e),
            }
        })?;
        let loader = Arc::new(loader);

        // 5. Load executable
        // Record bytecode size for loaded data accounting (done after InvokeContext creation)
        let bytecode_size = bytecode.len() as u64;
        let executable = Executable::load(bytecode, loader.clone()).map_err(|e| {
            // Log detailed ELF parsing error for debugging
            if log::log_enabled!(log::Level::Error) {
                error!(
                    "ELF parsing failed for contract {}: bytecode_size={}, error={:?}",
                    contract_hash,
                    bytecode.len(),
                    e
                );
            }
            // Also log first 64 bytes of bytecode for debugging
            if log::log_enabled!(log::Level::Debug) {
                let header_bytes: Vec<u8> = bytecode.iter().take(64).cloned().collect();
                debug!("ELF header bytes: {:02x?}", header_bytes);
            }
            TakoExecutionError::ExecutableLoadFailed {
                reason: "ELF parsing failed".to_string(),
                bytecode_size: bytecode.len(),
                error_details: format!("{:?}", e),
            }
        })?;

        // 6. Create InvokeContext with TOS blockchain state
        let mut invoke_context = InvokeContext::new_with_state(
            compute_budget,
            *contract_hash.as_bytes(),
            *block_hash.as_bytes(),
            block_height,
            block_timestamp,
            *tx_hash.as_bytes(),
            *tx_sender.as_bytes(),
            &mut storage,
            &mut accounts,
            &loader_adapter,
            loader.clone(),
        );

        // 7. Inject instant randomness for randomness syscalls
        // CRITICAL: This must be set before contract execution to prevent
        // get_instant_random from returning error code 1
        invoke_context.instant_random = Some(Self::generate_instant_randomness(
            block_hash.as_bytes(),
            block_height,
            block_timestamp,
            tx_hash.as_bytes(),
        ));

        // 7b. Inject VRF data for verifiable randomness syscalls (if available)
        // When enabled, block producers sign the block_hash with their VRF key
        // to provide provably random values to contracts via vrf_random()
        if let Some(vrf) = vrf_data {
            invoke_context.vrf_public_key = Some(vrf.public_key.to_bytes());
            invoke_context.vrf_output = Some(vrf.output.to_bytes());
            invoke_context.vrf_proof = Some(vrf.proof.to_bytes());

            // Set miner public key for VRF identity binding
            // This is required for validate_vrf() to compute the correct VRF input:
            // vrf_input = BLAKE3("TOS-VRF-INPUT-v1" || block_hash || miner_public_key)
            invoke_context.miner_public_key = miner_public_key.copied();

            // Validate VRF to set vrf_validated_hash
            // This ensures contracts can only access validated VRF data
            // SECURITY: Invalid VRF data is a hard error - do not continue execution
            invoke_context.validate_vrf().map_err(|e| {
                if log::log_enabled!(log::Level::Error) {
                    error!("VRF validation failed: {}", e);
                }
                TakoExecutionError::VrfValidationFailed(e)
            })?;

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "VRF data injected: public_key={}, output={}, miner={}",
                    hex::encode(vrf.public_key.as_bytes()),
                    hex::encode(vrf.output.as_bytes()),
                    miner_public_key.map_or_else(|| "none".to_string(), hex::encode)
                );
            }
        }

        // 7c. Wire referral provider (if available)
        // Enables contracts to access native referral system via get_uplines, etc.
        if let Some(ref mut adapter) = referral_adapter {
            invoke_context.set_referral_provider(adapter);
            if log::log_enabled!(log::Level::Debug) {
                debug!("Referral provider wired to InvokeContext");
            }
        }

        // 7d. Wire NFT provider (if available)
        // Enables contracts to access native NFT system via nft_mint, nft_transfer, etc.
        if let Some(ref mut adapter) = nft_adapter {
            invoke_context.set_nft_provider(adapter);
            if log::log_enabled!(log::Level::Debug) {
                debug!("NFT provider wired to InvokeContext");
            }
        }

        // 7e. Wire native asset provider (if available)
        // Enables contracts to access native asset system via asset syscalls
        if let Some(ref mut adapter) = native_asset_adapter {
            invoke_context.set_asset_provider(adapter);
            if log::log_enabled!(log::Level::Debug) {
                debug!("Native asset provider wired to InvokeContext");
            }
        }

        // Account for entry contract bytecode in loaded data size tracking
        // This is done AFTER creating InvokeContext (post-load accounting pattern)
        // because the bytecode is loaded before InvokeContext exists
        invoke_context
            .check_and_record_loaded_data(bytecode_size)
            .map_err(|e| {
                if log::log_enabled!(log::Level::Error) {
                    error!(
                        "Entry contract bytecode size {} exceeds loaded data limit: {:?}",
                        bytecode_size, e
                    );
                }
                TakoExecutionError::LoadedDataLimitExceeded {
                    current_size: bytecode_size,
                    limit: invoke_context
                        .get_compute_budget_limits()
                        .loaded_accounts_bytes
                        .get() as u64,
                    operation: "entry_contract_load".to_string(),
                    details: format!("Contract bytecode is {} bytes", bytecode_size),
                }
            })?;

        if log::log_enabled!(log::Level::Info) {
            info!(
                "Entry contract bytecode accounted: {} bytes (remaining: {} bytes)",
                bytecode_size,
                invoke_context.get_remaining_loaded_data_size()
            );
        }

        // Set input data for contract to access via get_input_data syscall
        // This allows contracts to receive parameters (entry points, constructors, user args)
        invoke_context.set_input_data(input_data.to_vec());

        // Enable log collection to capture contract logs (Program log: ...)
        // This allows transaction results to include all log messages emitted by the contract
        invoke_context.enable_log_collection();

        // Enable event collection to capture Ethereum-style events emitted by the contract
        // Events are used for off-chain indexing, monitoring, and real-time notifications
        invoke_context.enable_event_collection();

        // Enable debug mode if TOS is in debug mode
        #[cfg(debug_assertions)]
        invoke_context.enable_debug();

        // 6.5. Check if this is a precompile and handle it separately
        if invoke_context.is_precompile(program_id) {
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Executing precompile: program_id={:?}",
                    hex::encode(program_id)
                );
            }

            // For precompiles, input_data IS the instruction data
            // In a real transaction, we would also need instruction_datas from other instructions
            // For now, we only support single-instruction precompile calls
            let instruction_datas = vec![input_data];
            let instruction_data_refs: Vec<&[u8]> =
                instruction_datas.iter().map(|data| *data).collect();

            // Verify the precompile
            invoke_context
                .process_precompile(program_id, input_data, &instruction_data_refs)
                .map_err(|e| {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Precompile verification failed: {:?}", e);
                    }
                    TakoExecutionError::PrecompileVerificationFailed {
                        program_id: hex::encode(program_id),
                        error_details: format!("{:?}", e),
                    }
                })?;

            // Precompile verification succeeded
            // Return success result (precompiles consume 0 CU at runtime)
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Precompile verification succeeded: program_id={:?}",
                    hex::encode(program_id)
                );
            }

            // Extract log messages and events (if any)
            let log_messages = invoke_context.extract_log_messages().unwrap_or_default();
            let events = invoke_context.extract_events().unwrap_or_default();

            // Get return data (precompiles don't typically return data)
            let return_data = invoke_context
                .get_return_data()
                .map(|(_, data)| data.to_vec());

            // Drop InvokeContext to release borrows
            drop(invoke_context);

            // Get transfers (precompiles don't typically do transfers)
            let transfers = accounts.take_pending_transfers();

            return Ok(ExecutionResult {
                return_value: 0,          // Success
                instructions_executed: 0, // Precompiles don't execute instructions
                compute_units_used: 0, // FREE at runtime (cost charged during transaction validation)
                return_data,
                log_messages,
                events,
                transfers,
                cache, // Return cache for test persistence
            });
        }

        // 7. Not a precompile - proceed with regular contract execution
        // Validate ELF bytecode
        if log::log_enabled!(log::Level::Debug) {
            debug!("Validating ELF bytecode: size={} bytes", bytecode.len());
        }
        tos_common::contract::validate_contract_bytecode(bytecode).map_err(|e| {
            if log::log_enabled!(log::Level::Error) {
                error!("Bytecode validation failed: {:?}", e);
            }
            TakoExecutionError::invalid_bytecode("Invalid ELF format", Some(e))
        })?;

        // 8. Create memory mapping WITH HEAP
        let mut stack = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(STACK_SIZE);
        let stack_len = stack.len();

        // Create heap memory for dynamic allocation
        let mut heap = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(HEAP_SIZE);

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Created heap: {} KB at 0x{:x}",
                HEAP_SIZE / 1024,
                ebpf::MM_HEAP_START
            );
        }

        // Add heap to memory regions
        let regions: Vec<MemoryRegion> = vec![
            executable.get_ro_region(),
            MemoryRegion::new_writable(stack.as_slice_mut(), ebpf::MM_STACK_START),
            MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
        ];
        let memory_mapping = MemoryMapping::new(regions, &config, executable.get_tbpf_version())
            .map_err(|e| TakoExecutionError::MemoryMappingFailed {
                reason: "Memory region setup failed".to_string(),
                stack_size: STACK_SIZE,
                error_details: format!("{:?}", e),
            })?;

        // 8. Create VM
        let mut vm = EbpfVm::new(
            executable.get_loader().clone(),
            executable.get_tbpf_version(),
            &mut invoke_context,
            memory_mapping,
            stack_len,
        );

        // 9. Execute contract
        if log::log_enabled!(log::Level::Debug) {
            debug!("Executing contract bytecode via TBPF VM");
        }
        let (instruction_count, result) = vm.execute_program(&executable, true); // true = interpreter mode

        // 10. Calculate compute units used (before dropping invoke_context)
        let compute_units_used = compute_budget - invoke_context.get_remaining();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Execution complete: instructions={}, compute_units_used={}/{}",
                instruction_count, compute_units_used, compute_budget
            );
        }

        // 11. Extract log messages from contract execution
        let log_messages = invoke_context.extract_log_messages().unwrap_or_default();
        if log::log_enabled!(log::Level::Debug) {
            if !log_messages.is_empty() {
                debug!("Contract emitted {} log messages", log_messages.len());
            }
        }

        // 11a. Extract events from contract execution
        let events = invoke_context.extract_events().unwrap_or_default();
        if log::log_enabled!(log::Level::Debug) {
            if !events.is_empty() {
                debug!("Contract emitted {} events", events.len());
            }
        }

        // 12. Get return data (if any)
        let return_data = invoke_context
            .get_return_data()
            .map(|(_, data)| data.to_vec());

        // Drop InvokeContext to release the mutable borrow of accounts
        drop(invoke_context);

        // Now we can access accounts again to extract pending transfers
        let transfers = accounts.take_pending_transfers();

        // CRITICAL: Drop heap and stack after execution to prevent dangling pointers
        drop(heap);
        drop(stack);

        // 13. Process result
        match result {
            ProgramResult::Ok(return_value) => {
                if log::log_enabled!(log::Level::Info) {
                    info!(
                        "TOS Kernel(TAKO) execution succeeded: return_value={}, instructions={}, compute_units={}, return_data_size={}, log_count={}, event_count={}, stack_allocated={}KB, heap_allocated={}KB",
                        return_value,
                        instruction_count,
                        compute_units_used,
                        return_data.as_ref().map(|d| d.len()).unwrap_or(0),
                        log_messages.len(),
                        events.len(),
                        STACK_SIZE / 1024,
                        HEAP_SIZE / 1024
                    );
                }
                Ok(ExecutionResult {
                    return_value,
                    instructions_executed: instruction_count,
                    compute_units_used,
                    return_data,
                    log_messages,
                    events,
                    transfers,
                    cache, // Return cache for test persistence
                })
            }
            ProgramResult::Err(err) => {
                let execution_error =
                    TakoExecutionError::from_ebpf_error(err, instruction_count, compute_units_used);
                if log::log_enabled!(log::Level::Error) {
                    error!(
                        "TOS Kernel(TAKO) execution failed: category={}, error={}, log_count={}, stack_allocated={}KB, heap_allocated={}KB",
                        execution_error.category(),
                        execution_error.user_message(),
                        log_messages.len(),
                        STACK_SIZE / 1024,
                        HEAP_SIZE / 1024
                    );
                }
                // Note: Log messages are lost on error for now
                // Future: Could extend TakoExecutionError to include log_messages for debugging
                Err(execution_error)
            }
        }
    }

    /// Execute a contract with minimal parameters (uses defaults for blockchain state)
    ///
    /// This is a convenience method for testing. Production code should use
    /// the full `execute()` method with proper blockchain state.
    pub fn execute_simple<P: ContractProvider + Send>(
        bytecode: &[u8],
        provider: &mut P,
        topoheight: TopoHeight,
        contract_hash: &Hash,
    ) -> Result<ExecutionResult, TakoExecutionError> {
        Self::execute(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            &Hash::zero(), // block_hash
            0,             // block_height
            0,             // block_timestamp
            &Hash::zero(), // tx_hash
            &Hash::zero(), // tx_sender
            &[],           // input_data
            None,          // compute_budget (use default)
        )
    }

    /// Generate instant randomness for randomness syscalls
    ///
    /// Uses multi-layer entropy from TOS blockchain state:
    /// - Block hash: BlockDAG + POW entropy
    /// - Block height: Temporal entropy
    /// - Block timestamp: Additional temporal entropy
    /// - Transaction hash: Per-transaction entropy
    ///
    /// This implements TOS's instant randomness model which is:
    /// - Unpredictable: No advance knowledge due to POW randomness
    /// - Unbiasable: Cannot skip blocks in BlockDAG (all blocks count)
    /// - Deterministic: Same inputs produce same randomness
    /// - 0-delay: Available immediately
    ///
    /// # Security Properties
    ///
    /// For contracts requiring high-value randomness (>$1000), use the
    /// delayed randomness syscalls (commit/reveal) instead, which provide
    /// 10-second delay and future block entropy.
    ///
    /// # Arguments
    ///
    /// * `block_hash` - 32-byte block hash from BlockDAG
    /// * `block_height` - Block height in the chain
    /// * `block_timestamp` - Unix timestamp of the block
    /// * `tx_hash` - 32-byte transaction hash
    ///
    /// # Returns
    ///
    /// 32-byte array of cryptographically secure random data
    fn generate_instant_randomness(
        block_hash: &[u8],
        block_height: u64,
        block_timestamp: u64,
        tx_hash: &[u8],
    ) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        // Combine multiple entropy sources using Keccak256
        let mut hasher = Keccak256::new();

        // Entropy source 1: Block hash (BlockDAG + POW entropy)
        hasher.update(b"INSTANT_RANDOM_V1");
        hasher.update(block_hash);

        // Entropy source 2: Block height (temporal entropy)
        hasher.update(block_height.to_le_bytes());

        // Entropy source 3: Block timestamp (additional temporal entropy)
        hasher.update(block_timestamp.to_le_bytes());

        // Entropy source 4: Transaction hash (per-transaction entropy)
        hasher.update(tx_hash);

        // Finalize to 32-byte random value
        let result = hasher.finalize();
        let mut random = [0u8; 32];
        random.copy_from_slice(&result);

        random
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tos_common::{
        asset::AssetData,
        crypto::{Hash, PublicKey},
    };
    use tos_kernel::ValueCell;
    use tos_program_runtime::storage::{InMemoryStorage, StorageProvider};

    // Mock provider for testing
    struct MockProvider {
        storage: InMemoryStorage,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                storage: InMemoryStorage::new(),
            }
        }
    }

    impl tos_common::contract::ContractProvider for MockProvider {
        fn get_contract_balance_for_asset(
            &self,
            _contract: &Hash,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn get_account_balance_for_asset(
            &self,
            _key: &PublicKey,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(Some((100, 1000000)))
        }

        fn asset_exists(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }

        fn load_asset_data(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, AssetData)>, anyhow::Error> {
            Ok(None)
        }

        fn load_asset_supply(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn account_exists(
            &self,
            _key: &PublicKey,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }

        fn load_contract_module(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<Vec<u8>>, anyhow::Error> {
            Ok(None)
        }
    }

    impl tos_common::contract::ContractStorage for MockProvider {
        fn load_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
            // Use InMemoryStorage through StorageProvider trait
            let key_bytes = bincode::serialize(key)?;
            match self.storage.get(contract.as_bytes(), &key_bytes) {
                Ok(Some(data)) => {
                    let value: ValueCell = bincode::deserialize(&data)?;
                    Ok(Some((100, Some(value))))
                }
                Ok(None) => Ok(None),
                Err(_) => Ok(None),
            }
        }

        fn load_data_latest_topoheight(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(Some(100))
        }

        fn has_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            let key_bytes = bincode::serialize(key)?;
            match self.storage.get(contract.as_bytes(), &key_bytes) {
                Ok(result) => Ok(result.is_some()),
                Err(_) => Ok(false),
            }
        }

        fn has_contract(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
    }

    #[test]
    fn test_executor_validate_compute_budget() {
        let mut provider = MockProvider::new();
        let bytecode = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00"; // Minimal ELF header

        // Exceeds maximum
        let result = TakoExecutor::execute(
            bytecode,
            &mut provider,
            100,
            &Hash::zero(),
            &Hash::zero(),
            0,
            0,
            &Hash::zero(),
            &Hash::zero(),
            &[],
            Some(MAX_COMPUTE_BUDGET + 1),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_executor_validate_elf() {
        let mut provider = MockProvider::new();
        let invalid_bytecode = b"not an ELF file";

        let result =
            TakoExecutor::execute_simple(invalid_bytecode, &mut provider, 100, &Hash::zero());

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = err.to_string();
        // After precompile integration, ELF validation happens during executable loading
        // Error message: "Failed to load executable: ELF parsing failed"
        assert!(err_str.contains("ELF"));
    }

    #[test]
    fn test_loaded_data_limit_error_from_invoke_context() {
        use tos_tbpf::error::EbpfError;

        // Simulate the actual error message from InvokeContext::check_and_record_loaded_data
        // This is the format produced by invoke_context.rs:772-776
        let error_msg = "Loaded contract data size 70000000 exceeds limit 67108864 (tried to add 5000000 bytes)";
        let ebpf_err = EbpfError::SyscallError(error_msg.into());

        // Convert to TakoExecutionError - this is what happens in the executor
        let tako_err = TakoExecutionError::from_ebpf_error(ebpf_err, 1000, 500);

        // Verify it maps to LoadedDataLimitExceeded variant (KEY TEST for RPC error surfacing)
        match &tako_err {
            TakoExecutionError::LoadedDataLimitExceeded {
                current_size,
                limit,
                operation,
                details,
            } => {
                // Verify error details are parsed correctly from the string
                assert_eq!(*limit, 67108864, "Limit should be parsed as 64 MB");
                assert_eq!(
                    *current_size, 70000000,
                    "Current size should be parsed correctly"
                );
                assert!(!operation.is_empty(), "Operation should be determined");
                assert!(
                    details.contains("exceeds limit"),
                    "Details should contain full error"
                );

                // CRITICAL: Verify error category for RPC/metrics - what operators will see
                assert_eq!(tako_err.category(), "resource_limit");
            }
            _ => panic!(
                "Expected LoadedDataLimitExceeded error, got: {:?}",
                tako_err
            ),
        }

        // Verify the error has a user-friendly message for RPC responses
        let user_msg = tako_err.user_message();
        assert!(
            !user_msg.is_empty(),
            "Should have user message for RPC clients"
        );
    }

    #[test]
    fn test_loaded_data_limit_error_with_custom_limit() {
        use tos_tbpf::error::EbpfError;

        // Test with a non-default limit (128 MB) to verify parsing doesn't fall back to defaults
        let error_msg = "Loaded contract data size 135000000 exceeds limit 134217728 (tried to add 10000000 bytes)";
        let ebpf_err = EbpfError::SyscallError(error_msg.into());

        let tako_err = TakoExecutionError::from_ebpf_error(ebpf_err, 2000, 1000);

        match &tako_err {
            TakoExecutionError::LoadedDataLimitExceeded {
                current_size,
                limit,
                ..
            } => {
                // These would be 0 and 64MB if parsing failed
                assert_eq!(
                    *limit, 134217728,
                    "Limit should be 128 MB, not default 64 MB"
                );
                assert_eq!(
                    *current_size, 135000000,
                    "Current should be actual value, not 0"
                );
            }
            _ => panic!(
                "Expected LoadedDataLimitExceeded error, got: {:?}",
                tako_err
            ),
        }
    }

    // Note: Full integration test with actual contract execution requires
    // a compiled TAKO contract (.so file). See integration tests for that.
}
