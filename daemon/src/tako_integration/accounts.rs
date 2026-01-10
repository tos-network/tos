use std::collections::HashMap;
use tos_common::{
    block::TopoHeight,
    contract::{ContractProvider, TransferOutput},
    crypto::{Hash, PublicKey},
    serializer::Serializer,
};
/// Account adapter: TOS ContractProvider → TAKO AccountProvider
///
/// This module bridges TOS's account and balance system with TOS Kernel(TAKO)'s balance/transfer syscalls.
use tos_program_runtime::storage::AccountProvider;
use tos_tbpf::error::EbpfError;

/// Adapter that wraps TOS's ContractProvider to implement TAKO's AccountProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall get_balance(address)
///     ↓
/// TosAccountAdapter::get_balance()
///     ↓
/// TOS ContractProvider::get_account_balance_for_asset()
///     ↓
/// RocksDB Balances column family
/// ```
///
/// # Asset Management
///
/// TOS has a multi-asset system where each account can hold balances in multiple assets.
/// For TAKO integration, we default to the native TOS asset (Hash::zero()) for transfers
/// and balance queries unless otherwise specified.
///
/// # Example
///
/// ```no_run
/// use tos_daemon::tako_integration::TosAccountAdapter;
/// use tos_program_runtime::storage::AccountProvider;
/// use tos_common::contract::ContractProvider;
/// use tos_common::crypto::Hash;
/// use tos_common::block::TopoHeight;
///
/// # // Mock provider for demonstration
/// # struct MockProvider;
/// # impl ContractProvider for MockProvider {
/// #     fn get_contract_balance_for_asset(&self, _: &Hash, _: &Hash, _: TopoHeight) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> { Ok(None) }
/// #     fn get_account_balance_for_asset(&self, _: &tos_common::crypto::PublicKey, _: &Hash, _: TopoHeight) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> { Ok(Some((100, 5000))) }
/// #     fn asset_exists(&self, _: &Hash, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(true) }
/// #     fn load_asset_data(&self, _: &Hash, _: TopoHeight) -> Result<Option<(TopoHeight, tos_common::asset::AssetData)>, anyhow::Error> { Ok(None) }
/// #     fn load_asset_supply(&self, _: &Hash, _: TopoHeight) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> { Ok(None) }
/// #     fn account_exists(&self, _: &tos_common::crypto::PublicKey, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(true) }
/// #     fn load_contract_module(&self, _: &Hash, _: TopoHeight) -> Result<Option<Vec<u8>>, anyhow::Error> { Ok(None) }
/// # }
/// # impl tos_common::contract::ContractStorage for MockProvider {
/// #     fn load_data(&self, _: &Hash, _: &tos_kernel::ValueCell, _: TopoHeight) -> Result<Option<(TopoHeight, Option<tos_kernel::ValueCell>)>, anyhow::Error> { Ok(None) }
/// #     fn load_data_latest_topoheight(&self, _: &Hash, _: &tos_kernel::ValueCell, _: TopoHeight) -> Result<Option<TopoHeight>, anyhow::Error> { Ok(None) }
/// #     fn has_data(&self, _: &Hash, _: &tos_kernel::ValueCell, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn has_contract(&self, _: &Hash, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(false) }
/// # }
///
/// // Create adapter
/// let provider = MockProvider;
/// let topoheight = 100;
/// let adapter = TosAccountAdapter::new(&provider, topoheight);
///
/// // Get balance of an account (native asset)
/// let account_address = [1u8; 32];
/// let balance = adapter.get_balance(&account_address).ok();
///
/// // Transfer from contract to user (native asset)
/// let contract_address = [2u8; 32];
/// let user_address = [3u8; 32];
/// let mut adapter = TosAccountAdapter::new(&provider, topoheight);
/// adapter.transfer(&contract_address, &user_address, 1000).ok();
/// ```
pub struct TosAccountAdapter<'a> {
    /// TOS contract provider (backend)
    provider: &'a (dyn ContractProvider + Send),
    /// Current topoheight (for versioned reads)
    topoheight: TopoHeight,
    /// Native asset hash (TOS uses Hash::zero() for native tokens)
    native_asset: Hash,
    /// Transfers staged during the current execution
    pending_transfers: Vec<TransferOutput>,
    /// Balance deltas for accounts during the current execution
    /// Used to prevent double-spend when a contract calls transfer multiple times
    /// Maps: PublicKey → balance delta (positive = received, negative = sent)
    balance_deltas: HashMap<PublicKey, i128>,
}

