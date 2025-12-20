// Storage constant keys shared between RocksDB and other storage implementations
//
// These constants define the byte keys used for storing metadata in the database.
// Originally from sled/mod.rs, now centralized for use by all storage backends.

// Constant keys used for extra/common storage
pub const TIPS: &[u8; 4] = b"TIPS";
pub const TOP_TOPO_HEIGHT: &[u8; 4] = b"TOPO";
pub const TOP_HEIGHT: &[u8; 4] = b"TOPH";
pub const NETWORK: &[u8; 3] = b"NET";
pub const PRUNED_TOPOHEIGHT: &[u8; 4] = b"PRUN";

// Counters (prevent to perform a O(n))
pub const ACCOUNTS_COUNT: &[u8; 4] = b"CACC";
pub const TXS_COUNT: &[u8; 4] = b"CTXS";
pub const ASSETS_COUNT: &[u8; 4] = b"CAST";
pub const BLOCKS_COUNT: &[u8; 4] = b"CBLK";
pub const BLOCKS_EXECUTION_ORDER_COUNT: &[u8; 4] = b"EBLK";

// AI Mining state keys
pub const AI_MINING_STATE_TOPOHEIGHT: &[u8; 4] = b"AIMT";

// Contract counter
pub const CONTRACTS_COUNT: &[u8; 4] = b"CCON";

// Database version key
pub const DB_VERSION: &[u8; 4] = b"VRSN";
