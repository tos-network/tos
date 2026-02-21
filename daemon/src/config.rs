use std::num::NonZeroUsize;
#[cfg(any(test, feature = "testutils"))]
use std::sync::RwLock;

use lazy_static::lazy_static;
use tos_common::{
    api::daemon::{ChainTips, DevFeeThreshold, ForkCondition, HardFork, TosHardfork},
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

// Compile-time NonZeroUsize constant for LruCache initialization
// SAFETY: This constant is compile-time validated to be non-zero
pub const DEFAULT_CACHE_SIZE_NONZERO: NonZeroUsize = unsafe {
    // SAFETY: DEFAULT_CACHE_SIZE is a compile-time constant set to 1024 (non-zero)
    NonZeroUsize::new_unchecked(DEFAULT_CACHE_SIZE)
};

// V-26 Fix: Maximum number of orphaned transactions to prevent unbounded memory growth
// Using LRU eviction, oldest transactions are dropped when limit is reached
pub const MAX_ORPHANED_TRANSACTIONS: usize = 10_000;

// Compile-time NonZeroUsize constant for orphaned transactions LruCache
// SAFETY: This constant is compile-time validated to be non-zero
pub const MAX_ORPHANED_TRANSACTIONS_NONZERO: NonZeroUsize = unsafe {
    // SAFETY: MAX_ORPHANED_TRANSACTIONS is a compile-time constant set to 10,000 (non-zero)
    NonZeroUsize::new_unchecked(MAX_ORPHANED_TRANSACTIONS)
};

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
// This is to prevent spamming the network with low difficulty blocks
// Formula: MINIMUM_HASHRATE * BLOCK_TIME_TARGET / MILLIS_PER_SECOND
//
// Mainnet minimum hashrate: 100 KH/s
// This provides initial difficulty of 100,000 (100,000 * 1000 / 1000)
// Prevents block spam at chain launch while allowing difficulty to adjust down if needed
pub const MAINNET_MINIMUM_HASHRATE: u64 = 100 * KILO_HASH;

// Testnet minimum hashrate: 10 KH/s
// This provides initial difficulty of 10,000 (10,000 * 1000 / 1000)
// Balanced for testing with multiple miners
pub const TESTNET_MINIMUM_HASHRATE: u64 = 10 * KILO_HASH;

// Devnet minimum hashrate: 1 KH/s
// This provides initial difficulty of 1,000 (1,000 * 1000 / 1000)
// Low enough for single developer testing
pub const DEVNET_MINIMUM_HASHRATE: u64 = 1 * KILO_HASH;

// Genesis block difficulty - used for cumulative difficulty initialization
// Set to mainnet minimum difficulty for consistency
// Formula: MAINNET_MINIMUM_HASHRATE * BLOCK_TIME_TARGET / MILLIS_PER_SECOND
// Current: 100 KH/s * 1000ms / 1000 = 100,000
pub const GENESIS_BLOCK_DIFFICULTY: Difficulty = Difficulty::from_u64(100_000);

// 2 seconds maximum in future (prevent any attack on reducing difficulty but keep margin for unsynced devices)
pub const TIMESTAMP_IN_FUTURE_LIMIT: TimestampSeconds = 2 * MILLIS_PER_SECOND;

// keep at least last N blocks until top topoheight when pruning the chain
// WARNING: This must be at least 50 blocks for difficulty adjustement
pub const PRUNE_SAFETY_LIMIT: u64 = STABLE_LIMIT * 10;

// BlockDAG rules
// in how many height we consider the block stable
pub const STABLE_LIMIT: u64 = 24;

pub const fn get_stable_limit(version: BlockVersion) -> u64 {
    match version {
        BlockVersion::Nobunaga => STABLE_LIMIT,
    }
}

// Blocks propagation queue capacity: STABLE_LIMIT * TIPS_LIMIT = 24 * 3 = 72
pub const BLOCKS_PROPAGATION_CAPACITY: usize =
    STABLE_LIMIT as usize * tos_common::config::TIPS_LIMIT;

// SAFETY: BLOCKS_PROPAGATION_CAPACITY is computed from non-zero constants (24 * 3 = 72)
pub const BLOCKS_PROPAGATION_CAPACITY_NONZERO: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(BLOCKS_PROPAGATION_CAPACITY) };

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
        // TIP-1: With 3s blocks, this triggers after ~1.5 years
        // 15 768 000 blocks * 3s block time / 60s / 60m / 24h / 365d = 1.5 years
        // Note: Old comment referenced 12s blocks (3_942_000 blocks)
        // New calculation: 3_942_000 * (12/3) = 15_768_000 blocks for same duration
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

