use lazy_static::lazy_static;
use tos_common::{
    api::daemon::{DevFeeThreshold, HardFork},
    block::BlockVersion,
    config::BYTES_PER_KB,
    crypto::{Address, Hash, PublicKey},
    difficulty::Difficulty,
    network::Network,
    static_assert,
    time::TimestampSeconds,
};

// In case of potential forks, have a unique network id to not connect to others compatible chains
pub const NETWORK_ID_SIZE: usize = 16;
pub const NETWORK_ID: [u8; NETWORK_ID_SIZE] = [
    0x74, 0x65, 0x72, 0x6d, 0x69, 0x6e, 0x6f, 0x73, 0x73, 0x6f, 0x6e, 0x69, 0x6d, 0x72, 0x65, 0x74,
];

// bind addresses
pub const DEFAULT_P2P_BIND_ADDRESS: &str = "0.0.0.0:2125";

// SECURITY FIX: Changed from 0.0.0.0 to 127.0.0.1 to prevent unauthorized remote access
// RPC endpoints include administrative functions (submit_block, mempool inspection, peer management)
// that should NOT be exposed to the network without authentication.
// To allow remote access, explicitly set --rpc-bind-address 0.0.0.0:8080 (not recommended without firewall)
pub const DEFAULT_RPC_BIND_ADDRESS: &str = "127.0.0.1:8080";

// Default cache size for storage DB
pub const DEFAULT_CACHE_SIZE: usize = 1024;

// Block rules
// Millis per second, it is used to prevent having random 1000 values anywhere
pub const MILLIS_PER_SECOND: u64 = 1000;

// Constants for hashrate
// Used for difficulty calculation
// and to be easier to read
pub const HASH: u64 = 1;
pub const KILO_HASH: u64 = HASH * 1000;
pub const MEGA_HASH: u64 = KILO_HASH * 1000;
pub const GIGA_HASH: u64 = MEGA_HASH * 1000;
pub const TERA_HASH: u64 = GIGA_HASH * 1000;

// Minimum difficulty is calculated the following (each difficulty point is in H/s)
// BLOCK TIME in millis * N = minimum hashrate
// Minimum hashrate for all networks
// Set to 200 H/s to achieve ~1 block per second target
// Single-threaded CPU hashrate: 100-300 H/s (avg ~200 H/s)
// With difficulty=200 and hashrate=200, average block time = 200/200 = 1 second
// DAA activates after 2016 blocks, so this value is critical for initial mining experience
// All networks use the same value for consistency between test and production
pub const MINIMUM_HASHRATE: u64 = 200 * HASH;

// This is also used as testnet and devnet minimum difficulty
// Set to 200 H/s to achieve ~1 block/second target from genesis
// Matches MINIMUM_HASHRATE (200 H/s) to ensure consistent block time before DAA activates
// DAA (Difficulty Adjustment Algorithm) only activates after 2016 blocks
pub const GENESIS_BLOCK_DIFFICULTY: Difficulty = Difficulty::from_u64(200);

// V-21 Fix: Increased from 500ms to 10 seconds (10000ms) to allow reasonable clock skew
// Previous value (500ms) was too restrictive and caused network synchronization issues
// Reference BlockDAG implementations use 132 seconds; 10 seconds provides good balance between security and reliability
pub const TIMESTAMP_IN_FUTURE_LIMIT: TimestampSeconds = 10_000;

// V-26 Fix: Maximum number of orphaned transactions to prevent unbounded memory growth
// Using LRU eviction, oldest transactions are dropped when limit is reached
pub const MAX_ORPHANED_TRANSACTIONS: usize = 10_000;

// -----------------------------------------------------------------------------
// Parallel Execution Configuration (runtime toggles)
// -----------------------------------------------------------------------------
//
// We purposefully default to DISABLED in production builds and enable via
// environment variables. This avoids accidentally turning on parallel execution
// where it hasn't been validated.
//
// Environment variables:
//   - TOS_PARALLEL_EXECUTION
//       "1" | "true"  => enabled
//       (unset/other) => disabled (default)
//
//   - TOS_PARALLEL_TEST_MODE
//       "1" | "true"  => enabled (runs extra parity checks)
//       (unset/other) => disabled (default)
//
// Rationale:
// - Safer default for mainnet.
// - Easy to turn on for dev/test environments without a rebuild.
use std::env;

