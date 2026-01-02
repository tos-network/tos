use crate::{contract::register_opaque_types, crypto::Hash, static_assert};

pub const VERSION: &str = env!("BUILD_VERSION");

// Native TOS asset (plaintext balances)
pub const TOS_ASSET: Hash = Hash::zero();

// UNO privacy asset (encrypted balances using Twisted ElGamal)
// Asset ID: 0x01 (distinct from TOS_ASSET which is 0x00)
pub const UNO_ASSET: Hash = Hash::new([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
]);

// ===== ENERGY STAKE 2.0 MODEL =====
//
// New proportional energy allocation model:
// - Energy = (frozen_balance / total_energy_weight) × TOTAL_ENERGY_LIMIT
// - 24-hour linear decay recovery
// - Free quota for casual users (~3 transfers/day)
// - 14-day unfreeze delay queue (max 32 entries)
// - Delegation support (DelegateResource / UndelegateResource)
//
// Philosophy: "Casual free, power users stake"
// - Casual users: ~3 free transfers/day
// - Power users: stake TOS for more energy
// - dApp users: pay for smart contract execution

// Total energy available network-wide
// 184M TOS × 100 = 18.4 billion energy (if all TOS staked)
pub const TOTAL_ENERGY_LIMIT: u64 = 18_400_000_000;

// Free energy quota per account per day
// Provides ~3 free transfers for casual users
// Transfer cost ≈ 500 energy → 1,500 ÷ 500 ≈ 3 transfers
pub const FREE_ENERGY_QUOTA: u64 = 1_500;

// Energy recovery window in milliseconds (24 hours)
// Energy linearly recovers over this period after consumption
pub const ENERGY_RECOVERY_WINDOW_MS: u64 = 86_400_000;

// Maximum entries in unfreezing queue per account
// Prevents unbounded storage growth
pub const MAX_UNFREEZING_LIST_SIZE: usize = 32;

// Delay in days before unfrozen TOS can be withdrawn
pub const UNFREEZE_DELAY_DAYS: u32 = 14;

// Auto-burn rate: atomic TOS units per energy unit
// When frozen energy insufficient, TOS is burned at this rate
// 100 atomic = 0.000001 TOS per energy
pub const TOS_PER_ENERGY: u64 = 100;

// Minimum TOS amount for delegation (1 TOS)
pub const MIN_DELEGATION_AMOUNT: u64 = COIN_VALUE;

// Maximum lock period for delegated resources (days)
pub const MAX_DELEGATE_LOCK_DAYS: u32 = 365;

// Default lock period for delegated resources (days)
// 0 = no lock (can undelegate immediately after 3 days minimum)
pub const DEFAULT_DELEGATE_LOCK_DAYS: u32 = 3;

// Energy costs for different transaction types (Stake 2.0)
pub const ENERGY_COST_TRANSFER_BASE: u64 = 0; // Base cost for transfer (size-based)
pub const ENERGY_COST_TRANSFER_PER_OUTPUT: u64 = 100; // Per transfer output
pub const ENERGY_COST_NEW_ACCOUNT: u64 = 25_000; // Creating new account
pub const ENERGY_COST_BURN: u64 = 1_000; // Burn operation
pub const ENERGY_COST_CONTRACT_DEPLOY_BASE: u64 = 32_000; // Base cost for contract deploy
pub const ENERGY_COST_CONTRACT_DEPLOY_PER_BYTE: u64 = 10; // Per byte of bytecode

// TOS-based fee model constants
pub const FEE_PER_KB: u64 = 10000;
pub const FEE_PER_TRANSFER: u64 = 5000;

// ===== TOS-Only Fees (cannot use Energy) =====
// These fees must be paid in TOS, not Energy. Aligned with TRON mainnet.

/// Account creation fee - deducted from transfer amount when sending to new account
/// 0.1 TOS = 10,000,000 atomic units
/// This prevents Sybil attacks (mass account creation)
pub const FEE_PER_ACCOUNT_CREATION: u64 = 10_000_000;

/// MultiSig transaction fee - per signature, deducted from sender's TOS balance
/// 1 TOS = 100,000,000 atomic units per signature
/// Only charged when transaction has 2+ signatures
/// Aligned with TRON's MULTI_SIGN_FEE (1 TRX/signature)
pub const FEE_PER_MULTISIG_SIGNATURE: u64 = COIN_VALUE;

