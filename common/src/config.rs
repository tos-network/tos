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

// ===== NEW ENERGY-BASED FEE MODEL =====

// Energy-based fee model constants
// Only transfer operations consume energy
// Simplified model compared to TRON: 1 transfer = 1 energy (size-independent)
//
// Energy Model Overview:
// - Each transfer consumes exactly 1 energy regardless of transaction size
// - Energy is gained by freezing TOS with duration-based multipliers
// - Energy formula: 1 TOS × (2 × freeze_days) = energy units
// - Example: 1 TOS frozen for 7 days = 14 energy = 14 free transfers
pub const ENERGY_PER_TRANSFER: u64 = 1; // Basic transfer (1 energy per transfer)

// TOS-based fee model constants
pub const FEE_PER_KB: u64 = 10000;
pub const FEE_PER_ACCOUNT_CREATION: u64 = 0; // Free account creation (no fee)
pub const FEE_PER_TRANSFER: u64 = 5000;
pub const FEE_PER_MULTISIG_SIGNATURE: u64 = 500;

// UNO-based fee model constants (privacy transfers)
pub const UNO_FEE_PER_KB: u64 = 10000;
pub const UNO_FEE_PER_ACCOUNT_CREATION: u64 = 0; // Free account creation (no fee)
pub const UNO_FEE_PER_TRANSFER: u64 = 5000;
pub const UNO_FEE_PER_MULTISIG_SIGNATURE: u64 = 500;

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

// ===== FREEZE DURATION LIMITS =====
// Minimum freeze duration in days
pub const MIN_FREEZE_DURATION_DAYS: u32 = 3;
// Maximum freeze duration in days
pub const MAX_FREEZE_DURATION_DAYS: u32 = 180;

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