lazy_static! {
    /// Runtime toggle for parallel execution (default: OFF for safety)
    /// Enable via: export TOS_PARALLEL_EXECUTION=1
    static ref PARALLEL_EXECUTION_ENABLED: bool = {
        match env::var("TOS_PARALLEL_EXECUTION") {
            Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "True"),
            Err(_) => false,  // Safe default: disabled
        }
    };

    /// Runtime toggle for parallel test mode (default: OFF)
    /// Enable via: export TOS_PARALLEL_TEST_MODE=1
    static ref PARALLEL_EXECUTION_TEST_MODE: bool = {
        match env::var("TOS_PARALLEL_TEST_MODE") {
            Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "True"),
            Err(_) => false,  // Safe default: disabled
        }
    };
}

/// Returns true if parallel execution is enabled at runtime.
/// Zero-overhead access: cached at first call, ~1-2ns per subsequent call.
pub fn parallel_execution_enabled() -> bool {
    *PARALLEL_EXECUTION_ENABLED
}

/// Returns true if parallel test-mode is enabled at runtime.
/// Zero-overhead access: cached at first call, ~1-2ns per subsequent call.
pub fn parallel_test_mode_enabled() -> bool {
    *PARALLEL_EXECUTION_TEST_MODE
}

// Minimum transactions required to trigger parallel execution (avoid overhead on small blocks)
// Network-specific thresholds for different use cases:
pub const MIN_TXS_FOR_PARALLEL_MAINNET: usize = 20; // Production: Higher threshold for proven performance
pub const MIN_TXS_FOR_PARALLEL_TESTNET: usize = 10; // Testing: Medium threshold for realistic testing
pub const MIN_TXS_FOR_PARALLEL_DEVNET: usize = 4; // Development: Lower threshold for easier testing

/// Get minimum transaction count for parallel execution based on network type
///
/// # Arguments
/// * `network` - The network type (Mainnet, Testnet, Devnet, Stagenet)
///
/// # Returns
/// Minimum number of transactions required to trigger parallel execution
pub fn get_min_txs_for_parallel(network: &Network) -> usize {
    match network {
        Network::Mainnet => MIN_TXS_FOR_PARALLEL_MAINNET,
        Network::Testnet => MIN_TXS_FOR_PARALLEL_TESTNET,
        Network::Devnet => MIN_TXS_FOR_PARALLEL_DEVNET,
        Network::Stagenet => MIN_TXS_FOR_PARALLEL_TESTNET, // Use testnet threshold for stagenet
    }
}

// keep at least last N blocks until top topoheight when pruning the chain
// WARNING: This must be at least 50 blocks for difficulty adjustement
pub const PRUNE_SAFETY_LIMIT: u64 = STABLE_LIMIT * 10;

// BlockDAG rules
// Balance stability limit - blocks required for transaction finality
// Used in deviation checks to prevent accepting blocks too far from consensus
//
// TOS: 60 blocks @ 1 BPS = 60 seconds (1 minute)
// This value provides fast finality while maintaining excellent network stability
// Tested: 128+ blocks with 100% acceptance rate, aligned with industry standards
pub const STABLE_LIMIT: u64 = 60;

// Emission rules
// 15% (6 months), 10% (6 months), 5% per block going to dev address
// NOTE: The explained emission above was the expected one
// But due to a bug in the function to calculate the dev fee reward,
// the actual emission was directly set to 10% per block
// New emission rules are: 10% during 1.5 years, then 5% for the rest
// This is the same for the project but reduce a bit the mining cost as they earn 5% more
pub const DEV_FEES: [DevFeeThreshold; 2] = [
    // Activated for 3M blocks
    DevFeeThreshold {
        height: 0,
        fee_percentage: 10,
    },
    // Activated for the rest
    DevFeeThreshold {
        // With 1s blocks, this triggers after ~6 months
        // 15 768 000 blocks * 1s block time / 60s / 60m / 24h / 365d ≈ 0.5 years
        // Note: TIP-1's 3s block proposal was deprecated, using 1s blocks
        height: 15_768_000,
        fee_percentage: 5,
    },
];
// only 30% of reward for side block
// This is to prevent spamming side blocks
// and also give rewards for miners with valid work on main chain
pub const SIDE_BLOCK_REWARD_PERCENT: u64 = 30;
// maximum 3 blocks for side block reward
// Each side block reward will be divided by the number of side blocks * 2
// With a configuration of 3 blocks, we have the following percents:
// 1 block: 30%
// 2 blocks: 15%
// 3 blocks: 7%
// 4 blocks: minimum percentage set below
pub const SIDE_BLOCK_REWARD_MAX_BLOCKS: u64 = 3;
// minimum 5% of block reward for side block
// This is the minimum given for all others valid side blocks
pub const SIDE_BLOCK_REWARD_MIN_PERCENT: u64 = 5;
// Emission speed factor for the emission curve
// It is used to calculate based on the supply the block reward
pub const EMISSION_SPEED_FACTOR: u64 = 20;

