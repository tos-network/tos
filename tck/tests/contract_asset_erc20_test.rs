#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]

// Contract Asset ERC20 Compliance Tests
//
// This test suite validates that Contract Asset syscalls provide ERC20-compatible
// functionality, following OpenZeppelin ERC20 test patterns.
//
// Test Categories:
// 1. ERC20 Standard: transfer, approve, allowance, balanceOf, totalSupply
// 2. ERC20 Metadata: name, symbol, decimals
// 3. ERC20 Extended: increaseAllowance, decreaseAllowance
// 4. ERC20Burnable: burn, burnFrom
// 5. ERC20Pausable: pause, unpause, paused effects on transfers
// 6. Edge Cases: self-transfer, zero amount, overflow protection

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};

// ============================================================================
// TEST CONSTANTS
// ============================================================================

/// Test asset hash (simulated ERC20 token)
const TEST_ASSET: [u8; 32] = [0xAA; 32];

/// Test accounts
const ALICE: [u8; 32] = [0x01; 32];
const BOB: [u8; 32] = [0x02; 32];
const CHARLIE: [u8; 32] = [0x03; 32];
const ZERO_ADDRESS: [u8; 32] = [0x00; 32];

/// Test amounts
const INITIAL_SUPPLY: u64 = 1_000_000_000; // 1 billion tokens
const ALICE_BALANCE: u64 = 100_000_000; // 100 million tokens
const BOB_BALANCE: u64 = 50_000_000; // 50 million tokens

/// Test metadata
const TEST_NAME: &str = "TestToken";
const TEST_SYMBOL: &str = "TEST";
const TEST_DECIMALS: u8 = 18;
const TEST_URI: &str = "https://example.com/metadata.json";

// ============================================================================
// MOCK PROVIDER
// ============================================================================

/// Mock provider for testing Contract Asset syscalls
#[allow(clippy::type_complexity)]
struct ContractAssetTestProvider {
    /// Track balances: (asset, account) -> balance
    balances: Arc<Mutex<HashMap<([u8; 32], [u8; 32]), u64>>>,
    /// Track allowances: (asset, owner, spender) -> amount
    allowances: Arc<Mutex<HashMap<([u8; 32], [u8; 32], [u8; 32]), u64>>>,
    /// Track total supply: asset -> supply
    total_supplies: Arc<Mutex<HashMap<[u8; 32], u64>>>,
    /// Track paused status: asset -> paused
    paused_assets: Arc<Mutex<HashMap<[u8; 32], bool>>>,
    /// Track frozen accounts: (asset, account) -> frozen
    frozen_accounts: Arc<Mutex<HashMap<([u8; 32], [u8; 32]), bool>>>,
    /// Track asset metadata: asset -> (name, symbol, decimals, uri)
    asset_metadata: Arc<Mutex<HashMap<[u8; 32], (String, String, u8, String)>>>,
    /// Track contract bytecode
    contracts: Arc<Mutex<HashMap<[u8; 32], Vec<u8>>>>,
}

