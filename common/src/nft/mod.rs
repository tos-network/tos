// Native NFT System for TOS Blockchain
// This module provides chain-native NFT functionality.
//
// Features:
// - NFT Collections with configurable mint authority
// - Individual NFT tokens with on-chain attributes
// - Royalty support for secondary sales
// - Freeze/unfreeze functionality
// - Operator approvals (ERC721-style)
// - Token Bound Accounts (ERC6551-style)
// - Rental system with two-step flow
//
// Module Structure:
// - error: Error codes and types
// - types: Core data structures (NftCollection, Nft, etc.)
// - storage: Storage key prefixes and helpers
// - operations: Core operation logic (create, mint, transfer, burn, query)

mod cache;
mod error;
pub mod operations;
mod storage;
mod types;

pub use cache::*;
pub use error::*;
pub use operations::*;
pub use storage::*;
pub use types::*;