// Developer address for paying dev fees until Smart Contracts integration
// (testnet/mainnet format is converted lazily later)
pub const DEV_ADDRESS: &str = "tos1qsl6sj2u0gp37tr6drrq964rd4d8gnaxnezgytmt0cfltnp2wsgqqak28je";

// Chain sync config
// minimum X seconds between each chain sync request per peer
pub const CHAIN_SYNC_DELAY: u64 = 5;
// wait maximum between each chain sync request to peers
pub const CHAIN_SYNC_TIMEOUT_SECS: u64 = CHAIN_SYNC_DELAY * 3;
// first 30 blocks are sent in linear way, then it's exponential
pub const CHAIN_SYNC_REQUEST_EXPONENTIAL_INDEX_START: usize = 30;
// allows up to X blocks id (hash + height) sent for request
pub const CHAIN_SYNC_REQUEST_MAX_BLOCKS: usize = 64;
// minimum X blocks hashes sent for response
pub const CHAIN_SYNC_RESPONSE_MIN_BLOCKS: usize = 512;
// Default response blocks sent/accepted
pub const CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS: usize = 4096;
// allows up to X blocks hashes sent for response
pub const CHAIN_SYNC_RESPONSE_MAX_BLOCKS: usize = u16::MAX as _;
// send last 10 heights
pub const CHAIN_SYNC_TOP_BLOCKS: usize = 10;

// P2p rules
// time between each ping
pub const P2P_PING_DELAY: u64 = 10;
// time in seconds between each update of peerlist
pub const P2P_PING_PEER_LIST_DELAY: u64 = 60 * 5;
// maximum number of addresses to be send
pub const P2P_PING_PEER_LIST_LIMIT: usize = 16;
// default number of maximum peers
pub const P2P_DEFAULT_MAX_PEERS: usize = 32;
// default number of maximum outgoing peers
pub const P2P_DEFAULT_MAX_OUTGOING_PEERS: usize = 8;
// time in seconds between each time we try to connect to a new peer
pub const P2P_EXTEND_PEERLIST_DELAY: u64 = 60;
// time in seconds between each time we try to connect to a outgoing peer
// At least 5 minutes of countdown to retry to connect to the same peer
// This will be multiplied by the number of fails
pub const P2P_PEERLIST_RETRY_AFTER: u64 = 60 * 15;
// Delay in second to connect to priority nodes
pub const P2P_AUTO_CONNECT_PRIORITY_NODES_DELAY: u64 = 5;
// Default number of concurrent tasks for incoming p2p connections
pub const P2P_DEFAULT_CONCURRENCY_TASK_COUNT_LIMIT: usize = 4;
// Heartbeat interval in seconds to check if peer is still alive
pub const P2P_HEARTBEAT_INTERVAL: u64 = P2P_PING_DELAY / 2;
// Timeout in seconds
// If we didn't receive any packet from a peer during this time, we disconnect it
pub const P2P_PING_TIMEOUT: u64 = P2P_PING_DELAY * 6;