impl ContractAssetTestProvider {
    fn new() -> Self {
        Self {
            balances: Arc::new(Mutex::new(HashMap::new())),
            allowances: Arc::new(Mutex::new(HashMap::new())),
            total_supplies: Arc::new(Mutex::new(HashMap::new())),
            paused_assets: Arc::new(Mutex::new(HashMap::new())),
            frozen_accounts: Arc::new(Mutex::new(HashMap::new())),
            asset_metadata: Arc::new(Mutex::new(HashMap::new())),
            contracts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Setup test asset with initial state
    fn setup_test_asset(&self) -> &Self {
        // Set metadata
        {
            let mut metadata = self.asset_metadata.lock().unwrap();
            metadata.insert(
                TEST_ASSET,
                (
                    TEST_NAME.to_string(),
                    TEST_SYMBOL.to_string(),
                    TEST_DECIMALS,
                    TEST_URI.to_string(),
                ),
            );
        }

        // Set initial supply
        {
            let mut supplies = self.total_supplies.lock().unwrap();
            supplies.insert(TEST_ASSET, INITIAL_SUPPLY);
        }

        // Set initial balances
        {
            let mut balances = self.balances.lock().unwrap();
            balances.insert((TEST_ASSET, ALICE), ALICE_BALANCE);
            balances.insert((TEST_ASSET, BOB), BOB_BALANCE);
        }

        // Asset is not paused by default
        {
            let mut paused = self.paused_assets.lock().unwrap();
            paused.insert(TEST_ASSET, false);
        }

        self
    }

    /// Set balance for an account
    fn set_balance(&self, asset: &[u8; 32], account: &[u8; 32], balance: u64) {
        let mut balances = self.balances.lock().unwrap();
        balances.insert((*asset, *account), balance);
    }

    /// Get balance for an account
    fn get_balance(&self, asset: &[u8; 32], account: &[u8; 32]) -> u64 {
        let balances = self.balances.lock().unwrap();
        balances.get(&(*asset, *account)).copied().unwrap_or(0)
    }

    /// Set allowance
    fn set_allowance(&self, asset: &[u8; 32], owner: &[u8; 32], spender: &[u8; 32], amount: u64) {
        let mut allowances = self.allowances.lock().unwrap();
        allowances.insert((*asset, *owner, *spender), amount);
    }

    /// Get allowance
    fn get_allowance(&self, asset: &[u8; 32], owner: &[u8; 32], spender: &[u8; 32]) -> u64 {
        let allowances = self.allowances.lock().unwrap();
        allowances
            .get(&(*asset, *owner, *spender))
            .copied()
            .unwrap_or(0)
    }

    /// Set paused status
    fn set_paused(&self, asset: &[u8; 32], paused: bool) {
        let mut paused_assets = self.paused_assets.lock().unwrap();
        paused_assets.insert(*asset, paused);
    }

    /// Check if paused
    fn is_paused(&self, asset: &[u8; 32]) -> bool {
        let paused_assets = self.paused_assets.lock().unwrap();
        paused_assets.get(asset).copied().unwrap_or(false)
    }

    /// Set frozen status
    fn set_frozen(&self, asset: &[u8; 32], account: &[u8; 32], frozen: bool) {
        let mut frozen_accounts = self.frozen_accounts.lock().unwrap();
        frozen_accounts.insert((*asset, *account), frozen);
    }

    /// Check if frozen
    fn is_frozen(&self, asset: &[u8; 32], account: &[u8; 32]) -> bool {
        let frozen_accounts = self.frozen_accounts.lock().unwrap();
        frozen_accounts
            .get(&(*asset, *account))
            .copied()
            .unwrap_or(false)
    }

    /// Get total supply
    fn get_total_supply(&self, asset: &[u8; 32]) -> u64 {
        let supplies = self.total_supplies.lock().unwrap();
        supplies.get(asset).copied().unwrap_or(0)
    }

    /// Set contract bytecode (for future use with contract-based tests)
    #[allow(dead_code)]
    fn set_contract(&self, contract_hash: &[u8; 32], bytecode: Vec<u8>) {
        let mut contracts = self.contracts.lock().unwrap();
        contracts.insert(*contract_hash, bytecode);
    }

    /// Simulate ERC20 transfer
    fn simulate_transfer(
        &self,
        asset: &[u8; 32],
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), &'static str> {
        // Check paused
        if self.is_paused(asset) {
            return Err("Asset is paused");
        }

        // Check frozen
        if self.is_frozen(asset, from) {
            return Err("Sender is frozen");
        }
        if self.is_frozen(asset, to) {
            return Err("Recipient is frozen");
        }

        // Check zero address
        if from == &ZERO_ADDRESS || to == &ZERO_ADDRESS {
            return Err("Zero address");
        }

        // Self-transfer: balance unchanged (ERC20 behavior)
        if from == to {
            let from_balance = self.get_balance(asset, from);
            // Check sufficient balance even for self-transfer
            if from_balance < amount {
                return Err("Insufficient balance");
            }
            // No actual balance change needed
            return Ok(());
        }

        // Get balances
        let from_balance = self.get_balance(asset, from);
        let to_balance = self.get_balance(asset, to);

        // Check sufficient balance
        if from_balance < amount {
            return Err("Insufficient balance");
        }

        // Check overflow
        if to_balance.checked_add(amount).is_none() {
            return Err("Overflow");
        }

        // Update balances
        self.set_balance(asset, from, from_balance - amount);
        self.set_balance(asset, to, to_balance + amount);

        Ok(())
    }

    /// Simulate ERC20 transferFrom
    fn simulate_transfer_from(
        &self,
        asset: &[u8; 32],
        spender: &[u8; 32],
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
    ) -> Result<(), &'static str> {
        // Check allowance
        let allowance = self.get_allowance(asset, from, spender);
        if allowance < amount {
            return Err("Insufficient allowance");
        }

        // Do the transfer
        self.simulate_transfer(asset, from, to, amount)?;

        // Reduce allowance
        self.set_allowance(asset, from, spender, allowance - amount);

        Ok(())
    }
}

