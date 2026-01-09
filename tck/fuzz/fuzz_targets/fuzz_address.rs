//! Fuzz target for address parsing
//!
//! Tests that arbitrary strings do not cause panics
//! when parsed as blockchain addresses.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str::FromStr;
use tos_common::address::Address;

fuzz_target!(|data: &[u8]| {
    // Try to parse as UTF-8 string
    if let Ok(addr_str) = std::str::from_utf8(data) {
        // Attempt to parse as address
        // Should never panic, only return errors
        let _ = Address::from_str(addr_str);
    }

    // Also try direct bytes parsing if address supports it
    if data.len() == 32 {
        // Try to create address from 32-byte array
        let mut arr = [0u8; 32];
        arr.copy_from_slice(data);
        let _ = Address::new(arr);
    }
});