impl<'a> TosAccountAdapter<'a> {
    /// Create a new account adapter
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS contract provider
    /// * `topoheight` - Current topoheight for versioned reads
    pub fn new(provider: &'a (dyn ContractProvider + Send), topoheight: TopoHeight) -> Self {
        Self {
            provider,
            topoheight,
            native_asset: Hash::zero(), // TOS native asset
            pending_transfers: Vec::new(),
            balance_deltas: HashMap::new(),
        }
    }

    /// Create a new account adapter with a specific asset
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS contract provider
    /// * `topoheight` - Current topoheight for versioned reads
    /// * `asset` - Asset hash to use for balance queries
    pub fn new_with_asset(
        provider: &'a (dyn ContractProvider + Send),
        topoheight: TopoHeight,
        asset: Hash,
    ) -> Self {
        Self {
            provider,
            topoheight,
            native_asset: asset,
            pending_transfers: Vec::new(),
            balance_deltas: HashMap::new(),
        }
    }

    /// Convert 32-byte address to TOS PublicKey
    fn address_to_pubkey(address: &[u8; 32]) -> Result<PublicKey, EbpfError> {
        <PublicKey as Serializer>::from_bytes(address).map_err(|e| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid address format: {}", e),
            )))
        })
    }

    /// Consume the transfers staged during the execution and return them.
    pub fn take_pending_transfers(&mut self) -> Vec<TransferOutput> {
        std::mem::take(&mut self.pending_transfers)
    }
}