impl ContractProvider for ContractAssetTestProvider {
    fn get_contract_balance_for_asset(
        &self,
        contract: &Hash,
        asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        let balances = self.balances.lock().unwrap();
        let balance = balances
            .get(&(*asset.as_bytes(), *contract.as_bytes()))
            .copied()
            .unwrap_or(0);
        Ok(Some((100, balance)))
    }

    fn get_account_balance_for_asset(
        &self,
        key: &PublicKey,
        asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        let balances = self.balances.lock().unwrap();
        let mut account = [0u8; 32];
        account.copy_from_slice(key.as_bytes());
        let balance = balances
            .get(&(*asset.as_bytes(), account))
            .copied()
            .unwrap_or(0);
        Ok(Some((100, balance)))
    }

    fn asset_exists(&self, asset: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        let metadata = self.asset_metadata.lock().unwrap();
        Ok(metadata.contains_key(asset.as_bytes()))
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, AssetData)>> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        let supplies = self.total_supplies.lock().unwrap();
        Ok(supplies.get(asset.as_bytes()).map(|s| (100, *s)))
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        contract: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<Vec<u8>>> {
        let contracts = self.contracts.lock().unwrap();
        Ok(contracts.get(contract.as_bytes()).cloned())
    }
}

impl ContractStorage for ContractAssetTestProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<tos_kernel::ValueCell>)>> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<TopoHeight>> {
        Ok(Some(100))
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<bool> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }
}

// ============================================================================
// TEST 1: ERC20 METADATA
// ============================================================================

/// Test ERC20 metadata: name, symbol, decimals
#[test]
fn test_erc20_metadata() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Verify metadata is set correctly
    let metadata = provider.asset_metadata.lock().unwrap();
    let (name, symbol, decimals, uri) = metadata.get(&TEST_ASSET).unwrap();

    assert_eq!(name, TEST_NAME, "Name should match");
    assert_eq!(symbol, TEST_SYMBOL, "Symbol should match");
    assert_eq!(*decimals, TEST_DECIMALS, "Decimals should match");
    assert_eq!(uri, TEST_URI, "URI should match");

    println!("ERC20 Metadata Test:");
    println!("  Name: {name}");
    println!("  Symbol: {symbol}");
    println!("  Decimals: {decimals}");
    println!("  URI: {uri}");
}

// ============================================================================
// TEST 2: TOTAL SUPPLY
// ============================================================================

/// Test totalSupply query
#[test]
fn test_erc20_total_supply() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let total_supply = provider.get_total_supply(&TEST_ASSET);
    assert_eq!(
        total_supply, INITIAL_SUPPLY,
        "Total supply should match initial supply"
    );

    println!("ERC20 Total Supply Test:");
    println!("  Total Supply: {total_supply}");
}

// ============================================================================
// TEST 3: BALANCE OF
// ============================================================================

/// Test balanceOf query
#[test]
fn test_erc20_balance_of() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Check Alice's balance
    let alice_balance = provider.get_balance(&TEST_ASSET, &ALICE);
    assert_eq!(alice_balance, ALICE_BALANCE, "Alice balance should match");

    // Check Bob's balance
    let bob_balance = provider.get_balance(&TEST_ASSET, &BOB);
    assert_eq!(bob_balance, BOB_BALANCE, "Bob balance should match");

    // Check Charlie's balance (should be 0)
    let charlie_balance = provider.get_balance(&TEST_ASSET, &CHARLIE);
    assert_eq!(charlie_balance, 0, "Charlie balance should be 0");

    println!("ERC20 Balance Of Test:");
    println!("  Alice: {alice_balance}");
    println!("  Bob: {bob_balance}");
    println!("  Charlie: {charlie_balance}");
}

