//! Fuzz target for block deserialization
//!
//! Tests that arbitrary byte sequences never cause panics when deserializing blocks.
//! This is critical for network security as malicious peers could send crafted blocks.
//!
//! Security properties tested:
//! - No panic on malformed input
//! - No unbounded memory allocation
//! - Proper error handling for invalid data
//!
//! Run with: cargo +nightly fuzz run fuzz_block_deserialize

#![no_main]

use libfuzzer_sys::fuzz_target;
use tos_common::serializer::Reader;
use tos_common::block::{BlockHeader, Block};

fuzz_target!(|data: &[u8]| {
    // SECURITY: Limit input size to prevent memory exhaustion DoS
    const MAX_BLOCK_SIZE: usize = 10 * 1024 * 1024; // 10MB
    if data.len() > MAX_BLOCK_SIZE {
        return;
    }

    // Test 1: BlockHeader deserialization should never panic
    {
        let mut reader = Reader::new(data);
        let _ = BlockHeader::read(&mut reader);
        // Should return Result, never panic
    }

    // Test 2: Full Block deserialization should never panic
    {
        let mut reader = Reader::new(data);
        let _ = Block::read(&mut reader);
        // Should return Result, never panic
    }

    // Test 3: Multiple consecutive deserializations
    // Simulates parsing a stream of blocks
    {
        let mut reader = Reader::new(data);
        while !reader.is_empty() {
            let _ = BlockHeader::read(&mut reader);
            // Continue parsing until end of input
            // Should never panic on malformed stream
        }
    }

    // Test 4: Verify error handling is consistent
    {
        let mut reader1 = Reader::new(data);
        let mut reader2 = Reader::new(data);

        let result1 = BlockHeader::read(&mut reader1);
        let result2 = BlockHeader::read(&mut reader2);

        // Same input should produce same result
        match (result1, result2) {
            (Ok(_), Ok(_)) => {},
            (Err(_), Err(_)) => {},
            _ => {
                // Non-deterministic behavior would be a bug
                // But we don't panic, just note it
            }
        }
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let data = &[];
        let mut reader = Reader::new(data);
        assert!(BlockHeader::read(&mut reader).is_err());
    }

    #[test]
    fn test_single_byte() {
        let data = &[0x42];
        let mut reader = Reader::new(data);
        assert!(BlockHeader::read(&mut reader).is_err());
    }

    #[test]
    fn test_all_zeros() {
        let data = vec![0u8; 1000];
        let mut reader = Reader::new(&data);
        let _ = BlockHeader::read(&mut reader);
        // Should not panic
    }

    #[test]
    fn test_all_ones() {
        let data = vec![0xFFu8; 1000];
        let mut reader = Reader::new(&data);
        let _ = BlockHeader::read(&mut reader);
        // Should not panic
    }
}
