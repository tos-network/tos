// Allow clippy lints for legacy code
#![allow(clippy::all)]
#![warn(clippy::correctness)]

pub mod cipher;
pub mod config;
pub mod entry;
pub mod error;
pub mod mnemonics;
pub mod storage;
pub mod transaction_builder;
pub mod wallet;

pub mod precomputed_tables;

#[cfg(feature = "network_handler")]
pub mod daemon_api;

#[cfg(feature = "network_handler")]
pub mod network_handler;

#[cfg(feature = "network_handler")]
pub mod light_api;

#[cfg(feature = "network_handler")]
pub mod stateless_wallet;

pub mod api;
