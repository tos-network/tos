// TNS (TOS Name Service) Module
//
// This module provides account naming functionality for the TOS network,
// allowing users to register human-readable names (e.g., alice@tos.network)
// and send ephemeral messages to other registered users.

mod constants;
mod hash;
mod normalize;
mod reserved;
mod validate;

pub use constants::*;
pub use hash::*;
pub use normalize::*;
pub use reserved::*;
pub use validate::*;
