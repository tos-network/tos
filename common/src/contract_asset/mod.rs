//! Contract Asset Module
//!
//! This module provides contract asset support for the TOS blockchain,
//! implementing ERC20-like functionality at the protocol level.
//!
//! # Features
//!
//! - Core token operations (create, mint, burn, transfer)
//! - ERC20-compatible approval system
//! - Governance with vote delegation (ERC20Votes)
//! - Token timelock (vesting, staking)
//! - Role-based access control
//! - Admin operations (pause, freeze)
//! - Escrow system
//! - Permit (gasless approvals)
//! - AGI agent integration

pub mod constants;
pub mod error;
pub mod overlay;
pub mod roles;
pub mod types;

pub use constants::*;
pub use error::*;
pub use overlay::{ContractAssetKey, ContractAssetValue};
pub use roles::*;
pub use types::*;

// Token naming aliases for contract-scoped token domain.
pub type TokenKey = ContractAssetKey;
pub type TokenValue = ContractAssetValue;
pub type TokenData = ContractAssetData;
