// Allow some clippy lints for legacy code - to be fixed gradually
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::module_inception)]
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::result_unit_err)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::owned_cow)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::single_match)]
#![allow(clippy::needless_return)]
#![allow(clippy::type_complexity)]
#![allow(clippy::to_string_trait_impl)]

pub mod account;
pub mod api;
pub mod block;
pub mod contract;
pub mod crypto;
pub mod serializer;
pub mod transaction;

// AI Mining module
pub mod ai_mining;

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

// TODO: Fix doc_test_helpers module - currently broken due to outdated trait implementations
// The module has been temporarily disabled to allow doc-tests to run.
// It needs to be updated to match current ContractProvider and ContractExecutor traits.
// #[cfg(any(test, doctest))]
// pub mod doc_test_helpers;

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