// Peer rules
// number of seconds to reset the counter
// Set to 30 minutes
pub const PEER_FAIL_TIME_RESET: u64 = 30 * 60;
// number of fail to disconnect the peer
pub const PEER_FAIL_LIMIT: u8 = 50;
// number of fail during handshake before temp ban
pub const PEER_FAIL_TO_CONNECT_LIMIT: u8 = 3;
// number of seconds to temp ban the peer in case of fail reached during handshake
// It is only used for incoming connections
// Set to 1 minute
pub const PEER_TEMP_BAN_TIME_ON_CONNECT: u64 = 60;
// number of seconds to temp ban the peer in case of fail count limit (`PEER_FAIL_LIMIT`) reached
// Set to 15 minutes
pub const PEER_TEMP_BAN_TIME: u64 = 15 * 60;
// millis until we timeout
pub const PEER_TIMEOUT_REQUEST_OBJECT: u64 = 15_000;
// How many objects requests can be concurrently requested?
pub const PEER_OBJECTS_CONCURRENCY: usize = 64;
// millis until we timeout during a bootstrap request
pub const PEER_TIMEOUT_BOOTSTRAP_STEP: u64 = 60_000;
// millis until we timeout during a handshake
pub const PEER_TIMEOUT_INIT_CONNECTION: u64 = 5_000;
// millis until we timeout during outgoing connection try
pub const PEER_TIMEOUT_INIT_OUTGOING_CONNECTION: u64 = 30_000;
// millis until we timeout during a handshake
pub const PEER_TIMEOUT_DISCONNECT: u64 = 1_500;
// Maximum packet size set to 5 MiB
pub const PEER_MAX_PACKET_SIZE: u32 = 5 * (BYTES_PER_KB * BYTES_PER_KB) as u32;
// Peer TX cache size
// This is how many elements are stored in the LRU cache at maximum
pub const PEER_TX_CACHE_SIZE: usize = 1024;
// How many peers propagated are stored per peer in the LRU cache at maximum
pub const PEER_PEERS_CACHE_SIZE: usize = 1024;
// Peer Block cache size
pub const PEER_BLOCK_CACHE_SIZE: usize = 1024;
// Peer packet channel size
pub const PEER_PACKET_CHANNEL_SIZE: usize = 1024;
// Peer timeout for packet channel
// Millis
pub const PEER_SEND_BYTES_TIMEOUT: u64 = 3_000;

// Compact Blocks Configuration
// Enable compact blocks for bandwidth-efficient block propagation (BIP-152)
// When enabled, blocks are transmitted as short transaction IDs + header (~1.3 KB)
// instead of full transactions (~50 KB), achieving 97.4% bandwidth reduction
pub const COMPACT_BLOCKS_ENABLED: bool = true;
// Compact block cache capacity (number of pending compact blocks)
pub const COMPACT_BLOCK_CACHE_CAPACITY: usize = 100;
// Compact block cache timeout in seconds (how long to wait for missing transactions)
pub const COMPACT_BLOCK_CACHE_TIMEOUT_SECS: u64 = 60;

// Hard Forks configured
const HARD_FORKS: [HardFork; 3] = [
    HardFork {
        height: 0,
        version: BlockVersion::V0,
        changelog: "Initial version",
        version_requirement: None,
    },
    HardFork {
        height: 0,
        version: BlockVersion::V1,
        changelog: "tos-hash v2",
        version_requirement: None,
    },
    HardFork {
        height: 0,
        version: BlockVersion::V2,
        changelog: "MultiSig, P2P",
        version_requirement: None,
    },
];

// Testnet / Stagenet / Devnet hard forks
// Note: V0 (60s blocks) removed from devnet - starts directly with V1 (1s blocks)
const OTHERS_NETWORK_HARD_FORKS: [HardFork; 3] = [
    HardFork {
        height: 0,
        version: BlockVersion::V1,
        changelog: "tos-hash v2, 1-second blocks",
        version_requirement: None,
    },
    HardFork {
        height: 10,
        version: BlockVersion::V2,
        changelog: "MultiSig, P2P",
        version_requirement: Some(">=1.16.0"),
    },
    HardFork {
        height: 15,
        version: BlockVersion::V3,
        changelog: "Smart Contracts",
        version_requirement: Some(">=1.16.0"),
    },
];

// Mainnet seed nodes
const MAINNET_SEED_NODES: [&str; 7] = [
    // France
    "51.210.117.23:2125",
    // US
    "198.71.55.87:2125",
    // Germany
    "162.19.249.100:2125",
    // Singapore
    "139.99.89.27:2125",
    // Poland
    "51.68.142.141:2125",
    // Great Britain
    "51.195.220.137:2125",
    // Canada
    "66.70.179.137:2125",
];