/// Mainnet developer address (for backward compatibility).
/// Prefer `get_dev_public_key(network)` for network-aware code.
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

// Compile-time NonZeroUsize constants for peer LruCache initialization
// SAFETY: All peer cache/concurrency constants are compile-time set to non-zero values
pub const PEER_OBJECTS_CONCURRENCY_NONZERO: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(PEER_OBJECTS_CONCURRENCY) };
pub const PEER_TX_CACHE_SIZE_NONZERO: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(PEER_TX_CACHE_SIZE) };
pub const PEER_PEERS_CACHE_SIZE_NONZERO: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(PEER_PEERS_CACHE_SIZE) };
pub const PEER_BLOCK_CACHE_SIZE_NONZERO: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(PEER_BLOCK_CACHE_SIZE) };

// Peer packet channel size
pub const PEER_PACKET_CHANNEL_SIZE: usize = 1024;
// Peer timeout for packet channel
// Millis
pub const PEER_SEND_BYTES_TIMEOUT: u64 = 3_000;

// Hard Forks configured - Append-Only Architecture
// TOS uses Sengoku warlord naming (see version.md for v0-v61)
// v0: Nobunaga, v1: Nohime, v2: Oichi, v3: Mitsuhide, ...
//
// ForkCondition types (following Ethereum/reth design):
// - Block(height): Activate at specific block height (deterministic)
// - Timestamp(ms): Activate at Unix timestamp in milliseconds (time-based)
// - TCD(difficulty): Activate when cumulative difficulty reaches threshold (hashrate-based)
// - Never: Disabled feature (testnet-only or deprecated)
const HARD_FORKS: [HardFork; 1] = [
    HardFork {
        condition: ForkCondition::Block(0),
        version: BlockVersion::Nobunaga,
        changelog: "Nobunaga - Genesis with PoW V2, 3s blocks",
        version_requirement: None,
    },
    // === Future Hard Forks (Append-Only, see version.md) ===
    // Example: Block-based activation
    // HardFork {
    //     condition: ForkCondition::Block(1_000_000),
    //     version: BlockVersion::Nohime,  // v1
    //     changelog: "Nohime - ...",
    //     version_requirement: Some(">=0.2.0"),
    // },
    // Example: Timestamp-based activation (2026-01-01 00:00:00 UTC)
    // HardFork {
    //     condition: ForkCondition::Timestamp(1767225600000),
    //     version: BlockVersion::Oichi,  // v2
    //     changelog: "Oichi - ...",
    //     version_requirement: Some(">=0.3.0"),
    // },
    // Example: TCD-based activation (hashrate-dependent)
    // HardFork {
    //     condition: ForkCondition::TCD(1_000_000_000),
    //     version: BlockVersion::Mitsuhide,  // v3
    //     changelog: "Mitsuhide - ...",
    //     version_requirement: Some(">=0.4.0"),
    // },
];

// ============================================================================
// TIP (TOS Improvement Proposal) Configurations - Independent Activation
// ============================================================================
// Each TIP can be independently activated using ForkCondition.
// This follows Ethereum's EIP activation model.