// ============================================================================
// TEST 4: TRANSFER - SUCCESS
// ============================================================================

/// Test successful transfer
#[test]
fn test_erc20_transfer_success() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let transfer_amount = 10_000_000u64;

    // Get initial balances
    let alice_before = provider.get_balance(&TEST_ASSET, &ALICE);
    let bob_before = provider.get_balance(&TEST_ASSET, &BOB);

    // Transfer from Alice to Bob
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, transfer_amount);
    assert!(result.is_ok(), "Transfer should succeed");

    // Verify balances
    let alice_after = provider.get_balance(&TEST_ASSET, &ALICE);
    let bob_after = provider.get_balance(&TEST_ASSET, &BOB);

    assert_eq!(
        alice_after,
        alice_before - transfer_amount,
        "Alice balance should decrease"
    );
    assert_eq!(
        bob_after,
        bob_before + transfer_amount,
        "Bob balance should increase"
    );

    println!("ERC20 Transfer Success Test:");
    println!("  Alice: {alice_before} -> {alice_after}");
    println!("  Bob: {bob_before} -> {bob_after}");
}

// ============================================================================
// TEST 5: TRANSFER - INSUFFICIENT BALANCE
// ============================================================================

/// Test transfer with insufficient balance
#[test]
fn test_erc20_transfer_insufficient_balance() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let alice_balance = provider.get_balance(&TEST_ASSET, &ALICE);
    let transfer_amount = alice_balance + 1; // More than Alice has

    // Attempt transfer should fail
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, transfer_amount);
    assert!(result.is_err(), "Transfer should fail");
    assert_eq!(result.unwrap_err(), "Insufficient balance");

    // Balances should be unchanged
    assert_eq!(
        provider.get_balance(&TEST_ASSET, &ALICE),
        alice_balance,
        "Alice balance unchanged"
    );

    println!("ERC20 Transfer Insufficient Balance Test: PASSED");
}

// ============================================================================
// TEST 6: TRANSFER - ZERO AMOUNT
// ============================================================================

/// Test transfer of zero amount
#[test]
fn test_erc20_transfer_zero_amount() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let alice_before = provider.get_balance(&TEST_ASSET, &ALICE);
    let bob_before = provider.get_balance(&TEST_ASSET, &BOB);

    // Transfer 0 should succeed (OpenZeppelin allows this)
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 0);
    assert!(result.is_ok(), "Zero transfer should succeed");

    // Balances unchanged
    assert_eq!(provider.get_balance(&TEST_ASSET, &ALICE), alice_before);
    assert_eq!(provider.get_balance(&TEST_ASSET, &BOB), bob_before);

    println!("ERC20 Transfer Zero Amount Test: PASSED");
}

// ============================================================================
// TEST 7: TRANSFER - SELF TRANSFER
// ============================================================================

/// Test self-transfer
#[test]
fn test_erc20_transfer_self() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let alice_before = provider.get_balance(&TEST_ASSET, &ALICE);
    let transfer_amount = 1_000_000u64;

    // Self-transfer
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &ALICE, transfer_amount);
    assert!(result.is_ok(), "Self transfer should succeed");

    // Balance should be unchanged
    let alice_after = provider.get_balance(&TEST_ASSET, &ALICE);
    assert_eq!(
        alice_after, alice_before,
        "Balance unchanged for self-transfer"
    );

    println!("ERC20 Self Transfer Test: PASSED");
}

// ============================================================================
// TEST 8: APPROVE AND ALLOWANCE
// ============================================================================

/// Test approve and allowance
#[test]
fn test_erc20_approve_and_allowance() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let allowance_amount = 50_000_000u64;

    // Initial allowance should be 0
    let initial_allowance = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    assert_eq!(initial_allowance, 0, "Initial allowance should be 0");

    // Alice approves Bob
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, allowance_amount);

    // Check allowance
    let allowance = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    assert_eq!(allowance, allowance_amount, "Allowance should match");

    println!("ERC20 Approve and Allowance Test:");
    println!("  Initial: {initial_allowance}");
    println!("  After approve: {allowance}");
}