// Testnet seed nodes
const TESTNET_SEED_NODES: [&str; 1] = [
    // JP
    "157.7.65.157:2125",
];

// Genesis block to have the same starting point for every nodes
// Genesis block in hexadecimal format
const MAINNET_GENESIS_BLOCK: &str = "0200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000197ff69f0810000000000000000000000000000000000000000000000000000000000000000000000000000000000000000043fa8495c7a031f2c7a68c602eaa36d5a744fa69e44822f6b7e13f5cc2a7410000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const TESTNET_GENESIS_BLOCK: &str = "010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000018ae96229180000000000000000000000000000000000000000000000000000000000000000000000000000000000000000043fa8495c7a031f2c7a68c602eaa36d5a744fa69e44822f6b7e13f5cc2a7410000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

// Genesis block hash for both networks
// It must be the same as the hash of the genesis block
const MAINNET_GENESIS_BLOCK_HASH: Hash = Hash::new([
    125, 128, 19, 122, 147, 246, 47, 66, 57, 103, 85, 40, 152, 255, 81, 116, 36, 181, 100, 245,
    104, 139, 242, 106, 2, 115, 76, 58, 136, 231, 211, 171,
]);
const TESTNET_GENESIS_BLOCK_HASH: Hash = Hash::new([
    126, 75, 58, 199, 125, 227, 189, 220, 151, 221, 57, 169, 84, 28, 131, 20, 6, 150, 91, 156, 106,
    138, 89, 155, 242, 31, 88, 90, 130, 109, 114, 181,
]);

// Genesis block getter
// This is necessary to prevent having the same Genesis Block for differents network
// Dev returns none to generate a new genesis block each time it starts a chain
pub fn get_hex_genesis_block(network: &Network) -> Option<&str> {
    match network {
        Network::Mainnet => Some(MAINNET_GENESIS_BLOCK),
        Network::Testnet | Network::Stagenet => Some(TESTNET_GENESIS_BLOCK),
        Network::Devnet => None,
    }
}

lazy_static! {
    // Developer public key is lazily converted from address to support any network
    pub static ref DEV_PUBLIC_KEY: PublicKey = Address::from_string(&DEV_ADDRESS)
        .expect("valid dev address")
        .to_public_key();
}

// Genesis block hash based on network selected
pub fn get_genesis_block_hash(network: &Network) -> Option<&'static Hash> {
    match network {
        Network::Mainnet => Some(&MAINNET_GENESIS_BLOCK_HASH),
        Network::Testnet | Network::Stagenet => Some(&TESTNET_GENESIS_BLOCK_HASH),
        Network::Devnet => None,
    }
}

// Get seed nodes based on the network used
pub const fn get_seed_nodes(network: &Network) -> &[&str] {
    match network {
        Network::Mainnet => &MAINNET_SEED_NODES,
        Network::Testnet => &TESTNET_SEED_NODES,
        Network::Stagenet => &[],
        Network::Devnet => &[],
    }
}

// Get hard forks based on the network
pub const fn get_hard_forks(network: &Network) -> &[HardFork] {
    match network {
        Network::Mainnet => &HARD_FORKS,
        _ => &OTHERS_NETWORK_HARD_FORKS,
    }
}

// Static checks
static_assert!(
    CHAIN_SYNC_RESPONSE_MAX_BLOCKS >= CHAIN_SYNC_RESPONSE_MIN_BLOCKS,
    "Chain sync response max blocks must be greater than or equal to min blocks"
);
static_assert!(
    CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS >= CHAIN_SYNC_RESPONSE_MIN_BLOCKS,
    "Chain sync default response blocks must be greater than or equal to min blocks"
);
static_assert!(
    CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS <= CHAIN_SYNC_RESPONSE_MAX_BLOCKS,
    "Chain sync default response blocks must be less than or equal to max blocks"
);
static_assert!(
    CHAIN_SYNC_RESPONSE_MAX_BLOCKS <= u16::MAX as usize,
    "Chain sync response max blocks must be less than or equal to u16::MAX"
);
