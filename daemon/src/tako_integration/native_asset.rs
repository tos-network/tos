// Native Asset Adapter: TOS NativeAssetProvider → TAKO NativeAssetProvider
//
// This module bridges TOS's async NativeAssetProvider with TAKO's synchronous
// NativeAssetProvider trait, enabling smart contracts to access native assets
// (ERC20-like fungible tokens) via syscalls.
//
// Uses `try_block_on()` pattern for async/sync conversion, following the
// established pattern in referral.rs and scheduled_execution.rs.

use std::io;

use tos_common::{
    crypto::Hash,
    native_asset::{
        AgentAuthorization, Allowance, BalanceCheckpoint, Delegation, DelegationCheckpoint, Escrow,
        EscrowStatus, FreezeState, NativeAssetData, PauseState, ReleaseCondition, RoleId,
        SpendingLimit, SpendingPeriod, SupplyCheckpoint, TimelockOperation, TimelockStatus,
        TokenLock,
    },
    serializer::Serializer,
    tokio::try_block_on,
};
// TAKO's NativeAssetProvider trait (aliased to avoid conflict with TOS's provider)
use tos_program_runtime::storage::NativeAssetProvider as TakoNativeAssetProvider;
use tos_tbpf::error::EbpfError;

// TOS's async NativeAssetProvider trait
use crate::core::storage::NativeAssetProvider;

/// Chain ID for TOS network (used for domain separator in permits)
const TOS_CHAIN_ID: u64 = 1;

/// Adapter that wraps TOS's async NativeAssetProvider to implement TAKO's NativeAssetProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall (e.g., asset_transfer)
///     ↓
/// InvokeContext::asset_transfer()
///     ↓
/// TosNativeAssetAdapter::transfer() [TakoNativeAssetProvider]
///     ↓
/// try_block_on(NativeAssetProvider methods) [async → sync]
///     ↓
/// RocksDB storage operations
/// ```
///
/// # Thread Safety
///
/// This adapter uses `try_block_on()` which:
/// - Detects multi-thread runtime and uses `block_in_place`
/// - Falls back to `futures::executor::block_on` in single-thread context
/// - Proven pattern used throughout TOS blockchain
///
/// # Atomicity Note
///
/// Operations follow a phased approach (validate → vote power → balances) to minimize
/// inconsistency risk. However, true transactional atomicity requires RocksDB batch
/// writes (see TOS-026). In the unlikely event of a crash between phases, state may
/// become inconsistent. Future enhancement: integrate with atomic batch write system.
///
/// # Vote Power Migration Note
///
/// Vote power storage (`navp` prefix) is initialized when tokens are first minted via
/// `add_vote_power_for_mint()`. There is NO need for migration because:
/// - TOS mainnet has not launched yet (no existing accounts with balances)
/// - All new accounts will have vote power correctly initialized on first mint
/// - This code will be deployed before any tokens exist on the network
///
/// If TOS were to add vote checkpoints after mainnet launch, a migration would be
/// required to initialize vote power = balance for all existing accounts. Since we
/// are deploying this feature before launch, no such migration is necessary.
pub struct TosNativeAssetAdapter<'a, P: NativeAssetProvider + Send + Sync + ?Sized> {
    /// TOS native asset storage provider (mutable for write operations)
    provider: &'a mut P,
    /// Current block height for timestamp-based operations
    block_height: u64,
}

impl<'a, P: NativeAssetProvider + Send + Sync + ?Sized> TosNativeAssetAdapter<'a, P> {
    /// Create a new native asset adapter
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS native asset storage provider
    /// * `block_height` - Current block height for timestamp operations
    pub fn new(provider: &'a mut P, block_height: u64) -> Self {
        Self {
            provider,
            block_height,
        }
    }

    /// Convert [u8; 32] bytes to TOS Hash
    fn bytes_to_hash(bytes: &[u8; 32]) -> Hash {
        Hash::new(*bytes)
    }

    /// Convert TOS Hash to [u8; 32] bytes
    fn hash_to_bytes(hash: &Hash) -> [u8; 32] {
        *hash.as_bytes()
    }

    /// Convert error to EbpfError
    fn convert_error<E: std::fmt::Display>(err: E) -> EbpfError {
        EbpfError::SyscallError(Box::new(io::Error::new(
            io::ErrorKind::Other,
            format!("Native asset error: {}", err),
        )))
    }

    /// Create "not found" error
    fn not_found_error(msg: &str) -> EbpfError {
        EbpfError::SyscallError(Box::new(io::Error::new(io::ErrorKind::NotFound, msg)))
    }

    /// Create "invalid data" error
    fn invalid_data_error(msg: &str) -> EbpfError {
        EbpfError::SyscallError(Box::new(io::Error::new(io::ErrorKind::InvalidData, msg)))
    }

    /// Create "permission denied" error
    fn permission_denied_error(msg: &str) -> EbpfError {
        EbpfError::SyscallError(Box::new(io::Error::new(
            io::ErrorKind::PermissionDenied,
            msg,
        )))
    }