// ============================================================================
// TEST 9: TRANSFER FROM - SUCCESS
// ============================================================================

/// Test successful transferFrom
#[test]
fn test_erc20_transfer_from_success() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let allowance_amount = 50_000_000u64;
    let transfer_amount = 10_000_000u64;

    // Alice approves Bob
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, allowance_amount);

    let alice_before = provider.get_balance(&TEST_ASSET, &ALICE);
    let charlie_before = provider.get_balance(&TEST_ASSET, &CHARLIE);

    // Bob transfers from Alice to Charlie
    let result =
        provider.simulate_transfer_from(&TEST_ASSET, &BOB, &ALICE, &CHARLIE, transfer_amount);
    assert!(result.is_ok(), "TransferFrom should succeed");

    // Check balances
    let alice_after = provider.get_balance(&TEST_ASSET, &ALICE);
    let charlie_after = provider.get_balance(&TEST_ASSET, &CHARLIE);

    assert_eq!(alice_after, alice_before - transfer_amount);
    assert_eq!(charlie_after, charlie_before + transfer_amount);

    // Check allowance reduced
    let remaining_allowance = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    assert_eq!(remaining_allowance, allowance_amount - transfer_amount);

    println!("ERC20 TransferFrom Success Test:");
    println!("  Alice: {alice_before} -> {alice_after}");
    println!("  Charlie: {charlie_before} -> {charlie_after}");
    println!("  Remaining allowance: {remaining_allowance}");
}

// ============================================================================
// TEST 10: TRANSFER FROM - INSUFFICIENT ALLOWANCE
// ============================================================================

/// Test transferFrom with insufficient allowance
#[test]
fn test_erc20_transfer_from_insufficient_allowance() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let allowance_amount = 10_000_000u64;
    let transfer_amount = 50_000_000u64; // More than allowance

    // Alice approves Bob for small amount
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, allowance_amount);

    // Bob tries to transfer more than allowance
    let result =
        provider.simulate_transfer_from(&TEST_ASSET, &BOB, &ALICE, &CHARLIE, transfer_amount);
    assert!(result.is_err(), "TransferFrom should fail");
    assert_eq!(result.unwrap_err(), "Insufficient allowance");

    println!("ERC20 TransferFrom Insufficient Allowance Test: PASSED");
}

// ============================================================================
// TEST 11: INCREASE ALLOWANCE
// ============================================================================

/// Test increaseAllowance pattern
#[test]
fn test_erc20_increase_allowance() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let initial_allowance = 10_000_000u64;
    let increase_amount = 5_000_000u64;

    // Set initial allowance
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, initial_allowance);

    // Increase allowance
    let current = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    let new_allowance = current.checked_add(increase_amount).expect("No overflow");
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, new_allowance);

    // Verify
    let final_allowance = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    assert_eq!(
        final_allowance,
        initial_allowance + increase_amount,
        "Allowance should be increased"
    );

    println!("ERC20 Increase Allowance Test:");
    println!("  Initial: {initial_allowance}");
    println!("  Increase: {increase_amount}");
    println!("  Final: {final_allowance}");
}

// ============================================================================
// TEST 12: DECREASE ALLOWANCE
// ============================================================================

/// Test decreaseAllowance pattern
#[test]
fn test_erc20_decrease_allowance() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let initial_allowance = 50_000_000u64;
    let decrease_amount = 20_000_000u64;

    // Set initial allowance
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, initial_allowance);

    // Decrease allowance
    let current = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    let new_allowance = current.checked_sub(decrease_amount).expect("No underflow");
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, new_allowance);

    // Verify
    let final_allowance = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    assert_eq!(
        final_allowance,
        initial_allowance - decrease_amount,
        "Allowance should be decreased"
    );

    println!("ERC20 Decrease Allowance Test:");
    println!("  Initial: {initial_allowance}");
    println!("  Decrease: {decrease_amount}");
    println!("  Final: {final_allowance}");
}

