//! Storage key constants shared by all backend implementations
//!
//! Only includes constants actually used by RocksDB providers.

pub(crate) const TIPS: &[u8; 4] = b"TIPS";
pub(crate) const TOP_TOPO_HEIGHT: &[u8; 4] = b"TOPO";
pub(crate) const TOP_HEIGHT: &[u8; 4] = b"TOPH";
pub(crate) const PRUNED_TOPOHEIGHT: &[u8; 4] = b"PRUN";
pub(crate) const TXS_COUNT: &[u8; 4] = b"CTXS";
pub(crate) const BLOCKS_COUNT: &[u8; 4] = b"CBLK";
pub(crate) const BLOCKS_EXECUTION_ORDER_COUNT: &[u8; 4] = b"EBLK";