    /// Convert TAKO condition_type and condition_data to TOS ReleaseCondition
    ///
    /// condition_type:
    /// - 0 = TimeRelease (condition_data = u64 release_after as little-endian bytes)
    /// - 1 = MultiApproval (condition_data = [required: u8] + [approvers: [u8; 32]...])
    /// - 2 = HashLock (condition_data = [u8; 32] hash)
    fn parse_release_condition(
        condition_type: u8,
        condition_data: &[u8],
    ) -> Result<ReleaseCondition, EbpfError> {
        match condition_type {
            0 => {
                // TimeRelease: condition_data is u64 as little-endian
                if condition_data.len() < 8 {
                    return Err(Self::invalid_data_error(
                        "TimeRelease requires 8 bytes for release_after",
                    ));
                }
                let release_after =
                    u64::from_le_bytes(condition_data[..8].try_into().unwrap_or([0u8; 8]));
                Ok(ReleaseCondition::TimeRelease { release_after })
            }
            1 => {
                // MultiApproval: [required: u8] + [count: u8] + [approvers: [u8; 32]...]
                if condition_data.len() < 2 {
                    return Err(Self::invalid_data_error(
                        "MultiApproval requires at least 2 bytes (required, count)",
                    ));
                }
                let required = condition_data[0];
                let count = condition_data[1] as usize;

                // BUG-TOS-007 FIX: Validate count is not zero
                if count == 0 {
                    return Err(Self::invalid_data_error(
                        "MultiApproval: must have at least one approver",
                    ));
                }

                // BUG-TOS-007 FIX: Validate required <= count
                if (required as usize) > count {
                    return Err(Self::invalid_data_error(
                        "MultiApproval: required exceeds approver count",
                    ));
                }

                // BUG-TOS-007 FIX: Validate required is not zero
                if required == 0 {
                    return Err(Self::invalid_data_error(
                        "MultiApproval: required must be at least 1",
                    ));
                }

                let expected_len = 2 + count * 32;
                if condition_data.len() < expected_len {
                    return Err(Self::invalid_data_error(
                        "MultiApproval: not enough approvers data",
                    ));
                }
                let mut approvers = Vec::with_capacity(count);
                let zero_address = [0u8; 32];
                for i in 0..count {
                    let start = 2 + i * 32;
                    let mut addr = [0u8; 32];
                    addr.copy_from_slice(&condition_data[start..start + 32]);

                    // BUG-TOS-007 FIX: Validate approver is not zero address
                    if addr == zero_address {
                        return Err(Self::invalid_data_error(
                            "MultiApproval: approver cannot be zero address",
                        ));
                    }

                    approvers.push(addr);
                }
                Ok(ReleaseCondition::MultiApproval {
                    approvers,
                    required,
                })
            }
            2 => {
                // HashLock: [u8; 32] hash
                if condition_data.len() < 32 {
                    return Err(Self::invalid_data_error(
                        "HashLock requires 32 bytes for hash",
                    ));
                }
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&condition_data[..32]);
                Ok(ReleaseCondition::HashLock { hash })
            }
            _ => Err(Self::invalid_data_error("Unknown condition_type")),
        }
    }

    /// Convert TOS ReleaseCondition to TAKO condition_type and condition_data
    fn encode_release_condition(condition: &ReleaseCondition) -> (u8, Vec<u8>) {
        match condition {
            ReleaseCondition::TimeRelease { release_after } => {
                (0, release_after.to_le_bytes().to_vec())
            }
            ReleaseCondition::MultiApproval {
                approvers,
                required,
            } => {
                let mut data = Vec::with_capacity(2 + approvers.len() * 32);
                data.push(*required);
                data.push(approvers.len() as u8);
                for approver in approvers {
                    data.extend_from_slice(approver);
                }
                (1, data)
            }
            ReleaseCondition::HashLock { hash } => (2, hash.to_vec()),
        }
    }

    /// Convert EscrowStatus enum to u8
    fn escrow_status_to_u8(status: &EscrowStatus) -> u8 {
        match status {
            EscrowStatus::Active => 0,
            EscrowStatus::Released => 1,
            EscrowStatus::Cancelled => 2,
            EscrowStatus::Disputed => 3,
        }
    }

    /// Convert u8 to EscrowStatus enum
    fn u8_to_escrow_status(value: u8) -> EscrowStatus {
        match value {
            0 => EscrowStatus::Active,
            1 => EscrowStatus::Released,
            2 => EscrowStatus::Cancelled,
            3 => EscrowStatus::Disputed,
            _ => EscrowStatus::Active, // Default to Active for unknown values
        }
    }

    /// Convert SpendingPeriod enum to u8
    fn spending_period_to_u8(period: &SpendingPeriod) -> u8 {
        match period {
            SpendingPeriod::PerTransaction => 0,
            SpendingPeriod::PerBlock => 1,
            SpendingPeriod::Daily => 2,
            SpendingPeriod::Lifetime => 3,
        }
    }

    /// Convert u8 to SpendingPeriod enum
    fn u8_to_spending_period(value: u8) -> SpendingPeriod {
        match value {
            0 => SpendingPeriod::PerTransaction,
            1 => SpendingPeriod::PerBlock,
            2 => SpendingPeriod::Daily,
            3 => SpendingPeriod::Lifetime,
            _ => SpendingPeriod::Lifetime, // Default to Lifetime for unknown values
        }
    }

    /// Get asset or return error if not found
    fn get_asset_data(&self, asset: &[u8; 32]) -> Result<NativeAssetData, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Check if caller has role
    fn check_role(
        &self,
        asset: &Hash,
        role: &RoleId,
        caller: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        try_block_on(self.provider.has_native_asset_role(asset, role, caller))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Create a balance checkpoint for historical balance queries
    fn write_balance_checkpoint(
        &mut self,
        hash: &Hash,
        account: &[u8; 32],
        balance: u64,
    ) -> Result<(), EbpfError> {
        let checkpoint = BalanceCheckpoint {
            from_block: self.block_height,
            balance,
        };

        let count = try_block_on(
            self.provider
                .get_native_asset_balance_checkpoint_count(hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        try_block_on(self.provider.set_native_asset_balance_checkpoint(
            hash,
            account,
            count,
            &checkpoint,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // BUG-TOS-005 FIX: Use checked arithmetic for checkpoint count
        let new_count = count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Balance checkpoint count overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_balance_checkpoint_count(hash, account, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    /// Create a supply checkpoint for historical total supply queries
    fn write_supply_checkpoint(&mut self, hash: &Hash, supply: u64) -> Result<(), EbpfError> {
        let checkpoint = SupplyCheckpoint {
            from_block: self.block_height,
            supply,
        };

        let count = try_block_on(self.provider.get_native_asset_supply_checkpoint_count(hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        try_block_on(
            self.provider
                .set_native_asset_supply_checkpoint(hash, count, &checkpoint),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // BUG-TOS-005 FIX: Use checked arithmetic for checkpoint count
        let new_count = count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Supply checkpoint count overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_supply_checkpoint_count(hash, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    // ========================================
    // Vote Power & Checkpoint Operations (TOS-004 Fix)
    // ========================================

    /// Get stored vote power for an account (O(1) - no recalculation)
    fn get_vote_power(&self, hash: &Hash, account: &[u8; 32]) -> Result<u64, EbpfError> {
        try_block_on(self.provider.get_native_asset_vote_power(hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Set vote power for an account
    fn set_vote_power(
        &mut self,
        hash: &Hash,
        account: &[u8; 32],
        votes: u64,
    ) -> Result<(), EbpfError> {
        try_block_on(
            self.provider
                .set_native_asset_vote_power(hash, account, votes),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    /// Move vote power from one account to another (delta-based, O(1))
    ///
    /// Implements lazy initialization: if vote power is not yet tracked for an account
    /// but the account has a balance, initialize vote power from the balance first.
    /// This handles migration from pre-vote-tracking state gracefully.
    fn move_vote_power(
        &mut self,
        hash: &Hash,
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        // Skip if zero amount or same account
        if amount == 0 || from == to {
            return Ok(());
        }

        // Normalize zero address to skip (zero = no delegation = self)
        let zero_addr = [0u8; 32];
        if *from == zero_addr || *to == zero_addr {
            return Ok(()); // Zero address has no votes to move
        }

        // Get from votes with lazy initialization from balance if needed
        let mut from_votes = self.get_vote_power(hash, from)?;
        if from_votes == 0 {
            // Vote power not yet initialized - check if account has balance
            let balance = try_block_on(self.provider.get_native_asset_balance(hash, from))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
            if balance > 0 {
                // Lazy init: initialize vote power from balance (migration path)
                from_votes = balance;
                self.set_vote_power(hash, from, balance)?;
            }
        }

        // Subtract from source
        let new_from_votes = from_votes
            .checked_sub(amount)
            .ok_or_else(|| Self::invalid_data_error("Vote power underflow: inconsistent state"))?;
        self.set_vote_power(hash, from, new_from_votes)?;

        // Get to votes with lazy initialization from balance if needed
        let mut to_votes = self.get_vote_power(hash, to)?;
        if to_votes == 0 {
            let balance = try_block_on(self.provider.get_native_asset_balance(hash, to))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
            if balance > 0 {
                // Lazy init for destination as well
                to_votes = balance;
                self.set_vote_power(hash, to, balance)?;
            }
        }

        // Add to destination
        let new_to_votes = to_votes
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Vote power overflow"))?;
        self.set_vote_power(hash, to, new_to_votes)?;

        Ok(())
    }

    /// Get effective vote holder (self if no delegation or zero-address delegation)
    fn get_effective_vote_holder(
        &self,
        hash: &Hash,
        account: &[u8; 32],
    ) -> Result<[u8; 32], EbpfError> {
        let delegation = try_block_on(self.provider.get_native_asset_delegation(hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Return delegatee if set, otherwise self
        match delegation.delegatee {
            Some(delegatee) if delegatee != [0u8; 32] && delegatee != *account => Ok(delegatee),
            _ => Ok(*account),
        }
    }

    /// Write a vote checkpoint for an account
    /// If checkpoint already exists for current block, overwrite instead of append
    fn write_vote_checkpoint(&mut self, hash: &Hash, account: &[u8; 32]) -> Result<(), EbpfError> {
        // Skip zero address - zero address has no meaningful votes
        if *account == [0u8; 32] {
            return Ok(());
        }

        let votes = self.get_vote_power(hash, account)?;

        let count = try_block_on(
            self.provider
                .get_native_asset_checkpoint_count(hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        use tos_common::native_asset::Checkpoint;

        // Same-block checkpoint overwrite: check if last checkpoint is from current block
        if count > 0 {
            let last_idx = count.saturating_sub(1);
            let last_checkpoint = try_block_on(
                self.provider
                    .get_native_asset_checkpoint(hash, account, last_idx),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

            if last_checkpoint.from_block == self.block_height {
                // Overwrite existing checkpoint for this block
                let updated_checkpoint = Checkpoint {
                    from_block: self.block_height,
                    votes,
                };
                try_block_on(self.provider.set_native_asset_checkpoint(
                    hash,
                    account,
                    last_idx,
                    &updated_checkpoint,
                ))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
                return Ok(());
            }
        }

        // Append new checkpoint
        let checkpoint = Checkpoint {
            from_block: self.block_height,
            votes,
        };

        try_block_on(
            self.provider
                .set_native_asset_checkpoint(hash, account, count, &checkpoint),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let new_count = count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Vote checkpoint count overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_checkpoint_count(hash, account, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    /// Delta-based vote update for balance transfers (O(1))
    fn update_votes_for_balance_change(
        &mut self,
        hash: &Hash,
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        // Skip no-op transfers
        if amount == 0 || from == to {
            return Ok(());
        }

        // Get effective vote holders (respecting delegation)
        let from_delegatee = self.get_effective_vote_holder(hash, from)?;
        let to_delegatee = self.get_effective_vote_holder(hash, to)?;

        // If both delegate to same account, votes don't change
        if from_delegatee == to_delegatee {
            return Ok(());
        }

        // Move vote power from sender's delegatee to recipient's delegatee
        self.move_vote_power(hash, &from_delegatee, &to_delegatee, amount)?;

        // Write checkpoints for affected accounts
        self.write_vote_checkpoint(hash, &from_delegatee)?;
        if from_delegatee != to_delegatee {
            self.write_vote_checkpoint(hash, &to_delegatee)?;
        }

        Ok(())
    }

    /// Add vote power when minting (to recipient or their delegatee)
    fn add_vote_power_for_mint(
        &mut self,
        hash: &Hash,
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        if amount == 0 {
            return Ok(());
        }

        let vote_holder = self.get_effective_vote_holder(hash, to)?;
        let current_votes = self.get_vote_power(hash, &vote_holder)?;
        let new_votes = current_votes
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Vote power overflow"))?;
        self.set_vote_power(hash, &vote_holder, new_votes)?;
        self.write_vote_checkpoint(hash, &vote_holder)
    }

    /// Remove vote power when burning (from burner or their delegatee)
    fn remove_vote_power_for_burn(
        &mut self,
        hash: &Hash,
        from: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        if amount == 0 {
            return Ok(());
        }

        let vote_holder = self.get_effective_vote_holder(hash, from)?;
        let current_votes = self.get_vote_power(hash, &vote_holder)?;
        let new_votes = current_votes
            .checked_sub(amount)
            .ok_or_else(|| Self::invalid_data_error("Vote power underflow: inconsistent state"))?;
        self.set_vote_power(hash, &vote_holder, new_votes)?;
        self.write_vote_checkpoint(hash, &vote_holder)
    }
}

impl<'a, P: NativeAssetProvider + Send + Sync + ?Sized> TakoNativeAssetProvider
    for TosNativeAssetAdapter<'a, P>
{
    // ========================================
    // Asset Creation (Phase 1)
    // ========================================

    fn create_asset(
        &mut self,
        creator: &[u8; 32],
        name: &[u8],
        symbol: &[u8],
        decimals: u8,
        max_supply: Option<u64>,
        governance_enabled: bool,
        metadata_uri: &[u8],
        _block_height: u64,
    ) -> Result<[u8; 32], EbpfError> {
        // Generate asset ID from creator and name
        let mut hasher_input = Vec::with_capacity(32 + name.len() + 8);
        hasher_input.extend_from_slice(creator);
        hasher_input.extend_from_slice(name);
        hasher_input.extend_from_slice(&self.block_height.to_le_bytes());

        let asset_id = tos_common::crypto::hash(&hasher_input);

        // Check if asset already exists to prevent overwriting
        if try_block_on(self.provider.get_native_asset(&asset_id))
            .map_err(Self::convert_error)?
            .is_ok()
        {
            return Err(Self::invalid_data_error("Asset already exists"));
        }

        // Create asset data
        let name_str = String::from_utf8(name.to_vec())
            .map_err(|_| Self::invalid_data_error("Invalid name"))?;
        let symbol_str = String::from_utf8(symbol.to_vec())
            .map_err(|_| Self::invalid_data_error("Invalid symbol"))?;
        let metadata_str = if metadata_uri.is_empty() {
            None
        } else {
            Some(
                String::from_utf8(metadata_uri.to_vec())
                    .map_err(|_| Self::invalid_data_error("Invalid metadata URI"))?,
            )
        };

        let data = NativeAssetData {
            name: name_str,
            symbol: symbol_str,
            decimals,
            total_supply: 0,
            max_supply,
            mintable: true,
            burnable: true,
            pausable: true,
            freezable: true,
            governance: governance_enabled,
            creator: *creator,
            metadata_uri: metadata_str,
            created_at: self.block_height,
        };

        try_block_on(self.provider.set_native_asset(&asset_id, &data))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Grant DEFAULT_ADMIN_ROLE to creator
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        try_block_on(self.provider.grant_native_asset_role(
            &asset_id,
            &admin_role,
            creator,
            self.block_height,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(Self::hash_to_bytes(&asset_id))
    }

    // ========================================
    // Query Operations (Phase 2)
    // ========================================

    fn asset_exists(&self, asset: &[u8; 32]) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.has_native_asset(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn balance_of(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_balance(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn total_supply(&self, asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_supply(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn decimals(&self, asset: &[u8; 32]) -> Result<u8, EbpfError> {
        let data = self.get_asset_data(asset)?;
        Ok(data.decimals)
    }

    fn name(&self, asset: &[u8; 32]) -> Result<String, EbpfError> {
        let data = self.get_asset_data(asset)?;
        Ok(data.name)
    }

    fn symbol(&self, asset: &[u8; 32]) -> Result<String, EbpfError> {
        let data = self.get_asset_data(asset)?;
        Ok(data.symbol)
    }

    fn metadata_uri(&self, asset: &[u8; 32]) -> Result<Option<String>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_metadata_uri(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    // ========================================
    // Pause & Freeze State
    // ========================================

    fn is_paused(&self, asset: &[u8; 32]) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let state = try_block_on(self.provider.get_native_asset_pause_state(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(state.is_paused)
    }

    fn is_frozen(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let state = try_block_on(self.provider.get_native_asset_freeze_state(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(state.is_frozen)
    }

    // ========================================
    // Transfer Operations
    // ========================================

    fn transfer(
        &mut self,
        asset: &[u8; 32],
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        // Check pause state
        if self.is_paused(asset)? {
            return Err(Self::permission_denied_error("Asset is paused"));
        }

        // Check freeze states
        if self.is_frozen(asset, from)? {
            return Err(Self::permission_denied_error("Sender is frozen"));
        }
        if self.is_frozen(asset, to)? {
            return Err(Self::permission_denied_error("Recipient is frozen"));
        }

        // Block zero-address transfers to prevent vote power desync
        let zero_addr = [0u8; 32];
        if *to == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot transfer to zero address (use burn instead)",
            ));
        }
        if *from == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot transfer from zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check available balance (total - locked)
        let available = self.available_balance(asset, from)?;
        if available < amount {
            return Err(Self::permission_denied_error(
                "Insufficient available balance",
            ));
        }

        // Phase 1: Validation - calculate new values without state changes
        let from_balance = self.balance_of(asset, from)?;
        let new_from = from_balance
            .checked_sub(amount)
            .ok_or_else(|| Self::permission_denied_error("Insufficient balance"))?;

        let to_balance = self.balance_of(asset, to)?;
        let new_to = to_balance
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Balance overflow"))?;

        // Phase 2: Update vote power FIRST (if this fails, no balance state modified)
        self.update_votes_for_balance_change(&hash, from, to, amount)?;

        // Phase 3: Update balances and checkpoints
        try_block_on(
            self.provider
                .set_native_asset_balance(&hash, from, new_from),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, from, new_from)?;

        try_block_on(self.provider.set_native_asset_balance(&hash, to, new_to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, to, new_to)?;

        Ok(())
    }

    // ========================================
    // Mint & Burn Operations
    // ========================================

    fn mint(
        &mut self,
        asset: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Block zero-address mint to prevent vote power accumulation on unusable address
        let zero_addr = [0u8; 32];
        if *to == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot mint to zero address (use burn for token destruction)",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check if caller has MINTER role
        let minter_role = tos_common::native_asset::MINTER_ROLE;
        if !self.check_role(&hash, &minter_role, caller)? {
            return Err(Self::permission_denied_error("Caller is not a minter"));
        }

        // Phase 1: Validation - calculate new values without state changes
        let data = self.get_asset_data(asset)?;
        let current_supply = self.total_supply(asset)?;
        let new_supply = current_supply
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Supply overflow"))?;

        if let Some(max_supply) = data.max_supply {
            if new_supply > max_supply {
                return Err(Self::permission_denied_error("Would exceed max supply"));
            }
        }

        let balance = self.balance_of(asset, to)?;
        let new_balance = balance
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Balance overflow"))?;

        // Phase 2: Update vote power FIRST (if this fails, no balance state modified)
        self.add_vote_power_for_mint(&hash, to, amount)?;

        // Phase 3: Update balances and supply
        try_block_on(
            self.provider
                .set_native_asset_balance(&hash, to, new_balance),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, to, new_balance)?;

        try_block_on(self.provider.set_native_asset_supply(&hash, new_supply))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        self.write_supply_checkpoint(&hash, new_supply)?;

        Ok(())
    }

    fn burn(
        &mut self,
        asset: &[u8; 32],
        from: &[u8; 32],
        amount: u64,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency with other functions
        if *from == [0u8; 32] {
            return Err(Self::invalid_data_error("Cannot burn from zero address"));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check if caller has BURNER role or is the owner
        let burner_role = tos_common::native_asset::BURNER_ROLE;
        let is_burner = self.check_role(&hash, &burner_role, caller)?;
        let is_owner = from == caller;

        if !is_burner && !is_owner {
            return Err(Self::permission_denied_error(
                "Caller is not a burner and not the owner",
            ));
        }

        // Phase 1: Validation - calculate new values without state changes
        let balance = self.balance_of(asset, from)?;
        if balance < amount {
            return Err(Self::permission_denied_error("Insufficient balance"));
        }

        let new_balance = balance
            .checked_sub(amount)
            .ok_or_else(|| Self::permission_denied_error("Insufficient balance"))?;

        let supply = self.total_supply(asset)?;
        let new_supply = supply
            .checked_sub(amount)
            .ok_or_else(|| Self::invalid_data_error("Burn amount exceeds total supply"))?;

        // Phase 2: Update vote power FIRST (if this fails, no balance state modified)
        self.remove_vote_power_for_burn(&hash, from, amount)?;

        // Phase 3: Update balances and supply
        try_block_on(
            self.provider
                .set_native_asset_balance(&hash, from, new_balance),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, from, new_balance)?;

        try_block_on(self.provider.set_native_asset_supply(&hash, new_supply))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        self.write_supply_checkpoint(&hash, new_supply)?;

        Ok(())
    }

    fn add_balance(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to prevent vote power accumulation on unusable address
        let zero_addr = [0u8; 32];
        if *account == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot add balance to zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Phase 1: Validation
        let balance = self.balance_of(asset, account)?;
        let new_balance = balance
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Balance overflow"))?;

        // Phase 2: Update vote power FIRST
        self.add_vote_power_for_mint(&hash, account, amount)?;

        // Phase 3: Update balance
        try_block_on(
            self.provider
                .set_native_asset_balance(&hash, account, new_balance),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, account, new_balance)
    }

    fn subtract_balance(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency with add_balance
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot subtract balance from zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Phase 1: Validation
        let balance = self.balance_of(asset, account)?;
        let new_balance = balance
            .checked_sub(amount)
            .ok_or_else(|| Self::permission_denied_error("Insufficient balance"))?;

        // Phase 2: Update vote power FIRST
        self.remove_vote_power_for_burn(&hash, account, amount)?;

        // Phase 3: Update balance
        try_block_on(
            self.provider
                .set_native_asset_balance(&hash, account, new_balance),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, account, new_balance)
    }

    // ========================================
    // Approval Operations (Phase 3)
    // ========================================

    fn approve(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        spender: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency with transfer/mint/burn behavior
        let zero_addr = [0u8; 32];
        if *owner == zero_addr {
            return Err(Self::invalid_data_error("Cannot approve from zero address"));
        }
        if *spender == zero_addr {
            return Err(Self::invalid_data_error("Cannot approve to zero address"));
        }

        let hash = Self::bytes_to_hash(asset);

        if amount == 0 {
            // Revoke approval
            try_block_on(
                self.provider
                    .delete_native_asset_allowance(&hash, owner, spender),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        } else {
            let allowance = Allowance {
                amount,
                updated_at: self.block_height,
            };
            try_block_on(
                self.provider
                    .set_native_asset_allowance(&hash, owner, spender, &allowance),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        }

        Ok(())
    }

    fn allowance(
        &self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let allowance = try_block_on(
            self.provider
                .get_native_asset_allowance(&hash, owner, spender),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(allowance.amount)
    }

    fn transfer_from(
        &mut self,
        asset: &[u8; 32],
        spender: &[u8; 32],
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Check allowance
        let current_allowance = self.allowance(asset, from, spender)?;
        if current_allowance < amount && current_allowance != u64::MAX {
            return Err(Self::permission_denied_error("Insufficient allowance"));
        }

        // Perform transfer
        self.transfer(asset, from, to, amount)?;

        // Reduce allowance (unless unlimited)
        if current_allowance != u64::MAX {
            let new_allowance = current_allowance.saturating_sub(amount);
            if new_allowance == 0 {
                try_block_on(
                    self.provider
                        .delete_native_asset_allowance(&hash, from, spender),
                )
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
            } else {
                let allowance = Allowance {
                    amount: new_allowance,
                    updated_at: self.block_height,
                };
                try_block_on(
                    self.provider
                        .set_native_asset_allowance(&hash, from, spender, &allowance),
                )
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
            }
        }

        Ok(())
    }

    // ========================================
    // Governance Operations (Phase 4)
    // ========================================

    fn governance_enabled(&self, asset: &[u8; 32]) -> Result<bool, EbpfError> {
        let data = self.get_asset_data(asset)?;
        Ok(data.governance)
    }

    fn delegate(
        &mut self,
        asset: &[u8; 32],
        delegator: &[u8; 32],
        delegatee: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Block zero-address delegator to prevent creating checkpoints without vote movement
        let zero_addr = [0u8; 32];
        if *delegator == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot delegate from zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check if governance is enabled
        if !self.governance_enabled(asset)? {
            return Err(Self::permission_denied_error("Governance not enabled"));
        }

        // Phase 1: Read current state and validate
        let old_delegatee = self.get_effective_vote_holder(&hash, delegator)?;

        // Normalize new delegatee (zero = self)
        let new_delegatee = if *delegatee == [0u8; 32] {
            *delegator
        } else {
            *delegatee
        };

        // Skip if delegating to same effective delegatee
        if old_delegatee == new_delegatee {
            return Ok(()); // No vote movement needed
        }

        // Get delegator's balance (this is the vote power being moved)
        let delegator_balance = self.balance_of(asset, delegator)?;

        // Pre-calculate checkpoint count
        let count = try_block_on(
            self.provider
                .get_native_asset_delegation_checkpoint_count(&hash, delegator),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let new_count = count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Delegation checkpoint count overflow"))?;

        // Phase 2: Update vote power FIRST (if this fails, no state modified)
        self.move_vote_power(&hash, &old_delegatee, &new_delegatee, delegator_balance)?;

        // Write vote checkpoints for affected accounts
        self.write_vote_checkpoint(&hash, &old_delegatee)?;
        self.write_vote_checkpoint(&hash, &new_delegatee)?;

        // Phase 3: Update delegation state (after vote power succeeds)
        let delegation = Delegation {
            delegatee: Some(*delegatee),
            from_block: self.block_height,
        };
        try_block_on(
            self.provider
                .set_native_asset_delegation(&hash, delegator, &delegation),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Create delegation checkpoint for historical queries
        let checkpoint = DelegationCheckpoint {
            from_block: self.block_height,
            delegatee: *delegatee,
        };

        try_block_on(self.provider.set_native_asset_delegation_checkpoint(
            &hash,
            delegator,
            count,
            &checkpoint,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        try_block_on(
            self.provider
                .set_native_asset_delegation_checkpoint_count(&hash, delegator, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Phase 4: Update reverse delegation index
        // Remove from old delegatee's delegators list (if not self-delegation)
        if old_delegatee != *delegator {
            try_block_on(self.provider.remove_native_asset_delegator(
                &hash,
                &old_delegatee,
                delegator,
            ))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        }
        // Add to new delegatee's delegators list (if not self-delegation)
        if new_delegatee != *delegator {
            try_block_on(self.provider.add_native_asset_delegator(
                &hash,
                &new_delegatee,
                delegator,
            ))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        }

        Ok(())
    }

    fn get_delegate(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<[u8; 32], EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let delegation = try_block_on(self.provider.get_native_asset_delegation(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        // Return delegatee or zero address if no delegation
        Ok(delegation.delegatee.unwrap_or([0u8; 32]))
    }

    fn get_votes(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_checkpoint_count(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if count == 0 {
            return Ok(0);
        }

        let checkpoint = try_block_on(self.provider.get_native_asset_checkpoint(
            &hash,
            account,
            count - 1,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(checkpoint.votes)
    }

    fn get_past_votes(
        &self,
        asset: &[u8; 32],
        account: &[u8; 32],
        timepoint: u64,
    ) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_checkpoint_count(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if count == 0 {
            return Ok(0);
        }

        // Binary search for the checkpoint at or before timepoint
        let mut low = 0u32;
        let mut high = count;

        while low < high {
            let mid = low + (high - low) / 2; // Avoid overflow
            let checkpoint = try_block_on(
                self.provider
                    .get_native_asset_checkpoint(&hash, account, mid),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

            if checkpoint.from_block <= timepoint {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        if low == 0 {
            return Ok(0);
        }

        let checkpoint = try_block_on(self.provider.get_native_asset_checkpoint(
            &hash,
            account,
            low - 1,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(checkpoint.votes)
    }

    fn get_past_total_supply(&self, asset: &[u8; 32], timepoint: u64) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_supply_checkpoint_count(&hash),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if count == 0 {
            // No supply history, return current supply
            return self.total_supply(asset);
        }

        // Binary search for the checkpoint at or before timepoint
        let mut low = 0u32;
        let mut high = count;

        while low < high {
            let mid = low + (high - low) / 2; // Avoid overflow
            let checkpoint =
                try_block_on(self.provider.get_native_asset_supply_checkpoint(&hash, mid))
                    .map_err(Self::convert_error)?
                    .map_err(Self::convert_error)?;

            if checkpoint.from_block <= timepoint {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        if low == 0 {
            // No checkpoint at or before timepoint, return 0
            return Ok(0);
        }

        // Get the checkpoint at low - 1 (the last one at or before timepoint)
        let checkpoint = try_block_on(
            self.provider
                .get_native_asset_supply_checkpoint(&hash, low - 1),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(checkpoint.supply)
    }

    fn num_checkpoints(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_checkpoint_count(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(count as u64)
    }

    fn get_checkpoint(
        &self,
        asset: &[u8; 32],
        account: &[u8; 32],
        index: u64,
    ) -> Result<(u64, u64), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_checkpoint_count(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if index >= count as u64 {
            return Err(Self::not_found_error("Checkpoint index out of bounds"));
        }

        let checkpoint = try_block_on(self.provider.get_native_asset_checkpoint(
            &hash,
            account,
            index as u32,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok((checkpoint.from_block, checkpoint.votes))
    }

    fn clock_mode(&self, _asset: &[u8; 32]) -> Result<u8, EbpfError> {
        Ok(0) // Block number mode
    }

    // ========================================
    // Timelock Operations (Phase 5)
    // ========================================

    fn lock(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        amount: u64,
        unlock_at: u64,
        transferable: bool,
        _current_time: u64,
    ) -> Result<u64, EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error("Cannot lock for zero address"));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check available balance
        let available = self.available_balance(asset, account)?;
        if available < amount {
            return Err(Self::permission_denied_error(
                "Insufficient available balance",
            ));
        }

        // Get next lock ID
        let lock_id = try_block_on(self.provider.get_native_asset_next_lock_id(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Create lock
        let token_lock = TokenLock {
            id: lock_id,
            amount,
            unlock_at,
            transferable,
            locker: *account,
            created_at: self.block_height,
        };
        try_block_on(
            self.provider
                .set_native_asset_lock(&hash, account, &token_lock),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // BUG-TOS-005 FIX: Use checked arithmetic for lock ID
        let next_lock_id = lock_id
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock ID overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_next_lock_id(&hash, account, next_lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Update lock count
        let count = try_block_on(self.provider.get_native_asset_lock_count(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        // BUG-TOS-005 FIX: Use checked arithmetic for lock count
        let new_count = count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock count overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock_count(&hash, account, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Add to lock index
        try_block_on(
            self.provider
                .add_native_asset_lock_id(&hash, account, lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Update locked balance
        let locked = try_block_on(
            self.provider
                .get_native_asset_locked_balance(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        // BUG-TOS-005 FIX: Use checked arithmetic for locked balance
        let new_locked = locked
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Locked balance overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_locked_balance(&hash, account, new_locked),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(lock_id)
    }

    fn unlock(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<u64, EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error("Cannot unlock for zero address"));
        }

        let hash = Self::bytes_to_hash(asset);

        // Get lock
        let token_lock = try_block_on(self.provider.get_native_asset_lock(&hash, account, lock_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Check if unlockable
        if self.block_height < token_lock.unlock_at {
            return Err(Self::permission_denied_error("Lock not yet expired"));
        }

        let amount = token_lock.amount;

        // Delete lock
        try_block_on(
            self.provider
                .delete_native_asset_lock(&hash, account, lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Update lock count using checked arithmetic to catch invariant violations
        let count = try_block_on(self.provider.get_native_asset_lock_count(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        let new_count = count
            .checked_sub(1)
            .ok_or_else(|| Self::invalid_data_error("Lock count underflow in unlock"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock_count(&hash, account, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Remove from lock index
        try_block_on(
            self.provider
                .remove_native_asset_lock_id(&hash, account, lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Update locked balance using checked arithmetic
        let locked = try_block_on(
            self.provider
                .get_native_asset_locked_balance(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        let new_locked = locked
            .checked_sub(amount)
            .ok_or_else(|| Self::invalid_data_error("Locked balance underflow in unlock"))?;
        try_block_on(
            self.provider
                .set_native_asset_locked_balance(&hash, account, new_locked),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(amount)
    }

    fn get_lock(
        &self,
        asset: &[u8; 32],
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(u64, u64, bool, [u8; 32], u64), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let token_lock = try_block_on(self.provider.get_native_asset_lock(&hash, account, lock_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok((
            token_lock.amount,
            token_lock.unlock_at,
            token_lock.transferable,
            token_lock.locker,
            token_lock.created_at,
        ))
    }

    fn get_locks(
        &self,
        asset: &[u8; 32],
        account: &[u8; 32],
        offset: u64,
        limit: u64,
    ) -> Result<Vec<u64>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let all_locks = try_block_on(self.provider.get_native_asset_lock_ids(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Apply pagination
        let start = offset as usize;
        if start >= all_locks.len() {
            return Ok(vec![]);
        }
        let end = std::cmp::min(start + limit as usize, all_locks.len());
        Ok(all_locks[start..end].to_vec())
    }

    fn locked_balance(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(
            self.provider
                .get_native_asset_locked_balance(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn available_balance(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<u64, EbpfError> {
        let total = self.balance_of(asset, account)?;
        let locked = self.locked_balance(asset, account)?;
        // Use checked_sub to detect invariant violations where locked > total
        // This indicates data corruption and should not be masked
        total.checked_sub(locked).ok_or_else(|| {
            Self::invalid_data_error("Invariant violation: locked balance exceeds total balance")
        })
    }

    fn extend_lock(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        lock_id: u64,
        new_unlock_at: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot extend lock for zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        let mut token_lock =
            try_block_on(self.provider.get_native_asset_lock(&hash, account, lock_id))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;

        if new_unlock_at <= token_lock.unlock_at {
            return Err(Self::invalid_data_error("New unlock time must be greater"));
        }

        token_lock.unlock_at = new_unlock_at;
        try_block_on(
            self.provider
                .set_native_asset_lock(&hash, account, &token_lock),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn split_lock(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        lock_id: u64,
        split_amount: u64,
    ) -> Result<u64, EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot split lock for zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        let mut token_lock =
            try_block_on(self.provider.get_native_asset_lock(&hash, account, lock_id))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;

        if split_amount >= token_lock.amount {
            return Err(Self::invalid_data_error(
                "Split amount must be less than lock amount",
            ));
        }

        // Reduce original lock using checked arithmetic
        token_lock.amount = token_lock
            .amount
            .checked_sub(split_amount)
            .ok_or_else(|| Self::invalid_data_error("Lock amount underflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock(&hash, account, &token_lock),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Create new lock
        let new_lock_id = try_block_on(self.provider.get_native_asset_next_lock_id(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        let new_lock = TokenLock {
            id: new_lock_id,
            amount: split_amount,
            unlock_at: token_lock.unlock_at,
            transferable: token_lock.transferable,
            locker: token_lock.locker,
            created_at: self.block_height,
        };
        try_block_on(
            self.provider
                .set_native_asset_lock(&hash, account, &new_lock),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Use checked arithmetic for lock ID increment
        let next_lock_id = new_lock_id
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock ID overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_next_lock_id(&hash, account, next_lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let count = try_block_on(self.provider.get_native_asset_lock_count(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        // Use checked arithmetic for lock count
        let new_count = count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock count overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock_count(&hash, account, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(new_lock_id)
    }

    fn merge_locks(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        lock_ids: &[u64],
    ) -> Result<u64, EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot merge locks for zero address",
            ));
        }

        if lock_ids.len() < 2 {
            return Err(Self::invalid_data_error("Need at least 2 locks to merge"));
        }

        let hash = Self::bytes_to_hash(asset);

        // Get first lock for reference
        let first_lock = try_block_on(self.provider.get_native_asset_lock(
            &hash,
            account,
            lock_ids[0],
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let mut total_amount = first_lock.amount;

        // Verify all locks have same unlock_at and accumulate amounts using checked arithmetic
        for &lock_id in &lock_ids[1..] {
            let lock = try_block_on(self.provider.get_native_asset_lock(&hash, account, lock_id))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;

            if lock.unlock_at != first_lock.unlock_at {
                return Err(Self::invalid_data_error(
                    "All locks must have same unlock time",
                ));
            }
            total_amount = total_amount
                .checked_add(lock.amount)
                .ok_or_else(|| Self::invalid_data_error("Merged lock amount overflow"))?;
        }

        // Delete all old locks
        for &lock_id in lock_ids {
            try_block_on(
                self.provider
                    .delete_native_asset_lock(&hash, account, lock_id),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        }

        // Create merged lock
        let new_lock_id = try_block_on(self.provider.get_native_asset_next_lock_id(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        let new_lock = TokenLock {
            id: new_lock_id,
            amount: total_amount,
            unlock_at: first_lock.unlock_at,
            transferable: first_lock.transferable,
            locker: first_lock.locker,
            created_at: self.block_height,
        };
        try_block_on(
            self.provider
                .set_native_asset_lock(&hash, account, &new_lock),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Use checked arithmetic for lock ID increment
        let next_lock_id = new_lock_id
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock ID overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_next_lock_id(&hash, account, next_lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Update lock count (reduce by merged count - 1)
        // Use checked arithmetic to catch invariant violations
        let count = try_block_on(self.provider.get_native_asset_lock_count(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        let reduce_by = (lock_ids.len() as u32)
            .checked_sub(1)
            .ok_or_else(|| Self::invalid_data_error("Lock count reduction underflow"))?;
        let new_count = count
            .checked_sub(reduce_by)
            .ok_or_else(|| Self::invalid_data_error("Lock count underflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock_count(&hash, account, new_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(new_lock_id)
    }

    fn lock_count(&self, asset: &[u8; 32], account: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(self.provider.get_native_asset_lock_count(&hash, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(count as u64)
    }

    fn transfer_lock(
        &mut self,
        asset: &[u8; 32],
        from: &[u8; 32],
        to: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to prevent vote power issues and partial state updates
        let zero_addr = [0u8; 32];
        if *from == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot transfer lock from zero address",
            ));
        }
        if *to == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot transfer lock to zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        let token_lock = try_block_on(self.provider.get_native_asset_lock(&hash, from, lock_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        if !token_lock.transferable {
            return Err(Self::permission_denied_error("Lock is not transferable"));
        }

        let amount = token_lock.amount;

        // Check pause/freeze status before transferring
        // Note: We use subtract_balance/add_balance directly instead of transfer()
        // because transfer() checks available_balance which excludes locked tokens.
        // When transferring a lock, we're moving LOCKED tokens, not available tokens.
        let data = self.get_asset_data(asset)?;
        if data.pausable && self.is_paused(asset)? {
            return Err(Self::permission_denied_error("Asset is paused"));
        }
        if data.freezable && self.is_frozen(asset, from)? {
            return Err(Self::permission_denied_error("Sender is frozen"));
        }
        if data.freezable && self.is_frozen(asset, to)? {
            return Err(Self::permission_denied_error("Recipient is frozen"));
        }

        // Phase 1: Pre-validate all calculations to prevent partial state updates
        let from_balance = self.balance_of(asset, from)?;
        let _new_from_balance = from_balance
            .checked_sub(amount)
            .ok_or_else(|| Self::permission_denied_error("Insufficient balance"))?;

        let to_balance = self.balance_of(asset, to)?;
        let _new_to_balance = to_balance
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Recipient balance overflow"))?;

        // Also pre-validate lock count and locked balance for recipient
        let to_locked = try_block_on(self.provider.get_native_asset_locked_balance(&hash, to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        let _new_to_locked = to_locked
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Recipient locked balance overflow"))?;

        let to_count = try_block_on(self.provider.get_native_asset_lock_count(&hash, to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        let _new_to_count = to_count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Recipient lock count overflow"))?;

        // Phase 2: Transfer underlying balance (vote power updated inside)
        // All validations passed, now safe to mutate state
        self.subtract_balance(asset, from, amount)?;
        self.add_balance(asset, to, amount)?;

        // Delete from source
        try_block_on(self.provider.delete_native_asset_lock(&hash, from, lock_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Update source counts using checked arithmetic to catch invariant violations
        let from_count = try_block_on(self.provider.get_native_asset_lock_count(&hash, from))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        let new_from_count = from_count
            .checked_sub(1)
            .ok_or_else(|| Self::invalid_data_error("Source lock count underflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock_count(&hash, from, new_from_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let from_locked = try_block_on(self.provider.get_native_asset_locked_balance(&hash, from))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        let new_from_locked = from_locked
            .checked_sub(amount)
            .ok_or_else(|| Self::invalid_data_error("Source locked balance underflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_locked_balance(&hash, from, new_from_locked),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Create at destination with new ID
        let new_lock_id = try_block_on(self.provider.get_native_asset_next_lock_id(&hash, to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        let new_lock = TokenLock {
            id: new_lock_id,
            ..token_lock
        };
        try_block_on(self.provider.set_native_asset_lock(&hash, to, &new_lock))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Use checked arithmetic for lock ID increment
        let next_lock_id = new_lock_id
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock ID overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_next_lock_id(&hash, to, next_lock_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let to_count = try_block_on(self.provider.get_native_asset_lock_count(&hash, to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        // Use checked arithmetic for lock count
        let new_to_count = to_count
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Lock count overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_lock_count(&hash, to, new_to_count),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        let to_locked = try_block_on(self.provider.get_native_asset_locked_balance(&hash, to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        // Use checked arithmetic for locked balance
        let new_to_locked = to_locked
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Locked balance overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_locked_balance(&hash, to, new_to_locked),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(())
    }

    // ========================================
    // Role Operations (Phase 6)
    // ========================================

    fn has_role(
        &self,
        asset: &[u8; 32],
        role: &[u8; 32],
        account: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.has_native_asset_role(&hash, role, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn grant_role(
        &mut self,
        asset: &[u8; 32],
        role: &[u8; 32],
        account: &[u8; 32],
        granted_by: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // BUG-TOS-002 FIX: Validate caller has admin role for the role being granted
        // Get the admin role for this role
        let role_config = try_block_on(self.provider.get_native_asset_role_config(&hash, role))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Check if caller has the admin role
        let has_admin = self.check_role(&hash, &role_config.admin_role, granted_by)?;
        if !has_admin {
            return Err(Self::permission_denied_error(
                "Caller does not have admin role for this role",
            ));
        }

        // BUG-TOS-008 FIX: Validate account is not zero address
        if account == &[0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot grant role to zero address",
            ));
        }

        // Grant the role
        try_block_on(self.provider.grant_native_asset_role(
            &hash,
            role,
            account,
            self.block_height,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Add to role members index
        try_block_on(
            self.provider
                .add_native_asset_role_member(&hash, role, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn revoke_role(
        &mut self,
        asset: &[u8; 32],
        role: &[u8; 32],
        account: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot revoke role from zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // NOTE: RBAC validation (caller has admin role) must be done at syscall level in TAKO
        // The trait doesn't include a caller parameter for this function

        // Revoke the role
        try_block_on(self.provider.revoke_native_asset_role(&hash, role, account))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Remove from role members index
        try_block_on(
            self.provider
                .remove_native_asset_role_member(&hash, role, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn get_role_admin(&self, asset: &[u8; 32], role: &[u8; 32]) -> Result<[u8; 32], EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let config = try_block_on(self.provider.get_native_asset_role_config(&hash, role))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(config.admin_role)
    }

    fn set_role_admin(
        &mut self,
        asset: &[u8; 32],
        role: &[u8; 32],
        admin_role: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // NOTE: RBAC validation must be done at syscall level in TAKO
        // The trait doesn't include a caller parameter for this function

        let mut config = try_block_on(self.provider.get_native_asset_role_config(&hash, role))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        config.admin_role = *admin_role;

        try_block_on(
            self.provider
                .set_native_asset_role_config(&hash, role, &config),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn get_role_member_count(&self, asset: &[u8; 32], role: &[u8; 32]) -> Result<u32, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let config = try_block_on(self.provider.get_native_asset_role_config(&hash, role))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(config.member_count)
    }

    // ========================================
    // Admin Operations (Phase 7)
    // ========================================

    fn set_paused(
        &mut self,
        asset: &[u8; 32],
        paused: bool,
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Check if caller has PAUSER role
        let pauser_role = tos_common::native_asset::PAUSER_ROLE;
        if !self.check_role(&hash, &pauser_role, caller)? {
            return Err(Self::permission_denied_error("Caller is not a pauser"));
        }

        let state = PauseState {
            is_paused: paused,
            paused_by: if paused { Some(*caller) } else { None },
            paused_at: if paused {
                Some(self.block_height)
            } else {
                None
            },
        };
        try_block_on(self.provider.set_native_asset_pause_state(&hash, &state))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn set_frozen(
        &mut self,
        asset: &[u8; 32],
        account: &[u8; 32],
        frozen: bool,
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency
        if *account == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot set frozen state for zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check if caller has FREEZER role
        let freezer_role = tos_common::native_asset::FREEZER_ROLE;
        if !self.check_role(&hash, &freezer_role, caller)? {
            return Err(Self::permission_denied_error("Caller is not a freezer"));
        }

        let state = FreezeState {
            is_frozen: frozen,
            frozen_by: if frozen { Some(*caller) } else { None },
            frozen_at: if frozen {
                Some(self.block_height)
            } else {
                None
            },
        };
        try_block_on(
            self.provider
                .set_native_asset_freeze_state(&hash, account, &state),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn force_transfer(
        &mut self,
        asset: &[u8; 32],
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // NOTE: RBAC validation must be done at syscall level in TAKO
        // The trait doesn't include a caller parameter for this function

        // BUG-TOS-008 FIX: Validate amount is not zero
        if amount == 0 {
            return Err(Self::invalid_data_error("Amount must be greater than zero"));
        }

        // Block zero-address transfers to prevent vote power desync
        let zero_addr = [0u8; 32];
        if *to == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot transfer to zero address (use burn instead)",
            ));
        }
        if *from == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot transfer from zero address",
            ));
        }

        // Phase 1: Validation - calculate new values without state changes
        // Force transfer bypasses pause/freeze checks but respects locked balance invariant
        let from_balance = self.balance_of(asset, from)?;
        let new_from = from_balance
            .checked_sub(amount)
            .ok_or_else(|| Self::permission_denied_error("Insufficient balance"))?;

        // Check that new balance doesn't go below locked balance
        // This prevents the invariant violation: balance < locked
        let from_locked = try_block_on(self.provider.get_native_asset_locked_balance(&hash, from))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        if new_from < from_locked {
            return Err(Self::permission_denied_error(
                "Force transfer would leave balance below locked amount",
            ));
        }

        let to_balance = self.balance_of(asset, to)?;
        let new_to = to_balance
            .checked_add(amount)
            .ok_or_else(|| Self::invalid_data_error("Balance overflow"))?;

        // Phase 2: Update vote power FIRST (if this fails, no balance state modified)
        self.update_votes_for_balance_change(&hash, from, to, amount)?;

        // Phase 3: Update balances and checkpoints
        try_block_on(
            self.provider
                .set_native_asset_balance(&hash, from, new_from),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, from, new_from)?;

        try_block_on(self.provider.set_native_asset_balance(&hash, to, new_to))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        self.write_balance_checkpoint(&hash, to, new_to)
    }

    fn update_name(&mut self, asset: &[u8; 32], name: &[u8]) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // NOTE: RBAC validation must be done at syscall level in TAKO
        // The trait doesn't include a caller parameter for this function

        let mut data = self.get_asset_data(asset)?;
        data.name = String::from_utf8(name.to_vec())
            .map_err(|_| Self::invalid_data_error("Invalid name"))?;
        try_block_on(self.provider.set_native_asset(&hash, &data))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn update_symbol(&mut self, asset: &[u8; 32], symbol: &[u8]) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // NOTE: RBAC validation must be done at syscall level in TAKO
        // The trait doesn't include a caller parameter for this function

        let mut data = self.get_asset_data(asset)?;
        data.symbol = String::from_utf8(symbol.to_vec())
            .map_err(|_| Self::invalid_data_error("Invalid symbol"))?;
        try_block_on(self.provider.set_native_asset(&hash, &data))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn update_metadata_uri(&mut self, asset: &[u8; 32], uri: &[u8]) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // NOTE: RBAC validation must be done at syscall level in TAKO
        // The trait doesn't include a caller parameter for this function

        let uri_str = if uri.is_empty() {
            None
        } else {
            Some(
                String::from_utf8(uri.to_vec())
                    .map_err(|_| Self::invalid_data_error("Invalid metadata URI"))?,
            )
        };
        try_block_on(
            self.provider
                .set_native_asset_metadata_uri(&hash, uri_str.as_deref()),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    // ========================================
    // Escrow Operations (Phase 8)
    // ========================================

    fn next_escrow_id(&mut self, asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let counter = try_block_on(self.provider.get_native_asset_escrow_counter(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        // BUG-TOS-005 FIX: Use checked arithmetic for escrow counter
        let next_counter = counter
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Escrow counter overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_escrow_counter(&hash, next_counter),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(counter)
    }

    #[allow(clippy::too_many_arguments)]
    fn create_escrow(
        &mut self,
        asset: &[u8; 32],
        escrow_id: u64,
        sender: &[u8; 32],
        recipient: &[u8; 32],
        amount: u64,
        condition_type: u8,
        condition_data: &[u8],
        expires_at: u64,
        created_at: u64,
    ) -> Result<(), EbpfError> {
        // Block zero-address to prevent funds getting stuck when escrow releases
        // (add_balance rejects zero address, so released funds would be undeliverable)
        let zero_addr = [0u8; 32];
        if *sender == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot create escrow from zero address",
            ));
        }
        if *recipient == zero_addr {
            return Err(Self::invalid_data_error(
                "Cannot create escrow to zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);

        // Check if escrow already exists to prevent overwriting
        // If get_native_asset_escrow succeeds, the escrow already exists
        if try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .is_ok()
        {
            return Err(Self::invalid_data_error("Escrow already exists"));
        }

        // Validate release condition FIRST before any state changes
        // This ensures parsing errors don't leave balance deducted without escrow
        let condition = Self::parse_release_condition(condition_type, condition_data)?;

        // Check available balance (excludes locked tokens) before escrowing
        // This prevents locked tokens from being double-used in escrows
        let available = self.available_balance(asset, sender)?;
        if available < amount {
            return Err(Self::permission_denied_error(
                "Insufficient available balance for escrow",
            ));
        }

        // Transfer tokens from sender to escrow (subtract from sender)
        self.subtract_balance(asset, sender, amount)?;

        let escrow = Escrow {
            id: escrow_id,
            asset: hash.clone(),
            sender: *sender,
            recipient: *recipient,
            amount,
            condition,
            status: EscrowStatus::Active,
            approvals: Vec::new(),
            expires_at: if expires_at == 0 {
                None
            } else {
                Some(expires_at)
            },
            created_at,
            metadata: None,
        };
        try_block_on(self.provider.set_native_asset_escrow(&hash, &escrow))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Add to user escrow index for sender
        try_block_on(
            self.provider
                .add_native_asset_user_escrow(&hash, sender, escrow_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Add to user escrow index for recipient
        try_block_on(
            self.provider
                .add_native_asset_user_escrow(&hash, recipient, escrow_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    #[allow(clippy::type_complexity)]
    fn get_escrow(
        &self,
        asset: &[u8; 32],
        escrow_id: u64,
    ) -> Result<Option<([u8; 32], [u8; 32], u64, u8, u8, Vec<u8>, u32, u64, u64)>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let escrow = try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Convert TOS ReleaseCondition to TAKO condition_type + condition_data
        let (condition_type, condition_data) = Self::encode_release_condition(&escrow.condition);

        Ok(Some((
            escrow.sender,
            escrow.recipient,
            escrow.amount,
            Self::escrow_status_to_u8(&escrow.status),
            condition_type,
            condition_data,
            escrow.approvals.len() as u32,
            escrow.expires_at.unwrap_or(0),
            escrow.created_at,
        )))
    }

    fn update_escrow_status(
        &mut self,
        asset: &[u8; 32],
        escrow_id: u64,
        status: u8,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let mut escrow = try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        let old_status = escrow.status.clone();
        let new_status = Self::u8_to_escrow_status(status);

        // Escrow status state machine validation:
        // - Active can transition to Released or Cancelled
        // - Released and Cancelled are terminal states (cannot transition)
        match (&old_status, &new_status) {
            (EscrowStatus::Active, EscrowStatus::Released)
            | (EscrowStatus::Active, EscrowStatus::Cancelled) => {
                // Valid transition - continue
            }
            (EscrowStatus::Released, _) => {
                return Err(Self::invalid_data_error(
                    "Cannot change status of released escrow",
                ));
            }
            (EscrowStatus::Cancelled, _) => {
                return Err(Self::invalid_data_error(
                    "Cannot change status of cancelled escrow",
                ));
            }
            _ => {
                return Err(Self::invalid_data_error("Invalid escrow status transition"));
            }
        }

        // Update escrow status FIRST to prevent double-credit risk
        // Trade-off: If status update succeeds but credit fails, funds are stuck
        // in a Released/Cancelled escrow. This is recoverable through admin
        // intervention. The alternative (credit first) risks double-credit if
        // status update fails, which is worse as it creates unauthorized funds.
        let recipient = escrow.recipient;
        let sender = escrow.sender;
        let amount = escrow.amount;

        escrow.status = new_status.clone();
        try_block_on(self.provider.set_native_asset_escrow(&hash, &escrow))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Now credit the appropriate party
        // If this fails, escrow is already Released/Cancelled (prevents double-release)
        // but funds are stuck - recoverable through admin force_transfer
        if new_status == EscrowStatus::Released {
            self.add_balance(asset, &recipient, amount)?;
        } else if new_status == EscrowStatus::Cancelled {
            self.add_balance(asset, &sender, amount)?;
        }

        Ok(())
    }

    fn add_escrow_approval(
        &mut self,
        asset: &[u8; 32],
        escrow_id: u64,
        approver: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Block zero-address to maintain consistency
        if *approver == [0u8; 32] {
            return Err(Self::invalid_data_error(
                "Cannot add escrow approval for zero address",
            ));
        }

        let hash = Self::bytes_to_hash(asset);
        let mut escrow = try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        if !escrow.approvals.contains(approver) {
            escrow.approvals.push(*approver);
            try_block_on(self.provider.set_native_asset_escrow(&hash, &escrow))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
        }

        Ok(())
    }

    fn remove_escrow_approval(
        &mut self,
        asset: &[u8; 32],
        escrow_id: u64,
        approver: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let mut escrow = try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        if let Some(pos) = escrow.approvals.iter().position(|a| a == approver) {
            escrow.approvals.remove(pos);
            try_block_on(self.provider.set_native_asset_escrow(&hash, &escrow))
                .map_err(Self::convert_error)?
                .map_err(Self::convert_error)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn has_escrow_approval(
        &self,
        asset: &[u8; 32],
        escrow_id: u64,
        approver: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let escrow = try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(escrow.approvals.contains(approver))
    }

    fn get_escrow_approvals(
        &self,
        asset: &[u8; 32],
        escrow_id: u64,
    ) -> Result<Vec<[u8; 32]>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let escrow = try_block_on(self.provider.get_native_asset_escrow(&hash, escrow_id))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(escrow.approvals)
    }

    fn get_user_escrows(&self, asset: &[u8; 32], user: &[u8; 32]) -> Result<Vec<u64>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_user_escrows(&hash, user))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn add_user_escrow(
        &mut self,
        asset: &[u8; 32],
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(
            self.provider
                .add_native_asset_user_escrow(&hash, user, escrow_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    // ========================================
    // Permit Operations (Phase 9)
    // ========================================

    fn get_permit_nonce(&self, asset: &[u8; 32], owner: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_permit_nonce(&hash, owner))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn increment_permit_nonce(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
    ) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let nonce = self.get_permit_nonce(asset, owner)?;
        // Use checked arithmetic to prevent nonce overflow and replay attacks
        let new_nonce = nonce
            .checked_add(1)
            .ok_or_else(|| Self::invalid_data_error("Permit nonce overflow"))?;
        try_block_on(
            self.provider
                .set_native_asset_permit_nonce(&hash, owner, new_nonce),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(new_nonce)
    }

    fn get_chain_id(&self) -> Result<u64, EbpfError> {
        Ok(TOS_CHAIN_ID)
    }

    fn verify_ed25519_signature(
        &self,
        public_key: &[u8; 32],
        message: &[u8],
        signature: &[u8; 64],
    ) -> Result<bool, EbpfError> {
        // TOS uses Ristretto-based Schnorr signatures, not ed25519
        // Convert compressed public key to PublicKey
        let compressed = tos_common::crypto::elgamal::CompressedPublicKey::from_bytes(public_key)
            .map_err(|_| Self::invalid_data_error("Invalid public key"))?;
        let pubkey = compressed
            .decompress()
            .map_err(|_| Self::invalid_data_error("Failed to decompress public key"))?;

        let sig = tos_common::crypto::Signature::from_bytes(signature)
            .map_err(|_| Self::invalid_data_error("Invalid signature"))?;

        // Use Signature::verify method
        Ok(sig.verify(message, &pubkey))
    }

    // ========================================
    // Agent Operations (Phase 10)
    // ========================================

    #[allow(clippy::type_complexity)]
    fn get_agent_auth(
        &self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<Option<(u64, u8, u64, u64, u64, bool, u64, bool, u32)>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let has_auth = try_block_on(
            self.provider
                .has_native_asset_agent_auth(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if !has_auth {
            return Ok(None);
        }

        let auth = try_block_on(
            self.provider
                .get_native_asset_agent_auth(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Compute is_active from expires_at (0 = never expires, otherwise check block height)
        let is_active = auth.expires_at == 0 || auth.expires_at > self.block_height;

        Ok(Some((
            auth.spending_limit.max_amount,
            Self::spending_period_to_u8(&auth.spending_limit.period),
            0, // epoch_blocks not used in TOS
            auth.spending_limit.current_spent,
            auth.spending_limit.period_start,
            auth.can_delegate,
            auth.expires_at,
            is_active,
            auth.allowed_recipients.len() as u32,
        )))
    }

    #[allow(clippy::too_many_arguments)]
    fn set_agent_auth(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
        max_amount: u64,
        period_type: u8,
        _epoch_blocks: u64,
        current_spent: u64,
        period_start: u64,
        can_delegate: bool,
        expires_at: u64,
        _is_active: bool,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let spending_limit = SpendingLimit {
            max_amount,
            period: Self::u8_to_spending_period(period_type),
            current_spent,
            period_start,
        };

        let auth = AgentAuthorization {
            agent: *agent,
            owner: *owner,
            asset: hash.clone(),
            spending_limit,
            can_delegate,
            allowed_recipients: Vec::new(),
            expires_at,
            created_at: self.block_height,
        };
        try_block_on(self.provider.set_native_asset_agent_auth(&hash, &auth))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn remove_agent_auth(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(
            self.provider
                .delete_native_asset_agent_auth(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn get_owner_agents(
        &self,
        asset: &[u8; 32],
        owner: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_owner_agents(&hash, owner))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn add_owner_agent(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(
            self.provider
                .add_native_asset_owner_agent(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn remove_owner_agent(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(
            self.provider
                .remove_native_asset_owner_agent(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn get_agent_allowed_recipients(
        &self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let auth = try_block_on(
            self.provider
                .get_native_asset_agent_auth(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        Ok(auth.allowed_recipients)
    }

    fn set_agent_allowed_recipients(
        &mut self,
        asset: &[u8; 32],
        owner: &[u8; 32],
        agent: &[u8; 32],
        recipients: &[[u8; 32]],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let mut auth = try_block_on(
            self.provider
                .get_native_asset_agent_auth(&hash, owner, agent),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;
        auth.allowed_recipients = recipients.to_vec();
        try_block_on(self.provider.set_native_asset_agent_auth(&hash, &auth))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    // ========================================
    // Phase 2.1: Basic Query Operations
    // ========================================

    fn get_admin(&self, asset: &[u8; 32]) -> Result<[u8; 32], EbpfError> {
        let data = self.get_asset_data(asset)?;
        Ok(data.creator) // Creator is initial admin
    }

    // ========================================
    // Phase 2.3: Role Enumeration
    // ========================================

    fn get_role_member(
        &self,
        asset: &[u8; 32],
        role: &[u8; 32],
        index: u64,
    ) -> Result<[u8; 32], EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(
            self.provider
                .get_native_asset_role_member(&hash, role, index as u32),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn get_role_members(
        &self,
        asset: &[u8; 32],
        role: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        try_block_on(self.provider.get_native_asset_role_members(&hash, role))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    // ========================================
    // Phase 2.4: Governance Enhancements
    // ========================================

    fn undelegate(
        &mut self,
        asset: &[u8; 32],
        delegator: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        self.delegate(asset, delegator, &[0u8; 32])
    }

    #[allow(clippy::too_many_arguments)]
    fn delegate_by_sig(
        &mut self,
        asset: &[u8; 32],
        delegatee: &[u8; 32],
        delegator: &[u8; 32],
        nonce: u64,
        expiry: u64,
        signature: &[u8; 64],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        // Verify nonce
        let current_nonce = self.get_permit_nonce(asset, delegator)?;
        if nonce != current_nonce {
            return Err(Self::invalid_data_error("Invalid nonce"));
        }

        // Verify expiry
        if expiry < self.block_height {
            return Err(Self::permission_denied_error("Signature expired"));
        }

        // Verify signature
        let message = [
            asset.as_slice(),
            delegatee,
            delegator,
            &nonce.to_le_bytes(),
            &expiry.to_le_bytes(),
        ]
        .concat();
        if !self.verify_ed25519_signature(delegator, &message, signature)? {
            return Err(Self::permission_denied_error("Invalid signature"));
        }

        // Increment nonce
        self.increment_permit_nonce(asset, delegator)?;

        // Perform delegation
        self.delegate(asset, delegator, delegatee)
    }

    fn get_past_delegate(
        &self,
        asset: &[u8; 32],
        account: &[u8; 32],
        block_height: u64,
    ) -> Result<[u8; 32], EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_delegation_checkpoint_count(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if count == 0 {
            // No delegation history, return self-delegation
            return Ok(*account);
        }

        // Binary search for the checkpoint at or before block_height
        let mut low = 0u32;
        let mut high = count;

        while low < high {
            let mid = low + (high - low) / 2; // Avoid overflow
            let checkpoint = try_block_on(
                self.provider
                    .get_native_asset_delegation_checkpoint(&hash, account, mid),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

            if checkpoint.from_block <= block_height {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        if low == 0 {
            // No checkpoint at or before block_height, return self-delegation
            return Ok(*account);
        }

        // Get the checkpoint at low - 1 (the last one at or before block_height)
        let checkpoint = try_block_on(self.provider.get_native_asset_delegation_checkpoint(
            &hash,
            account,
            low - 1,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Return delegatee (zero means self-delegation)
        if checkpoint.delegatee == [0u8; 32] {
            Ok(*account)
        } else {
            Ok(checkpoint.delegatee)
        }
    }

    fn get_past_balance(
        &self,
        asset: &[u8; 32],
        account: &[u8; 32],
        block_height: u64,
    ) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let count = try_block_on(
            self.provider
                .get_native_asset_balance_checkpoint_count(&hash, account),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if count == 0 {
            // No balance history, return current balance
            return self.balance_of(asset, account);
        }

        // Binary search for the checkpoint at or before block_height
        let mut low = 0u32;
        let mut high = count;

        while low < high {
            let mid = low + (high - low) / 2; // Avoid overflow
            let checkpoint = try_block_on(
                self.provider
                    .get_native_asset_balance_checkpoint(&hash, account, mid),
            )
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

            if checkpoint.from_block <= block_height {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        if low == 0 {
            // No checkpoint at or before block_height, return 0 (no balance yet)
            return Ok(0);
        }

        // Get the checkpoint at low - 1 (the last one at or before block_height)
        let checkpoint = try_block_on(self.provider.get_native_asset_balance_checkpoint(
            &hash,
            account,
            low - 1,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(checkpoint.balance)
    }

    fn get_domain(&self, asset: &[u8; 32]) -> Result<Vec<u8>, EbpfError> {
        let data = self.get_asset_data(asset)?;
        // Return simple domain encoding
        let mut domain = Vec::new();
        domain.extend_from_slice(data.name.as_bytes());
        domain.push(0);
        domain.extend_from_slice(b"1"); // version
        domain.push(0);
        domain.extend_from_slice(&TOS_CHAIN_ID.to_le_bytes());
        domain.extend_from_slice(asset);
        Ok(domain)
    }

    // ========================================
    // Phase 2.6: Ownership Transfer
    // ========================================

    fn propose_admin(
        &mut self,
        asset: &[u8; 32],
        new_admin: &[u8; 32],
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Check if caller is current admin
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error("Caller is not admin"));
        }

        try_block_on(
            self.provider
                .set_native_asset_pending_admin(&hash, Some(new_admin)),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn accept_admin(
        &mut self,
        asset: &[u8; 32],
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Get pending admin
        let pending = try_block_on(self.provider.get_native_asset_pending_admin(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        let pending_admin = pending.ok_or_else(|| Self::not_found_error("No pending admin"))?;

        // Check if caller is the pending admin
        if caller != &pending_admin {
            return Err(Self::permission_denied_error("Caller is not pending admin"));
        }

        // Grant admin role to new admin
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        try_block_on(self.provider.grant_native_asset_role(
            &hash,
            &admin_role,
            caller,
            self.block_height,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Clear pending admin
        try_block_on(self.provider.set_native_asset_pending_admin(&hash, None))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn cancel_admin_proposal(
        &mut self,
        asset: &[u8; 32],
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Check if caller is current admin
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error("Caller is not admin"));
        }

        try_block_on(self.provider.set_native_asset_pending_admin(&hash, None))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn get_pending_admin(&self, asset: &[u8; 32]) -> Result<[u8; 32], EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let pending = try_block_on(self.provider.get_native_asset_pending_admin(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;
        Ok(pending.unwrap_or([0u8; 32]))
    }

    fn renounce_admin(
        &mut self,
        asset: &[u8; 32],
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        self.revoke_role(asset, &admin_role, caller)
    }

    fn get_admin_delay(&self, asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let admin_delay = try_block_on(self.provider.get_native_asset_admin_delay(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // If pending delay is set and effective, return it
        if let (Some(pending), Some(effective_at)) = (
            admin_delay.pending_delay,
            admin_delay.pending_delay_effective_at,
        ) {
            if self.block_height >= effective_at {
                return Ok(pending);
            }
        }

        Ok(admin_delay.delay)
    }

    fn change_admin_delay(
        &mut self,
        asset: &[u8; 32],
        new_delay: u64,
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // BUG-TOS-002 FIX: Validate caller has DEFAULT_ADMIN_ROLE
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error(
                "Caller does not have DEFAULT_ADMIN_ROLE for change_admin_delay",
            ));
        }

        // Get current admin delay config
        let mut admin_delay = try_block_on(self.provider.get_native_asset_admin_delay(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Check if pending delay should be applied first
        if let (Some(pending), Some(effective_at)) = (
            admin_delay.pending_delay,
            admin_delay.pending_delay_effective_at,
        ) {
            if self.block_height >= effective_at {
                admin_delay.delay = pending;
            }
        }

        // Set new pending delay
        admin_delay.pending_delay = Some(new_delay);
        // Delay becomes effective after current delay period
        admin_delay.pending_delay_effective_at = Some(
            self.block_height
                .checked_add(admin_delay.delay)
                .ok_or_else(|| Self::invalid_data_error("Block height overflow"))?,
        );

        try_block_on(
            self.provider
                .set_native_asset_admin_delay(&hash, &admin_delay),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn get_pending_admin_delay(&self, asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);
        let admin_delay = try_block_on(self.provider.get_native_asset_admin_delay(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        Ok(admin_delay.pending_delay.unwrap_or(0))
    }

    fn cancel_admin_delay_change(
        &mut self,
        asset: &[u8; 32],
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // BUG-TOS-002 FIX: Validate caller has DEFAULT_ADMIN_ROLE
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error(
                "Caller does not have DEFAULT_ADMIN_ROLE for cancel_admin_delay_change",
            ));
        }

        // Get current admin delay config
        let mut admin_delay = try_block_on(self.provider.get_native_asset_admin_delay(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        // Clear pending delay
        admin_delay.pending_delay = None;
        admin_delay.pending_delay_effective_at = None;

        try_block_on(
            self.provider
                .set_native_asset_admin_delay(&hash, &admin_delay),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    // ========================================
    // Phase 2.7: Admin Timelock
    // ========================================

    #[allow(clippy::too_many_arguments)]
    fn timelock_schedule(
        &mut self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
        target: &[u8; 32],
        data: &[u8],
        delay: u64,
        caller: &[u8; 32],
        block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Validate caller has DEFAULT_ADMIN_ROLE to schedule timelock operations
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error(
                "Caller does not have DEFAULT_ADMIN_ROLE for timelock_schedule",
            ));
        }

        // Check if operation already exists
        let existing = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        if existing.is_some() {
            return Err(Self::invalid_data_error("Operation already exists"));
        }

        // Enforce minimum delay
        let min_delay = try_block_on(self.provider.get_native_asset_timelock_min_delay(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        if delay < min_delay {
            return Err(Self::invalid_data_error("Delay is less than minimum"));
        }

        // Calculate ready_at timestamp
        let ready_at = block_height
            .checked_add(delay)
            .ok_or_else(|| Self::invalid_data_error("Block height overflow"))?;

        // Create and store the operation
        let operation = TimelockOperation {
            id: *operation_id,
            target: *target,
            data: data.to_vec(),
            ready_at,
            status: TimelockStatus::Pending,
            scheduler: *caller,
            scheduled_at: block_height,
        };

        try_block_on(
            self.provider
                .set_native_asset_timelock_operation(&hash, &operation),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn timelock_execute(
        &mut self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
        caller: &[u8; 32],
        block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Validate caller has DEFAULT_ADMIN_ROLE to execute timelock operations
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error(
                "Caller does not have DEFAULT_ADMIN_ROLE for timelock_execute",
            ));
        }

        // Get the operation
        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?
        .ok_or_else(|| Self::not_found_error("Operation not found"))?;

        // Check if operation is ready
        if operation.status != TimelockStatus::Pending {
            return Err(Self::invalid_data_error("Operation is not pending"));
        }

        if block_height < operation.ready_at {
            return Err(Self::invalid_data_error("Operation is not ready yet"));
        }

        // Mark as done
        let mut updated_operation = operation;
        updated_operation.status = TimelockStatus::Done;

        try_block_on(
            self.provider
                .set_native_asset_timelock_operation(&hash, &updated_operation),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn timelock_cancel(
        &mut self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Validate caller has DEFAULT_ADMIN_ROLE to cancel timelock operations
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error(
                "Caller does not have DEFAULT_ADMIN_ROLE for timelock_cancel",
            ));
        }

        // Check if operation exists and is pending
        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?
        .ok_or_else(|| Self::not_found_error("Operation not found"))?;

        if operation.status != TimelockStatus::Pending {
            return Err(Self::invalid_data_error(
                "Can only cancel pending operations",
            ));
        }

        // Delete the operation
        try_block_on(
            self.provider
                .delete_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn timelock_get_operation(
        &self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        match operation {
            Some(op) => {
                // Serialize operation to bytes
                let bytes = op.to_bytes();
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    fn timelock_get_min_delay(&self, asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        try_block_on(self.provider.get_native_asset_timelock_min_delay(&hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    fn timelock_set_min_delay(
        &mut self,
        asset: &[u8; 32],
        new_delay: u64,
        caller: &[u8; 32],
        _block_height: u64,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        // Validate caller has DEFAULT_ADMIN_ROLE to modify timelock minimum delay
        let admin_role = tos_common::native_asset::DEFAULT_ADMIN_ROLE;
        if !self.check_role(&hash, &admin_role, caller)? {
            return Err(Self::permission_denied_error(
                "Caller does not have DEFAULT_ADMIN_ROLE for timelock_set_min_delay",
            ));
        }

        try_block_on(
            self.provider
                .set_native_asset_timelock_min_delay(&hash, new_delay),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    fn timelock_get_timestamp(
        &self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
    ) -> Result<Option<u64>, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(operation.map(|op| op.ready_at))
    }

    fn timelock_is_operation(
        &self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(operation.is_some())
    }

    fn timelock_is_pending(
        &self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(operation.map_or(false, |op| op.status == TimelockStatus::Pending))
    }

    fn timelock_is_ready(
        &self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
        block_height: u64,
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(operation.map_or(false, |op| {
            op.status == TimelockStatus::Pending && block_height >= op.ready_at
        }))
    }

    fn timelock_is_done(
        &self,
        asset: &[u8; 32],
        operation_id: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(asset);

        let operation = try_block_on(
            self.provider
                .get_native_asset_timelock_operation(&hash, operation_id),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        Ok(operation.map_or(false, |op| op.status == TimelockStatus::Done))
    }
}