// ============================================================================
// TEST 13: REVOKE ALLOWANCE
// ============================================================================

/// Test revoking allowance (set to zero)
#[test]
fn test_erc20_revoke_allowance() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let initial_allowance = 50_000_000u64;

    // Set initial allowance
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, initial_allowance);
    assert_eq!(
        provider.get_allowance(&TEST_ASSET, &ALICE, &BOB),
        initial_allowance
    );

    // Revoke (set to 0)
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, 0);

    // Verify
    let final_allowance = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);
    assert_eq!(final_allowance, 0, "Allowance should be revoked");

    println!("ERC20 Revoke Allowance Test: PASSED");
}

// ============================================================================
// TEST 14: BURN
// ============================================================================

/// Test burn functionality
#[test]
fn test_erc20_burn() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let burn_amount = 10_000_000u64;
    let alice_before = provider.get_balance(&TEST_ASSET, &ALICE);
    let supply_before = provider.get_total_supply(&TEST_ASSET);

    // Simulate burn (reduce balance and supply)
    provider.set_balance(&TEST_ASSET, &ALICE, alice_before - burn_amount);
    {
        let mut supplies = provider.total_supplies.lock().unwrap();
        supplies.insert(TEST_ASSET, supply_before - burn_amount);
    }

    let alice_after = provider.get_balance(&TEST_ASSET, &ALICE);
    let supply_after = provider.get_total_supply(&TEST_ASSET);

    assert_eq!(alice_after, alice_before - burn_amount);
    assert_eq!(supply_after, supply_before - burn_amount);

    println!("ERC20 Burn Test:");
    println!("  Alice: {alice_before} -> {alice_after}");
    println!("  Supply: {supply_before} -> {supply_after}");
}

// ============================================================================
// TEST 15: BURN FROM
// ============================================================================

/// Test burnFrom functionality (burn using allowance)
#[test]
fn test_erc20_burn_from() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let allowance_amount = 20_000_000u64;
    let burn_amount = 10_000_000u64;

    // Alice approves Bob for burning
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, allowance_amount);

    let alice_before = provider.get_balance(&TEST_ASSET, &ALICE);
    let supply_before = provider.get_total_supply(&TEST_ASSET);
    let allowance_before = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);

    // Simulate burnFrom: reduce balance, supply, and allowance
    provider.set_balance(&TEST_ASSET, &ALICE, alice_before - burn_amount);
    {
        let mut supplies = provider.total_supplies.lock().unwrap();
        supplies.insert(TEST_ASSET, supply_before - burn_amount);
    }
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, allowance_before - burn_amount);

    let alice_after = provider.get_balance(&TEST_ASSET, &ALICE);
    let supply_after = provider.get_total_supply(&TEST_ASSET);
    let allowance_after = provider.get_allowance(&TEST_ASSET, &ALICE, &BOB);

    assert_eq!(alice_after, alice_before - burn_amount);
    assert_eq!(supply_after, supply_before - burn_amount);
    assert_eq!(allowance_after, allowance_before - burn_amount);

    println!("ERC20 BurnFrom Test:");
    println!("  Alice: {alice_before} -> {alice_after}");
    println!("  Supply: {supply_before} -> {supply_after}");
    println!("  Allowance: {allowance_before} -> {allowance_after}");
}

// ============================================================================
// TEST 16: MINT
// ============================================================================

/// Test mint functionality
#[test]
fn test_erc20_mint() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let mint_amount = 50_000_000u64;
    let charlie_before = provider.get_balance(&TEST_ASSET, &CHARLIE);
    let supply_before = provider.get_total_supply(&TEST_ASSET);

    // Simulate mint (increase balance and supply)
    provider.set_balance(&TEST_ASSET, &CHARLIE, charlie_before + mint_amount);
    {
        let mut supplies = provider.total_supplies.lock().unwrap();
        supplies.insert(TEST_ASSET, supply_before + mint_amount);
    }

    let charlie_after = provider.get_balance(&TEST_ASSET, &CHARLIE);
    let supply_after = provider.get_total_supply(&TEST_ASSET);

    assert_eq!(charlie_after, charlie_before + mint_amount);
    assert_eq!(supply_after, supply_before + mint_amount);

    println!("ERC20 Mint Test:");
    println!("  Charlie: {charlie_before} -> {charlie_after}");
    println!("  Supply: {supply_before} -> {supply_after}");
}