lazy_static! {
    /// Mainnet TIP activations
    static ref MAINNET_TIPS: ChainTips = ChainTips::new(vec![
        // TIP-100 (SmartContracts):
        // Mainnet is intentionally disabled at this stage.
        // Purpose: keep mainnet behavior stable while allowing testnet/devnet integration.
        // Avatar clients should gate "deploy/invoke contract" entry by network/TIP activation.
        (TosHardfork::SmartContracts, ForkCondition::Never),
    ]);

    /// Testnet TIP activations
    static ref TESTNET_TIPS: ChainTips = ChainTips::new(vec![
        // TIP-100 enabled from genesis for client and protocol validation.
        (TosHardfork::SmartContracts, ForkCondition::Block(0)),
    ]);

    /// Devnet TIP activations
    static ref DEVNET_TIPS: ChainTips = ChainTips::new(vec![
        // TIP-100 enabled from genesis for local development and debugging.
        (TosHardfork::SmartContracts, ForkCondition::Block(0)),
    ]);

}

// Test-only override for hard fork config
#[cfg(any(test, feature = "testutils"))]
lazy_static! {
    static ref HARD_FORKS_OVERRIDE: RwLock<Option<&'static [HardFork]>> = RwLock::new(None);
}

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

// Discovery bootnodes (discv6 UDP)
// These nodes handle peer discovery only, not block/transaction sync
// Format: tosnode://<node_id>@<host>:<port> or just <host>:<port>

// Mainnet discovery bootnodes
const MAINNET_DISCOVERY_BOOTNODES: [&str; 0] = [
    // TODO: Add mainnet bootnodes when deployed
];

// Testnet discovery bootnodes
const TESTNET_DISCOVERY_BOOTNODES: [&str; 0] = [
    // TODO: Add testnet bootnodes when deployed
];

// Genesis block to have the same starting point for every nodes
// Genesis block in hexadecimal format (Nobunaga version with VRF flag)
// Format: header fields + VRF flag (00 = no VRF)
const MAINNET_GENESIS_BLOCK: &str = "0000000000000000000000019ca6b1dc0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000043fa8495c7a031f2c7a68c602eaa36d5a744fa69e44822f6b7e13f5cc2a741000";
const TESTNET_GENESIS_BLOCK: &str = "000000000000000000000001941f297c0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000043fa8495c7a031f2c7a68c602eaa36d5a744fa69e44822f6b7e13f5cc2a741000";

