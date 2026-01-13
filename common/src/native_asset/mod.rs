//! Native Asset Module
//!
//! This module provides native asset support for the TOS blockchain,
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
pub mod roles;
pub mod types;

pub use constants::*;
pub use error::*;
pub use roles::*;
pub use types::*;
