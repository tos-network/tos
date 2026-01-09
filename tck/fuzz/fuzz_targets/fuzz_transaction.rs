//! Fuzz target for transaction deserialization
//!
//! Tests that arbitrary byte sequences do not cause panics
//! when parsed as transactions.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tos_common::serializer::Reader;
use tos_common::transaction::Transaction;

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize as transaction
    // Should never panic, only return errors
    let mut reader = Reader::new(data);
    let _ = Transaction::read(&mut reader);
});
