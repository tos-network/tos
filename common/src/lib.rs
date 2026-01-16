// Allow specific clippy lints that are acceptable design decisions
#![allow(clippy::too_many_arguments)] // API design choice
#![allow(clippy::module_inception)] // Module organization choice
#![allow(clippy::upper_case_acronyms)] // Style choice for acronyms like RPC, TX
#![allow(clippy::result_unit_err)] // Used for simple validation functions
#![allow(clippy::ptr_arg)] // API compatibility
#![allow(clippy::owned_cow)] // Cow usage pattern

pub mod a2a;
pub mod account;
pub mod api;
pub mod block;
pub mod contract;
pub mod crypto;
pub mod serializer;
pub mod transaction;

// Native Referral System module
pub mod referral;

// Native KYC Level System module
pub mod kyc;

// Native NFT System module
pub mod nft;

// Contract Asset System module (ERC20-like tokens)
pub mod contract_asset;

// TNS (TOS Name Service) module
pub mod tns;

pub mod asset;
pub mod config;
pub mod context;
pub mod difficulty;
pub mod immutable;
pub mod network;
pub mod queue;
pub mod time;
pub mod utils;
pub mod varuint;
pub mod versioned_type;

pub mod tokio;

#[cfg(feature = "rpc")]
pub mod rpc;

#[cfg(feature = "prompt")]
pub mod prompt;

#[cfg(feature = "clap")]
// If clap feature is enabled, build the correct style for CLI
pub fn get_cli_styles() -> clap::builder::Styles {
    use clap::builder::styling::*;

    clap::builder::Styles::styled()
        .usage(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        )
        .header(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        )
        .literal(Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))))
        .invalid(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Red))),
        )
        .error(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Red))),
        )
        .valid(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Green))),
        )
        .placeholder(Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))))
}