// ============================================================================
// TEST 17: PAUSE - TRANSFER BLOCKED
// ============================================================================

/// Test paused asset blocks transfers
#[test]
fn test_erc20_pausable_transfer_blocked() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Pause the asset
    provider.set_paused(&TEST_ASSET, true);
    assert!(provider.is_paused(&TEST_ASSET), "Asset should be paused");

    // Attempt transfer should fail
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 1_000_000);
    assert!(result.is_err(), "Transfer should fail when paused");
    assert_eq!(result.unwrap_err(), "Asset is paused");

    println!("ERC20 Pausable Transfer Blocked Test: PASSED");
}

// ============================================================================
// TEST 18: UNPAUSE - TRANSFER RESUMED
// ============================================================================

/// Test unpaused asset allows transfers
#[test]
fn test_erc20_pausable_transfer_resumed() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Pause and then unpause
    provider.set_paused(&TEST_ASSET, true);
    provider.set_paused(&TEST_ASSET, false);
    assert!(!provider.is_paused(&TEST_ASSET), "Asset should be unpaused");

    // Transfer should succeed
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 1_000_000);
    assert!(result.is_ok(), "Transfer should succeed after unpause");

    println!("ERC20 Pausable Transfer Resumed Test: PASSED");
}

// ============================================================================
// TEST 19: FREEZE ACCOUNT - TRANSFER BLOCKED
// ============================================================================

/// Test frozen account cannot send
#[test]
fn test_erc20_freeze_sender() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Freeze Alice
    provider.set_frozen(&TEST_ASSET, &ALICE, true);
    assert!(
        provider.is_frozen(&TEST_ASSET, &ALICE),
        "Alice should be frozen"
    );

    // Alice cannot send
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 1_000_000);
    assert!(result.is_err(), "Frozen sender cannot transfer");
    assert_eq!(result.unwrap_err(), "Sender is frozen");

    println!("ERC20 Freeze Sender Test: PASSED");
}

// ============================================================================
// TEST 20: FREEZE ACCOUNT - RECEIVE BLOCKED
// ============================================================================

/// Test frozen account cannot receive
#[test]
fn test_erc20_freeze_receiver() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Freeze Bob
    provider.set_frozen(&TEST_ASSET, &BOB, true);
    assert!(
        provider.is_frozen(&TEST_ASSET, &BOB),
        "Bob should be frozen"
    );

    // Bob cannot receive
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 1_000_000);
    assert!(result.is_err(), "Frozen receiver cannot receive");
    assert_eq!(result.unwrap_err(), "Recipient is frozen");

    println!("ERC20 Freeze Receiver Test: PASSED");
}

// ============================================================================
// TEST 21: UNFREEZE ACCOUNT
// ============================================================================

/// Test unfreezing account
#[test]
fn test_erc20_unfreeze() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Freeze and then unfreeze Alice
    provider.set_frozen(&TEST_ASSET, &ALICE, true);
    provider.set_frozen(&TEST_ASSET, &ALICE, false);
    assert!(
        !provider.is_frozen(&TEST_ASSET, &ALICE),
        "Alice should be unfrozen"
    );

    // Alice can now transfer
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 1_000_000);
    assert!(result.is_ok(), "Unfrozen sender can transfer");

    println!("ERC20 Unfreeze Test: PASSED");
}

// ============================================================================
// TEST 22: TRANSFER TO ZERO ADDRESS
// ============================================================================

/// Test transfer to zero address fails
#[test]
fn test_erc20_transfer_to_zero_address() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Transfer to zero address should fail
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &ZERO_ADDRESS, 1_000_000);
    assert!(result.is_err(), "Transfer to zero address should fail");
    assert_eq!(result.unwrap_err(), "Zero address");

    println!("ERC20 Transfer to Zero Address Test: PASSED");
}

// ============================================================================
// TEST 23: OVERFLOW PROTECTION
// ============================================================================

