use std::collections::HashMap;
use tos_common::{
    block::TopoHeight,
    contract::{ContractProvider, TransferOutput},
    crypto::{Hash, PublicKey},
    serializer::Serializer,
};
/// Account adapter: TOS ContractProvider → TAKO AccountProvider
///
/// This module bridges TOS's account and balance system with TAKO VM's balance/transfer syscalls.
use tos_program_runtime::storage::AccountProvider;
use tos_tbpf::error::EbpfError;

/// Adapter that wraps TOS's ContractProvider to implement TAKO's AccountProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall tos_get_balance(address)
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
/// ```rust,ignore
/// use tako_integration::TosAccountAdapter;
///
/// let adapter = TosAccountAdapter::new(&tos_provider, topoheight);
///
/// // Get balance of an account (native asset)
/// let balance = adapter.get_balance(&account_address)?;
///
/// // Transfer from contract to user (native asset)
/// adapter.transfer(&contract_address, &user_address, 1000)?;
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

        let balance = self
            .provider
            .get_account_balance_for_asset(&pubkey, &self.native_asset, self.topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to get account balance: {}", e),
                )))
            })?;

        // Return balance or 0 if account doesn't exist
        Ok(balance.map(|(_, bal)| bal).unwrap_or(0))
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

        // Calculate virtual balance (actual balance + pending deltas)
        // This prevents double-spend when a contract calls transfer multiple times
        let delta = self.balance_deltas.get(&from_pubkey).unwrap_or(&0);
        let virtual_balance = if *delta < 0 {
            // Subtract the negative delta (amount already staged for sending)
            actual_balance.saturating_sub(delta.unsigned_abs() as u64)
        } else {
            // Add the positive delta (amount already staged for receiving)
            actual_balance.saturating_add(*delta as u64)
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

        // Check if recipient account exists
        let recipient_exists = self
            .provider
            .account_exists(&to_pubkey, self.topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to check recipient account: {}", e),
                )))
            })?;

        if !recipient_exists {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Recipient account does not exist",
            ))));
        }

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
            _key: &tos_vm::ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<tos_vm::ValueCell>)>, anyhow::Error> {
            Ok(None)
        }

        fn load_data_latest_topoheight(
            &self,
            _contract: &Hash,
            _key: &tos_vm::ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(None)
        }

        fn has_data(
            &self,
            _contract: &Hash,
            _key: &tos_vm::ValueCell,
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
    fn test_transfer_nonexistent_recipient() {
        let mut provider = MockProvider::new();
        let from = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let to = PublicKey::from_bytes(&[2u8; 32]).unwrap();

        provider.set_balance(from.clone(), Hash::zero(), 10000);
        // Don't add 'to' account

        let mut adapter = TosAccountAdapter::new(&provider, 100);
        let result = adapter.transfer(from.as_bytes(), to.as_bytes(), 1000);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
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
}