// Note: UNO transfers use Energy model with 5x multiplier (see EnergyFeeCalculator)
// Account creation fee for UNO is the same as TOS (FEE_PER_ACCOUNT_CREATION)

// Contracts rules
// 1 TOS per contract deployed
// Each contract deployed has a overhead of 1 TOS
// This amount is burned and is needed for safety of the chain
// Otherwise people could bloat the chain by deploying contracts
// And could make the chain unusable or slow
// Note that if we depends on fees only, miners could do such attacks for free
// by mining their own transactions and getting the fees back
pub const BURN_PER_CONTRACT: u64 = COIN_VALUE;
// 1 TOS per token created
// This is to prevent spamming the network with tokens
pub const COST_PER_TOKEN: u64 = COIN_VALUE;
// 30% of the transaction fee is burned
// This is to reduce the supply over time
// and also to prevent spamming the network with low fee transactions
// or free tx from miners
// This should be enabled once Smart Contracts are released
pub const TX_GAS_BURN_PERCENT: u64 = 30;
// Fee per store operation in a contract
// Each store operation has a fixed cost of 0.000001 TOS
pub const FEE_PER_STORE_CONTRACT: u64 = 100;
// Fee per byte of data stored in a contract
// Each byte of data stored (key + value) in a contract has a fixed cost
// 0.00000005 TOS per byte
pub const FEE_PER_BYTE_STORED_CONTRACT: u64 = 5;
// Fee per byte of data stored in a contract memory
// Each byte of data stored in the contract memory has a fixed cost
pub const FEE_PER_BYTE_IN_CONTRACT_MEMORY: u64 = 1;
// Fee per byte of data used to emit an event
// Data is not stored, but only exposed to websocket listeners
pub const FEE_PER_BYTE_OF_EVENT_DATA: u64 = 2;
// Max gas usage available per block
// Currently, set to 10 TOS per transaction
pub const MAX_GAS_USAGE_PER_TX: u64 = COIN_VALUE * 10;

// 8 decimals numbers
pub const COIN_DECIMALS: u8 = 8;
// 100 000 000 to represent 1 TOS
pub const COIN_VALUE: u64 = 10u64.pow(COIN_DECIMALS as u32);
// 184M full coin
pub const MAXIMUM_SUPPLY: u64 = 184_000_000 * COIN_VALUE;

// ===== TOS AMOUNT LIMITS =====
// Minimum TOS amount for freeze operations (1 TOS)
pub const MIN_FREEZE_TOS_AMOUNT: u64 = COIN_VALUE;
// Minimum TOS amount for unfreeze operations (1 TOS)
pub const MIN_UNFREEZE_TOS_AMOUNT: u64 = COIN_VALUE;

// Addresses format
// mainnet prefix address
pub const PREFIX_ADDRESS: &str = "tos";
// testnet prefix address
pub const TESTNET_PREFIX_ADDRESS: &str = "tst";

// Bootstrap address for creating Global Committee
// This is the same as DEV_ADDRESS - only the developer can bootstrap the global committee
// SECURITY: This restricts BootstrapCommittee transactions to a known, trusted address
pub const BOOTSTRAP_ADDRESS: &str =
    "tos1qsl6sj2u0gp37tr6drrq964rd4d8gnaxnezgytmt0cfltnp2wsgqqak28je";

// Proof prefix
pub const PREFIX_PROOF: &str = "proof";

// 1 KB = 1024 bytes
pub const BYTES_PER_KB: usize = 1024;

// Max transaction size in bytes
pub const MAX_TRANSACTION_SIZE: usize = BYTES_PER_KB * BYTES_PER_KB; // 1 MB

// Max block size in bytes
// 1024 * 1024 + (256 * 1024) bytes = 1.25 MB maximum size per block with txs
pub const MAX_BLOCK_SIZE: usize = (BYTES_PER_KB * BYTES_PER_KB) + (256 * BYTES_PER_KB);

// BlockDAG rules
pub const TIPS_LIMIT: usize = 3; // maximum 3 TIPS per block

// Initialize the configuration
pub fn init() {
    // register the opaque types
    register_opaque_types();
}

// Static checks
static_assert!(
    MAX_TRANSACTION_SIZE <= MAX_BLOCK_SIZE,
    "Max transaction size must be less than or equal to max block size"
);
static_assert!(
    MAXIMUM_SUPPLY >= COIN_VALUE,
    "Maximum supply must be greater than or equal to coin value"
);
