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
// - storage: Storage key prefixes and helpers (to be implemented)

mod error;
mod storage;
mod types;

pub use error::*;
pub use storage::*;
pub use types::*;
