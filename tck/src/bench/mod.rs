//! Performance benchmark helpers for TOS-TCK.
//!
//! These helpers are used by Criterion benches under `tck/benches/`.
//! The benchmark harnesses live in that directory, while this module
//! provides shared utilities to keep the benches consistent.

pub mod block_benchmark;
pub mod dag_benchmark;
pub mod sync_benchmark;
pub mod tps_benchmark;
