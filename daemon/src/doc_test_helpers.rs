//! Doc-test helper utilities for tos_daemon
//!
//! This module provides minimal helpers for documentation examples.
//! Most doc-tests use `no_run` with inline mocks to avoid complex setup.

use tos_common::crypto::Hash;

/// Generate a test hash from a simple seed
pub fn test_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

/// Create minimal valid TAKO bytecode (ELF magic bytes)
pub fn minimal_tako_bytecode() -> Vec<u8> {
    vec![0x7F, b'E', b'L', b'F']
}

/// Create a minimal ELF bytecode with extended header
pub fn minimal_elf_bytecode() -> Vec<u8> {
    let mut elf = vec![0u8; 64];
    elf[0..4].copy_from_slice(b"\x7FELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little-endian
    elf[6] = 1; // version
    elf[16] = 0x03; // ET_DYN
    elf[18] = 0xF7; // EM_BPF
    elf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helpers() {
        assert_eq!(test_hash(1).as_bytes()[0], 1);
        assert_eq!(&minimal_tako_bytecode()[..4], b"\x7FELF");
        assert_eq!(&minimal_elf_bytecode()[..4], b"\x7FELF");
    }
}