impl<'a> AccountProvider for TosAccountAdapter<'a> {
    fn get_balance(&self, address: &[u8; 32]) -> Result<u64, EbpfError> {
        let pubkey = Self::address_to_pubkey(address)?;

        let actual_balance = self
            .provider
            .get_account_balance_for_asset(&pubkey, &self.native_asset, self.topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to get account balance: {}", e),
                )))
            })?
            .map(|(_, bal)| bal)
            .unwrap_or(0);

        // Apply pending balance deltas to get virtual balance
        // This ensures that balance queries during execution see the effects of prior transfers
        let delta = self.balance_deltas.get(&pubkey).unwrap_or(&0);
        let virtual_balance = match delta.cmp(&0) {
            std::cmp::Ordering::Less => {
                // Negative delta: subtract from balance
                let abs_delta = delta.unsigned_abs();
                if abs_delta > actual_balance as u128 {
                    // This shouldn't happen if transfers were validated correctly
                    return Ok(0);
                }
                actual_balance - (abs_delta as u64)
            }
            std::cmp::Ordering::Greater => {
                // Positive delta: add to balance
                if *delta > u64::MAX as i128 {
                    // Cap at u64::MAX
                    return Ok(u64::MAX);
                }
                actual_balance.saturating_add(*delta as u64)
            }
            std::cmp::Ordering::Equal => {
                // Zero delta: return actual balance
                actual_balance
            }
        };

        Ok(virtual_balance)
    }

    fn transfer(&mut self, from: &[u8; 32], to: &[u8; 32], amount: u64) -> Result<(), EbpfError> {
        // Note: In TOS, transfers are not executed immediately during contract execution.
        // Instead, they are accumulated as ContractOutput::Transfer and processed after
        // contract execution completes. This ensures atomicity and proper balance checks.
        //
        // For TAKO integration Phase 1, we implement a simplified approach:
        // - Balance checks are performed immediately
        // - Actual transfers are cached and applied after execution
        //
        // TODO [Phase 2]: Integrate with TOS's ContractOutput system for full transaction atomicity

        // EVM-compatible behavior: zero-amount transfers are no-ops
        // This matches Ethereum's CALL behavior where value=0 transfers always succeed
        if amount == 0 {
            return Ok(());
        }

        let from_pubkey = Self::address_to_pubkey(from)?;
        let to_pubkey = Self::address_to_pubkey(to)?;

        // Get actual balance from provider
        let actual_balance = self
            .provider
            .get_account_balance_for_asset(&from_pubkey, &self.native_asset, self.topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to get sender balance: {}", e),
                )))
            })?
            .map(|(_, bal)| bal)
            .unwrap_or(0);

        // SECURITY FIX (R2-C1-03): Integer overflow protection in balance delta calculations
        //
        // Calculate virtual balance (actual balance + pending deltas)
        // This prevents double-spend when a contract calls transfer multiple times
        //
        // Previously, this code used unchecked casting from i128 to u64:
        //   actual_balance.saturating_add(*delta as u64)
        // This could cause integer overflow or incorrect balance calculations.
        //
        // New implementation uses explicit overflow checks and bounds validation.
        let delta = self.balance_deltas.get(&from_pubkey).unwrap_or(&0);
        let virtual_balance = match delta.cmp(&0) {
            std::cmp::Ordering::Less => {
                // Negative delta: subtract from balance with underflow protection
                let abs_delta = delta.unsigned_abs();

                // Check if delta magnitude exceeds actual balance
                if abs_delta > actual_balance as u128 {
                    return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "Negative delta {} exceeds actual balance {}",
                            delta, actual_balance
                        ),
                    ))));
                }

                // Safe subtraction: we've verified abs_delta <= actual_balance
                actual_balance - (abs_delta as u64)
            }
            std::cmp::Ordering::Greater => {
                // Positive delta: add to balance with overflow protection

                // Check if delta exceeds u64::MAX (would overflow when casting)
                if *delta > u64::MAX as i128 {
                    return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Delta {} exceeds maximum allowed value (u64::MAX)", delta),
                    ))));
                }

                // Use checked_add to detect overflow when adding to actual_balance
                actual_balance.checked_add(*delta as u64).ok_or_else(|| {
                    EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "Balance overflow: {} + {} exceeds u64::MAX",
                            actual_balance, delta
                        ),
                    )))
                })?
            }
            std::cmp::Ordering::Equal => {
                // Zero delta: return actual balance unchanged
                actual_balance
            }
        };

        // Verify sufficient virtual balance
        if virtual_balance < amount {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Insufficient balance: has {} (actual: {}, pending delta: {}), needs {}",
                    virtual_balance, actual_balance, delta, amount
                ),
            ))));
        }

        // Note: We no longer check if recipient account exists.
        // Account creation is FREE, so transfers to new addresses will auto-create the account.
        // This matches the behavior of regular TOS transfers.

        // Update balance deltas to track virtual balances
        // Sender loses amount (negative delta)
        *self.balance_deltas.entry(from_pubkey.clone()).or_insert(0) -= amount as i128;
        // Recipient gains amount (positive delta)
        *self.balance_deltas.entry(to_pubkey.clone()).or_insert(0) += amount as i128;

        // Stage the transfer so the outer transaction processor can persist it atomically.
        self.pending_transfers.push(TransferOutput {
            destination: to_pubkey,
            amount,
            asset: self.native_asset.clone(),
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tos_common::{
        asset::AssetData,
        crypto::{Hash, PublicKey},
    };

    // Mock ContractProvider for testing
    struct MockProvider {
        balances: HashMap<(PublicKey, Hash), u64>,
        accounts: HashMap<PublicKey, bool>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                balances: HashMap::new(),
                accounts: HashMap::new(),
            }
        }

        fn set_balance(&mut self, pubkey: PublicKey, asset: Hash, balance: u64) {
            self.balances.insert((pubkey.clone(), asset), balance);
            self.accounts.insert(pubkey, true);
        }
    }

    impl ContractProvider for MockProvider {
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
            key: &PublicKey,
            asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(self
                .balances
                .get(&(key.clone(), asset.clone()))
                .map(|bal| (100, *bal)))
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
            key: &PublicKey,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(self.accounts.contains_key(key))
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
            _contract: &Hash,
            _key: &tos_kernel::ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<tos_kernel::ValueCell>)>, anyhow::Error> {
            Ok(None)
        }

        fn load_data_latest_topoheight(
            &self,
            _contract: &Hash,
            _key: &tos_kernel::ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(None)
        }

        fn has_data(
            &self,
            _contract: &Hash,
            _key: &tos_kernel::ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }

        fn has_contract(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }
    }

    #[test]
    fn test_get_balance_existing_account() {
        let mut provider = MockProvider::new();
        let pubkey = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        provider.set_balance(pubkey.clone(), Hash::zero(), 5000);

        let adapter = TosAccountAdapter::new(&provider, 100);
        let balance = adapter.get_balance(pubkey.as_bytes()).unwrap();
        assert_eq!(balance, 5000);
    }

    #[test]
    fn test_get_balance_nonexistent_account() {
        let provider = MockProvider::new();
        let pubkey = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        let adapter = TosAccountAdapter::new(&provider, 100);
        let balance = adapter.get_balance(pubkey.as_bytes()).unwrap();
        assert_eq!(balance, 0);
    }

    #[test]
    fn test_transfer_sufficient_balance() {
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 10000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 5000);
        assert!(result.is_ok());

        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(
            transfers[0],
            TransferOutput {
                destination: to,
                amount: 5000,
                asset: Hash::zero(),
            }
        );

        // Queue should be empty after consuming
        assert!(adapter.take_pending_transfers().is_empty());
    }

    #[test]
    fn test_transfer_insufficient_balance() {
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 1000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 5000);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Insufficient balance"));
    }

    #[test]
    fn test_transfer_to_new_account_auto_creates() {
        // Account creation is FREE, so transfers to non-existent accounts should succeed
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 10000);
        // Don't add 'to' account - it will be auto-created by the transfer

        let mut adapter = TosAccountAdapter::new(&provider, 100);
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 1000);

        // Transfer should succeed - account will be auto-created
        assert!(
            result.is_ok(),
            "Transfer to new account should succeed (auto-create)"
        );

        // Verify the transfer was staged
        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount, 1000);
        assert_eq!(transfers[0].destination, to);
    }

    #[test]
    fn test_transfer_multiple_exceeding_balance() {
        // Test that multiple transfers are tracked with virtual balance
        // to prevent double-spend attacks
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        // Sender has 1000 units
        provider.set_balance(from.clone(), Hash::zero(), 1000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // First transfer of 600 succeeds
        let result1 = adapter.transfer(from.as_bytes(), to.as_bytes(), 600);
        assert!(result1.is_ok());

        // Second transfer of 300 succeeds (total 900, still under 1000)
        let result2 = adapter.transfer(from.as_bytes(), to.as_bytes(), 300);
        assert!(result2.is_ok());

        // Third transfer of 200 should fail (total would be 1100, exceeds 1000)
        let result3 = adapter.transfer(from.as_bytes(), to.as_bytes(), 200);
        assert!(result3.is_err());
        assert!(result3
            .unwrap_err()
            .to_string()
            .contains("Insufficient balance"));

        // Verify that exactly 2 transfers were staged
        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 2);
        assert_eq!(transfers[0].amount, 600);
        assert_eq!(transfers[1].amount, 300);
    }

    #[test]
    fn test_transfer_to_self() {
        // Test transferring to self - balance delta should be zero
        let mut provider = MockProvider::new();
        let account = PublicKey::from_bytes(&[1u8; 32]).unwrap();

        provider.set_balance(account.clone(), Hash::zero(), 1000);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Transfer to self should work
        let result = adapter.transfer(account.as_bytes(), account.as_bytes(), 500);
        assert!(result.is_ok());

        // Second transfer to self should also work (net delta is 0)
        let result2 = adapter.transfer(account.as_bytes(), account.as_bytes(), 500);
        assert!(result2.is_ok());

        // Third transfer to self should also work
        let result3 = adapter.transfer(account.as_bytes(), account.as_bytes(), 500);
        assert!(result3.is_ok());

        // All transfers should be staged
        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 3);
    }

    // ============================================================================
    // SECURITY TESTS (R2-C1-03): Integer Overflow Protection
    // ============================================================================
    //
    // These tests verify that the balance delta calculation code properly
    // handles integer overflow scenarios that could lead to:
    // 1. Incorrect balance calculations
    // 2. Privilege escalation (creating balance out of thin air)
    // 3. DoS attacks (panics from arithmetic overflow)
    //
    // Reference: Security Audit Finding R2-C1-03
    // Fixed: Lines 161-219 (balance delta calculation with overflow checks)

    #[test]
    fn test_security_i128_max_delta_causes_error() {
        // SECURITY TEST: Verify that i128::MAX delta is rejected (not overflow/panic)
        //
        // Attack scenario: Malicious contract tries to cause overflow by
        // manipulating balance_deltas to contain i128::MAX
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 1000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Manually inject i128::MAX into balance_deltas (simulating attack)
        adapter.balance_deltas.insert(from.clone(), i128::MAX);

        // Attempt transfer - should return error, not panic/overflow
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 100);

        assert!(result.is_err(), "Expected error for i128::MAX delta");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("exceeds maximum allowed value") || err_msg.contains("overflow"),
            "Error message should mention overflow or maximum value, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_security_negative_delta_exceeding_balance_causes_error() {
        // SECURITY TEST: Verify that negative delta exceeding balance is rejected
        //
        // Attack scenario: Contract has staged outgoing transfers exceeding balance
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        // Account has 1000 units
        provider.set_balance(from.clone(), Hash::zero(), 1000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Manually inject negative delta larger than balance (simulating attack)
        // This could happen if balance_deltas tracking is corrupted
        adapter.balance_deltas.insert(from.clone(), -5000i128);

        // Attempt transfer - should detect that virtual balance would be negative
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 100);

        assert!(
            result.is_err(),
            "Expected error when negative delta exceeds balance"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("exceeds actual balance") || err_msg.contains("Negative delta"),
            "Error message should mention delta exceeding balance, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_security_accumulated_deltas_causing_overflow() {
        // SECURITY TEST: Verify that accumulated positive deltas causing overflow are rejected
        //
        // Attack scenario: Multiple incoming transfers push virtual balance over u64::MAX
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        // Account has maximum balance
        provider.set_balance(from.clone(), Hash::zero(), u64::MAX);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Manually inject positive delta that would overflow when added to u64::MAX
        adapter.balance_deltas.insert(from.clone(), 1000i128);

        // Attempt transfer - should detect overflow in virtual balance calculation
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 100);

        assert!(
            result.is_err(),
            "Expected error when accumulated deltas cause overflow"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("overflow") || err_msg.contains("exceeds u64::MAX"),
            "Error message should mention overflow, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_security_edge_case_u64_max_balance() {
        // SECURITY TEST: Verify correct handling of u64::MAX balance
        //
        // Edge case: Account has maximum possible balance
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        // Set balance to maximum u64 value
        provider.set_balance(from.clone(), Hash::zero(), u64::MAX);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Transfer with u64::MAX balance should work
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 1000);
        assert!(
            result.is_ok(),
            "Transfer with u64::MAX balance should succeed"
        );

        // Verify the transfer was staged
        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount, 1000);
    }

    #[test]
    fn test_security_zero_delta_no_overflow() {
        // SECURITY TEST: Verify that zero delta doesn't cause issues
        //
        // Edge case: Delta is exactly zero
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 5000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Explicitly set delta to zero
        adapter.balance_deltas.insert(from.clone(), 0i128);

        // Transfer should work normally
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 1000);
        assert!(result.is_ok(), "Transfer with zero delta should succeed");

        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount, 1000);
    }

    #[test]
    fn test_security_large_positive_delta_within_bounds() {
        // SECURITY TEST: Verify large positive deltas work correctly when within u64 bounds
        //
        // Valid scenario: Large positive delta that doesn't overflow
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 1_000_000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Large but valid positive delta
        let large_delta = 500_000i128;
        adapter.balance_deltas.insert(from.clone(), large_delta);

        // Virtual balance should be 1_000_000 + 500_000 = 1_500_000
        // Transfer of 1_400_000 should succeed
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 1_400_000);
        assert!(
            result.is_ok(),
            "Transfer with large positive delta should succeed"
        );

        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount, 1_400_000);
    }

    #[test]
    fn test_security_large_negative_delta_within_bounds() {
        // SECURITY TEST: Verify large negative deltas work correctly when within balance
        //
        // Valid scenario: Large negative delta that doesn't exceed balance
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 1_000_000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Large but valid negative delta
        let large_delta = -500_000i128;
        adapter.balance_deltas.insert(from.clone(), large_delta);

        // Virtual balance should be 1_000_000 - 500_000 = 500_000
        // Transfer of 400_000 should succeed
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 400_000);
        assert!(
            result.is_ok(),
            "Transfer with large negative delta should succeed"
        );

        let transfers = adapter.take_pending_transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount, 400_000);
    }

    #[test]
    fn test_zero_amount_transfer_to_nonexistent_account() {
        // EVM-compatible: zero-amount transfers are no-ops, even to non-existent accounts
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[0x33u8; 32]).unwrap();

        // Sender has some balance
        provider.set_balance(from.clone(), Hash::zero(), 1000);
        // DON'T add 'to' account - it doesn't exist

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Zero transfer should succeed (no-op)
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 0);
        assert!(result.is_ok(), "Zero-amount transfer should succeed");

        // No transfer should be staged (it's a no-op)
        let transfers = adapter.take_pending_transfers();
        assert_eq!(
            transfers.len(),
            0,
            "Zero transfer should not stage anything"
        );
    }

    #[test]
    fn test_zero_amount_transfer_to_existing_account() {
        // Zero-amount transfers should succeed without any checks
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 1000);
        provider.set_balance(to.clone(), Hash::zero(), 0);

        let mut adapter = TosAccountAdapter::new(&provider, 100);

        // Zero transfer should succeed
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 0);
        assert!(result.is_ok(), "Zero-amount transfer should succeed");

        // No transfer staged
        let transfers = adapter.take_pending_transfers();
        assert_eq!(
            transfers.len(),
            0,
            "Zero transfer should not stage anything"
        );
    }
}