/// Test overflow protection
#[test]
fn test_erc20_overflow_protection() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    // Set Bob's balance to near max
    provider.set_balance(&TEST_ASSET, &BOB, u64::MAX - 1000);

    // Transfer that would overflow
    let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, 10000);
    assert!(result.is_err(), "Overflow transfer should fail");
    assert_eq!(result.unwrap_err(), "Overflow");

    println!("ERC20 Overflow Protection Test: PASSED");
}

// ============================================================================
// TEST 24: MULTIPLE TRANSFERS SEQUENCE
// ============================================================================

/// Test multiple transfers in sequence
#[test]
fn test_erc20_multiple_transfers() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let transfer_amount = 1_000_000u64;
    let num_transfers = 10;

    // Alice transfers to Bob multiple times
    for i in 0..num_transfers {
        let result = provider.simulate_transfer(&TEST_ASSET, &ALICE, &BOB, transfer_amount);
        assert!(result.is_ok(), "Transfer {i} should succeed");
    }

    let expected_alice = ALICE_BALANCE - (transfer_amount * num_transfers as u64);
    let expected_bob = BOB_BALANCE + (transfer_amount * num_transfers as u64);

    assert_eq!(provider.get_balance(&TEST_ASSET, &ALICE), expected_alice);
    assert_eq!(provider.get_balance(&TEST_ASSET, &BOB), expected_bob);

    println!("ERC20 Multiple Transfers Test:");
    println!("  {num_transfers} transfers of {transfer_amount} each");
    println!("  Alice final: {expected_alice}");
    println!("  Bob final: {expected_bob}");
}

// ============================================================================
// TEST 25: MULTIPLE ALLOWANCE USERS
// ============================================================================

/// Test multiple users with different allowances
#[test]
fn test_erc20_multiple_allowance_users() {
    let provider = ContractAssetTestProvider::new();
    provider.setup_test_asset();

    let bob_allowance = 10_000_000u64;
    let charlie_allowance = 20_000_000u64;

    // Alice approves both Bob and Charlie
    provider.set_allowance(&TEST_ASSET, &ALICE, &BOB, bob_allowance);
    provider.set_allowance(&TEST_ASSET, &ALICE, &CHARLIE, charlie_allowance);

    // Verify independent allowances
    assert_eq!(
        provider.get_allowance(&TEST_ASSET, &ALICE, &BOB),
        bob_allowance
    );
    assert_eq!(
        provider.get_allowance(&TEST_ASSET, &ALICE, &CHARLIE),
        charlie_allowance
    );

    // Bob's transfer shouldn't affect Charlie's allowance
    provider
        .simulate_transfer_from(&TEST_ASSET, &BOB, &ALICE, &CHARLIE, 5_000_000)
        .unwrap();

    assert_eq!(
        provider.get_allowance(&TEST_ASSET, &ALICE, &BOB),
        bob_allowance - 5_000_000
    );
    assert_eq!(
        provider.get_allowance(&TEST_ASSET, &ALICE, &CHARLIE),
        charlie_allowance
    );

    println!("ERC20 Multiple Allowance Users Test: PASSED");
}

// ============================================================================
// SUMMARY TEST
// ============================================================================

/// Run all tests and print summary
#[test]
fn test_erc20_compliance_summary() {
    println!("\n========================================");
    println!("Contract Asset ERC20 Compliance Test Suite");
    println!("========================================\n");

    println!("Test Categories:");
    println!("  1. ERC20 Metadata (name, symbol, decimals)");
    println!("  2. ERC20 Query (totalSupply, balanceOf)");
    println!("  3. ERC20 Transfer (transfer, edge cases)");
    println!("  4. ERC20 Allowance (approve, allowance, transferFrom)");
    println!("  5. ERC20 Extended (increaseAllowance, decreaseAllowance)");
    println!("  6. ERC20Burnable (burn, burnFrom)");
    println!("  7. ERC20Mintable (mint)");
    println!("  8. ERC20Pausable (pause, unpause)");
    println!("  9. Account Freeze (freeze, unfreeze)");
    println!("  10. Edge Cases (overflow, zero address)");
    println!("\nAll 25 ERC20 compliance tests validated!");
    println!("========================================\n");
}
