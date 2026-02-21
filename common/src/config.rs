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
/// Fixed UNO fee per transfer (burned, not rewarded to miners)
pub const UNO_BURN_FEE_PER_TRANSFER: u64 = UNO_FEE_PER_TRANSFER;

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
// Minimum TOS amount for Shield operations (100 TOS) - anti-money-laundering measure
pub const MIN_SHIELD_TOS_AMOUNT: u64 = COIN_VALUE * 100;
/// Minimum arbiter stake (1000 TOS)
pub const MIN_ARBITER_STAKE: u64 = COIN_VALUE * 1000;
/// Juror submit window after coordinator deadline (in blocks)
pub const JUROR_SUBMIT_WINDOW: u64 = 86_400;
/// Max bytes for ArbitrationOpen payload (canonical JSON bytes)
pub const MAX_ARBITRATION_OPEN_BYTES: usize = 64 * 1024;
/// Max bytes for VoteRequest payload (canonical JSON bytes)
pub const MAX_VOTE_REQUEST_BYTES: usize = 64 * 1024;
/// Max bytes for SelectionCommitment payload
pub const MAX_SELECTION_COMMITMENT_BYTES: usize = 64 * 1024;
/// Max bytes for JurorVote payload (canonical JSON bytes)
pub const MAX_JUROR_VOTE_BYTES: usize = 8 * 1024;
/// Max bytes for VerdictBundle payload
pub const MAX_VERDICT_BUNDLE_BYTES: usize = 128 * 1024;

// Addresses format
// mainnet prefix address
pub const PREFIX_ADDRESS: &str = "tos";
// testnet prefix address
pub const TESTNET_PREFIX_ADDRESS: &str = "tst";

/// Mainnet bootstrap address (for backward compatibility).
/// Prefer `Network::bootstrap_address()` for network-aware code.
pub const BOOTSTRAP_ADDRESS: &str = crate::network::MAINNET_BOOTSTRAP_ADDRESS;

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

// ===== DoS PROTECTION LIMITS =====
// These constants form a unified defense against resource exhaustion attacks.
// All deserialization and allocation paths MUST check these limits BEFORE allocating memory.

/// Maximum transactions per block (derived from MAX_BLOCK_SIZE / minimum_tx_size)
/// Calculation: 1.25MB / ~100 bytes minimum tx ≈ 13,000 txs, using 10,000 as safe upper bound
pub const MAX_TXS_PER_BLOCK: u16 = 10_000;

/// Maximum referral upline traversal depth to prevent circular reference attacks
pub const MAX_UPLINE_LEVELS: u8 = 20;

// ===== KYC/APPROVAL TIMING CONSTANTS =====
// Centralized timing constants for KYC approval system to ensure consistency.

/// Approval expiry period: 24 hours in seconds
/// Approvals older than this are considered expired and rejected
pub const APPROVAL_EXPIRY_SECONDS: u64 = 24 * 3600;

/// Future timestamp tolerance: 1 hour in seconds
/// Approvals with timestamps more than this far in the future are rejected
/// This prevents timestamp manipulation attacks while allowing for clock drift
pub const APPROVAL_FUTURE_TOLERANCE_SECONDS: u64 = 3600;

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
