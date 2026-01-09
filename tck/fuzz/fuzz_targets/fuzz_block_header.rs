//! Fuzz target for block header deserialization
//!
//! Tests that arbitrary byte sequences do not cause panics
//! when parsed as block headers.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tos_common::block::BlockHeader;
use tos_common::serializer::Reader;

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize as block header
    // Should never panic, only return errors
    let mut reader = Reader::new(data);
    let _ = BlockHeader::read(&mut reader);
});