// Genesis block hash for both networks (Nobunaga version)
// It must be the same as the hash of the genesis block
const MAINNET_GENESIS_BLOCK_HASH: Hash = Hash::new([
    221, 209, 7, 100, 97, 8, 80, 224, 81, 145, 186, 236, 51, 103, 32, 182, 9, 114, 45, 183, 90,
    196, 73, 13, 174, 183, 58, 248, 71, 42, 158, 163,
]);
const TESTNET_GENESIS_BLOCK_HASH: Hash = Hash::new([
    95, 233, 222, 20, 50, 56, 148, 22, 57, 190, 21, 243, 199, 236, 8, 193, 202, 181, 217, 156, 219,
    209, 131, 249, 135, 144, 71, 132, 31, 10, 94, 10,
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

// Developer public key is lazily converted from address to support any network
// SAFE: DEV_ADDRESS is a compile-time constant with a known valid format
// Panic on invalid address is intentional - daemon cannot start with invalid dev address
#[allow(clippy::expect_used, clippy::panic)]
fn create_dev_public_key() -> PublicKey {
    match Address::from_string(&DEV_ADDRESS) {
        Ok(address) => address.to_public_key(),
        Err(err) => {
            panic!(
                "FATAL: Invalid DEV_ADDRESS '{}': {} - daemon cannot start with invalid developer address",
                DEV_ADDRESS, err
            );
        }
    }
}

lazy_static! {
    pub static ref DEV_PUBLIC_KEY: PublicKey = create_dev_public_key();
}

/// Get the developer public key for a given network.
/// Panics on invalid address - daemon cannot start with invalid dev address.
#[allow(clippy::expect_used, clippy::panic)]
pub fn get_dev_public_key(network: &Network) -> PublicKey {
    let addr_str = network.dev_address();
    match Address::from_string(addr_str) {
        Ok(address) => address.to_public_key(),
        Err(err) => {
            panic!(
                "FATAL: Invalid dev address '{}': {} - daemon cannot start with invalid developer address",
                addr_str, err
            );
        }
    }
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

// Get discovery bootnodes based on the network used
// These are UDP-only nodes for discv6 peer discovery
pub const fn get_discovery_bootnodes(network: &Network) -> &[&str] {
    match network {
        Network::Mainnet => &MAINNET_DISCOVERY_BOOTNODES,
        Network::Testnet => &TESTNET_DISCOVERY_BOOTNODES,
        Network::Stagenet => &[],
        Network::Devnet => &[],
    }
}

// Get hard forks based on the network
// All networks use the same hard fork configuration (Nobunaga genesis)
#[cfg(not(any(test, feature = "testutils")))]
pub fn get_hard_forks(_network: &Network) -> &'static [HardFork] {
    &HARD_FORKS
}

// Get hard forks based on the network (test version with override support)
#[cfg(any(test, feature = "testutils"))]
pub fn get_hard_forks(_network: &Network) -> &'static [HardFork] {
    if let Ok(guard) = HARD_FORKS_OVERRIDE.read() {
        if let Some(override_forks) = *guard {
            return override_forks;
        }
    }
    &HARD_FORKS
}

/// Override hard fork configuration for tests (leaks the slice for static lifetime).
/// Only available in test builds.
#[cfg(any(test, feature = "testutils"))]
pub fn set_hard_forks_override_for_tests(forks: Vec<HardFork>) {
    let leaked: &'static [HardFork] = Box::leak(forks.into_boxed_slice());
    if let Ok(mut guard) = HARD_FORKS_OVERRIDE.write() {
        *guard = Some(leaked);
    }
}

/// Clear hard fork override (restore default config).
/// Only available in test builds.
#[cfg(any(test, feature = "testutils"))]
pub fn clear_hard_forks_override_for_tests() {
    if let Ok(mut guard) = HARD_FORKS_OVERRIDE.write() {
        *guard = None;
    }
}

// Get TIP (TOS Improvement Proposal) activations based on the network
// Each network can have different TIP activation schedules
pub fn get_chain_tips(network: &Network) -> &'static ChainTips {
    match network {
        Network::Mainnet => &MAINNET_TIPS,
        Network::Testnet | Network::Stagenet => &TESTNET_TIPS,
        Network::Devnet => &DEVNET_TIPS,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::{
        block::Block,
        crypto::{Address, Hashable},
        serializer::{Reader, Serializer},
    };

    /// Expected developer address for genesis blocks
    const EXPECTED_DEV_ADDRESS: &str =
        "tos1qsl6sj2u0gp37tr6drrq964rd4d8gnaxnezgytmt0cfltnp2wsgqqak28je";

    /// Expected mainnet timestamp: 2026-03-01 00:00:00 UTC
    const EXPECTED_MAINNET_TIMESTAMP: u64 = 1772323200000;

    /// Expected testnet timestamp: 2025-01-01 00:00:00 UTC
    const EXPECTED_TESTNET_TIMESTAMP: u64 = 1735689600000;

    /// Test that MAINNET_GENESIS_BLOCK can be deserialized and all fields match expected values
    #[test]
    fn test_mainnet_genesis_block_integrity() {
        // Decode the hex string to bytes
        let genesis_bytes =
            hex::decode(MAINNET_GENESIS_BLOCK).expect("MAINNET_GENESIS_BLOCK should be valid hex");

        // Deserialize the block
        let mut reader = Reader::new(&genesis_bytes);
        let block = Block::read(&mut reader)
            .expect("MAINNET_GENESIS_BLOCK should deserialize to a valid Block");

        // === Verify Block Version ===
        assert_eq!(
            block.get_header().get_version(),
            BlockVersion::Nobunaga,
            "Mainnet genesis block should have Nobunaga version"
        );

        // === Verify Block Height ===
        assert_eq!(
            block.get_height(),
            0,
            "Mainnet genesis block height should be 0"
        );

        // === Verify Timestamp ===
        assert_eq!(
            block.get_timestamp(),
            EXPECTED_MAINNET_TIMESTAMP,
            "Mainnet genesis block timestamp should be 2026-03-01 00:00:00 UTC ({})",
            EXPECTED_MAINNET_TIMESTAMP
        );

        // === Verify Miner (Developer Public Key) ===
        let expected_dev_pubkey = Address::from_string(EXPECTED_DEV_ADDRESS)
            .expect("Developer address should be valid")
            .to_public_key();
        assert_eq!(
            block.get_miner(),
            &expected_dev_pubkey,
            "Mainnet genesis block miner should be developer public key"
        );

        // === Verify Tips (should be empty for genesis) ===
        assert!(
            block.get_tips().is_empty(),
            "Mainnet genesis block should have no parent tips"
        );

        // === Verify Transactions (should be empty for genesis) ===
        assert!(
            block.get_transactions().is_empty(),
            "Mainnet genesis block should have no transactions"
        );

        // === Verify Extra Nonce (should be all zeros) ===
        let extra_nonce = block.get_extra_nonce();
        assert_eq!(extra_nonce.len(), 32, "Extra nonce should be 32 bytes");
        assert!(
            extra_nonce.iter().all(|&b| b == 0),
            "Mainnet genesis block extra_nonce should be all zeros"
        );

        // === Verify Hash ===
        let computed_hash = block.get_header().hash();
        assert_eq!(
            computed_hash, MAINNET_GENESIS_BLOCK_HASH,
            "Computed mainnet genesis block hash should match MAINNET_GENESIS_BLOCK_HASH"
        );

        // === Verify Round-trip Serialization ===
        let reserialized_hex = block.to_hex();
        assert_eq!(
            reserialized_hex, MAINNET_GENESIS_BLOCK,
            "Re-serialized mainnet genesis block should match original hex"
        );
    }

    /// Test that TESTNET_GENESIS_BLOCK can be deserialized and all fields match expected values
    #[test]
    fn test_testnet_genesis_block_integrity() {
        // Decode the hex string to bytes
        let genesis_bytes =
            hex::decode(TESTNET_GENESIS_BLOCK).expect("TESTNET_GENESIS_BLOCK should be valid hex");

        // Deserialize the block
        let mut reader = Reader::new(&genesis_bytes);
        let block = Block::read(&mut reader)
            .expect("TESTNET_GENESIS_BLOCK should deserialize to a valid Block");

        // === Verify Block Version ===
        assert_eq!(
            block.get_header().get_version(),
            BlockVersion::Nobunaga,
            "Testnet genesis block should have Nobunaga version"
        );

        // === Verify Block Height ===
        assert_eq!(
            block.get_height(),
            0,
            "Testnet genesis block height should be 0"
        );

        // === Verify Timestamp ===
        assert_eq!(
            block.get_timestamp(),
            EXPECTED_TESTNET_TIMESTAMP,
            "Testnet genesis block timestamp should be 2025-01-01 00:00:00 UTC ({})",
            EXPECTED_TESTNET_TIMESTAMP
        );

        // === Verify Miner (Developer Public Key) ===
        let expected_dev_pubkey = Address::from_string(EXPECTED_DEV_ADDRESS)
            .expect("Developer address should be valid")
            .to_public_key();
        assert_eq!(
            block.get_miner(),
            &expected_dev_pubkey,
            "Testnet genesis block miner should be developer public key"
        );

        // === Verify Tips (should be empty for genesis) ===
        assert!(
            block.get_tips().is_empty(),
            "Testnet genesis block should have no parent tips"
        );

        // === Verify Transactions (should be empty for genesis) ===
        assert!(
            block.get_transactions().is_empty(),
            "Testnet genesis block should have no transactions"
        );

        // === Verify Extra Nonce (should be all zeros) ===
        let extra_nonce = block.get_extra_nonce();
        assert_eq!(extra_nonce.len(), 32, "Extra nonce should be 32 bytes");
        assert!(
            extra_nonce.iter().all(|&b| b == 0),
            "Testnet genesis block extra_nonce should be all zeros"
        );

        // === Verify Hash ===
        let computed_hash = block.get_header().hash();
        assert_eq!(
            computed_hash, TESTNET_GENESIS_BLOCK_HASH,
            "Computed testnet genesis block hash should match TESTNET_GENESIS_BLOCK_HASH"
        );

        // === Verify Round-trip Serialization ===
        let reserialized_hex = block.to_hex();
        assert_eq!(
            reserialized_hex, TESTNET_GENESIS_BLOCK,
            "Re-serialized testnet genesis block should match original hex"
        );
    }

    /// Test that mainnet and testnet only differ in timestamp (all other fields should be same)
    #[test]
    fn test_genesis_blocks_field_comparison() {
        // Deserialize both blocks
        let mainnet_bytes =
            hex::decode(MAINNET_GENESIS_BLOCK).expect("MAINNET_GENESIS_BLOCK should be valid hex");
        let testnet_bytes =
            hex::decode(TESTNET_GENESIS_BLOCK).expect("TESTNET_GENESIS_BLOCK should be valid hex");

        let mut mainnet_reader = Reader::new(&mainnet_bytes);
        let mut testnet_reader = Reader::new(&testnet_bytes);

        let mainnet_block =
            Block::read(&mut mainnet_reader).expect("MAINNET_GENESIS_BLOCK should deserialize");
        let testnet_block =
            Block::read(&mut testnet_reader).expect("TESTNET_GENESIS_BLOCK should deserialize");

        // === Fields that should be SAME ===
        assert_eq!(
            mainnet_block.get_header().get_version(),
            testnet_block.get_header().get_version(),
            "Both genesis blocks should have same version (Nobunaga)"
        );
        assert_eq!(
            mainnet_block.get_height(),
            testnet_block.get_height(),
            "Both genesis blocks should have same height (0)"
        );
        assert_eq!(
            mainnet_block.get_miner(),
            testnet_block.get_miner(),
            "Both genesis blocks should have same miner (developer key)"
        );
        assert_eq!(
            mainnet_block.get_tips().len(),
            testnet_block.get_tips().len(),
            "Both genesis blocks should have same number of tips (0)"
        );
        assert_eq!(
            mainnet_block.get_transactions().len(),
            testnet_block.get_transactions().len(),
            "Both genesis blocks should have same number of transactions (0)"
        );
        assert_eq!(
            mainnet_block.get_extra_nonce(),
            testnet_block.get_extra_nonce(),
            "Both genesis blocks should have same extra_nonce (all zeros)"
        );

        // === Fields that should be DIFFERENT ===
        assert_ne!(
            mainnet_block.get_timestamp(),
            testnet_block.get_timestamp(),
            "Mainnet and testnet should have different timestamps"
        );
        assert_ne!(
            mainnet_block.hash(),
            testnet_block.hash(),
            "Mainnet and testnet should have different hashes"
        );

        // === Verify exact timestamp difference ===
        assert_eq!(
            mainnet_block.get_timestamp(),
            EXPECTED_MAINNET_TIMESTAMP,
            "Mainnet timestamp should be 2026-03-01"
        );
        assert_eq!(
            testnet_block.get_timestamp(),
            EXPECTED_TESTNET_TIMESTAMP,
            "Testnet timestamp should be 2025-01-01"
        );
    }

    /// Test that get_hex_genesis_block returns correct values for each network
    #[test]
    fn test_get_hex_genesis_block() {
        assert_eq!(
            get_hex_genesis_block(&Network::Mainnet),
            Some(MAINNET_GENESIS_BLOCK)
        );
        assert_eq!(
            get_hex_genesis_block(&Network::Testnet),
            Some(TESTNET_GENESIS_BLOCK)
        );
        assert_eq!(
            get_hex_genesis_block(&Network::Stagenet),
            Some(TESTNET_GENESIS_BLOCK)
        );
        assert_eq!(get_hex_genesis_block(&Network::Devnet), None);
    }

    /// Test that get_genesis_block_hash returns correct values for each network
    #[test]
    fn test_get_genesis_block_hash() {
        assert_eq!(
            get_genesis_block_hash(&Network::Mainnet),
            Some(&MAINNET_GENESIS_BLOCK_HASH)
        );
        assert_eq!(
            get_genesis_block_hash(&Network::Testnet),
            Some(&TESTNET_GENESIS_BLOCK_HASH)
        );
        assert_eq!(
            get_genesis_block_hash(&Network::Stagenet),
            Some(&TESTNET_GENESIS_BLOCK_HASH)
        );
        assert_eq!(get_genesis_block_hash(&Network::Devnet), None);
    }

    #[test]
    fn test_smart_contract_tip_activation_by_network() {
        let mainnet_tips = get_chain_tips(&Network::Mainnet);
        let testnet_tips = get_chain_tips(&Network::Testnet);
        let devnet_tips = get_chain_tips(&Network::Devnet);

        assert!(!mainnet_tips.is_active_at_height(
            tos_common::api::daemon::TosHardfork::SmartContracts,
            0
        ));
        assert!(testnet_tips.is_active_at_height(
            tos_common::api::daemon::TosHardfork::SmartContracts,
            0
        ));
        assert!(devnet_tips.is_active_at_height(
            tos_common::api::daemon::TosHardfork::SmartContracts,
            0
        ));
    }

    /// Test that mainnet and testnet genesis blocks have different hashes
    #[test]
    fn test_genesis_blocks_are_different() {
        assert_ne!(
            MAINNET_GENESIS_BLOCK, TESTNET_GENESIS_BLOCK,
            "Mainnet and testnet genesis blocks should be different"
        );
        assert_ne!(
            MAINNET_GENESIS_BLOCK_HASH, TESTNET_GENESIS_BLOCK_HASH,
            "Mainnet and testnet genesis block hashes should be different"
        );
    }

    /// Test that genesis block hex strings are valid and have expected format
    #[test]
    fn test_genesis_block_hex_format() {
        // Both genesis blocks should be valid hex
        let mainnet_bytes = hex::decode(MAINNET_GENESIS_BLOCK);
        let testnet_bytes = hex::decode(TESTNET_GENESIS_BLOCK);

        assert!(
            mainnet_bytes.is_ok(),
            "MAINNET_GENESIS_BLOCK should be valid hex"
        );
        assert!(
            testnet_bytes.is_ok(),
            "TESTNET_GENESIS_BLOCK should be valid hex"
        );

        // Genesis blocks should have reasonable size (header only, no transactions)
        let mainnet_len = mainnet_bytes.unwrap().len();
        let testnet_len = testnet_bytes.unwrap().len();

        assert!(
            mainnet_len > 50 && mainnet_len < 500,
            "Mainnet genesis block size should be reasonable: {} bytes",
            mainnet_len
        );
        assert!(
            testnet_len > 50 && testnet_len < 500,
            "Testnet genesis block size should be reasonable: {} bytes",
            testnet_len
        );
    }

    /// Test developer address constant matches DEV_ADDRESS in config
    #[test]
    fn test_developer_address_consistency() {
        // Verify the expected dev address matches the config DEV_ADDRESS
        assert_eq!(
            EXPECTED_DEV_ADDRESS, DEV_ADDRESS,
            "Test expected dev address should match config DEV_ADDRESS"
        );

        // Verify the address can be parsed and converted to public key
        let address =
            Address::from_string(EXPECTED_DEV_ADDRESS).expect("Developer address should be valid");
        let pubkey = address.to_public_key();

        // Verify it matches DEV_PUBLIC_KEY from lazy_static
        assert_eq!(
            pubkey, *DEV_PUBLIC_KEY,
            "Parsed developer public key should match DEV_PUBLIC_KEY"
        );
    }
}
