//! Entry point and hook encoding for TOS Kernel(TAKO) contract invocation
//!
//! This module provides deterministic encoding for contract entry points and hooks.
//! The encoding format is designed to support the full range of IDs and be easily
//! extendable for future use cases.
//!
//! ## Encoding Format
//!
//! ### Entry Point Encoding (3 bytes)
//! ```text
//! [0x00, entry_id_low_byte, entry_id_high_byte]
//! ```
//! - Byte 0: Type discriminator (0x00 = Entry Point)
//! - Byte 1: Low byte of entry_id (bits 0-7)
//! - Byte 2: High byte of entry_id (bits 8-15)
//! - Supports full u16 range: 0 to 65535
//!
//! ### Hook Encoding (2 bytes)
//! ```text
//! [0x01, hook_id]
//! ```
//! - Byte 0: Type discriminator (0x01 = Hook)
//! - Byte 1: Hook ID (u8, 0-255)
//!
//! ## Rationale
//!
//! 1. **Little-endian byte order**: Matches most modern architectures (x86, ARM)
//! 2. **Fixed-size encoding**: Predictable memory layout, no variable-length parsing
//! 3. **Type discriminator**: Enables future extension with new invocation types
//! 4. **No data loss**: Supports full u16 range for entry_id (previous: cast to u8)
//!
//! ## TOS Kernel(TAKO) Decoder
//!
//! The TOS Kernel(TAKO) side should decode the invocation parameters.
//! Here's an example demonstrating the encoding/decoding roundtrip:
//!
//! ```rust
//! use tos_common::transaction::encoding::{encode_entry_point, encode_hook};
//! use tos_common::transaction::encoding::{ENTRY_POINT_DISCRIMINATOR, HOOK_DISCRIMINATOR};
//!
//! // Example decoder function
//! fn decode_invocation(params: &[u8]) -> Result<String, &'static str> {
//!     match params.first() {
//!         Some(&ENTRY_POINT_DISCRIMINATOR) if params.len() >= 3 => {
//!             let entry_id = u16::from_le_bytes([params[1], params[2]]);
//!             Ok(format!("Entry({})", entry_id))
//!         }
//!         Some(&HOOK_DISCRIMINATOR) if params.len() >= 2 => {
//!             let hook_id = params[1];
//!             Ok(format!("Hook({})", hook_id))
//!         }
//!         _ => Err("InvalidInvocationType")
//!     }
//! }
//!
//! // Test encoding and decoding roundtrip
//! let entry_encoded = encode_entry_point(1234);
//! assert_eq!(decode_invocation(&entry_encoded).unwrap(), "Entry(1234)");
//!
//! let hook_encoded = encode_hook(42);
//! assert_eq!(decode_invocation(&hook_encoded).unwrap(), "Hook(42)");
//!
//! // Test invalid discriminator
//! assert!(decode_invocation(&[0xFF]).is_err());
//! ```
//!
//! ## Future Extensions
//!
//! The discriminator byte allows for future invocation types:
//! - 0x02: Constructor with parameters
//! - 0x03: Upgrade/migration entry point
//! - 0x04: View-only query (no state modification)
//! - etc.

/// Type discriminator for entry point invocation
pub const ENTRY_POINT_DISCRIMINATOR: u8 = 0x00;

/// Type discriminator for hook invocation
pub const HOOK_DISCRIMINATOR: u8 = 0x01;

/// Encode an entry point ID for TOS Kernel(TAKO) invocation
///
/// # Format
/// ```text
/// [0x00, entry_id_low_byte, entry_id_high_byte]
/// ```
///
/// # Arguments
/// * `entry_id` - Entry point identifier (u16, supports 0-65535)
///
/// # Returns
/// 3-byte vector containing the encoded entry point
///
/// # Examples
/// ```
/// use tos_common::transaction::encoding::encode_entry_point;
///
/// // Entry 0
/// assert_eq!(encode_entry_point(0), vec![0x00, 0x00, 0x00]);
///
/// // Entry 255 (fits in one byte, but uses two)
/// assert_eq!(encode_entry_point(255), vec![0x00, 0xFF, 0x00]);
///
/// // Entry 256 (requires two bytes)
/// assert_eq!(encode_entry_point(256), vec![0x00, 0x00, 0x01]);
///
/// // Entry 65535 (maximum u16)
/// assert_eq!(encode_entry_point(65535), vec![0x00, 0xFF, 0xFF]);
/// ```
pub fn encode_entry_point(entry_id: u16) -> Vec<u8> {
    let [low, high] = entry_id.to_le_bytes();
    vec![ENTRY_POINT_DISCRIMINATOR, low, high]
}

/// Encode a hook ID for TOS Kernel(TAKO) invocation
///
/// # Format
/// ```text
/// [0x01, hook_id]
/// ```
///
/// # Arguments
/// * `hook_id` - Hook identifier (u8, supports 0-255)
///
/// # Returns
/// 2-byte vector containing the encoded hook
///
/// # Examples
/// ```
/// use tos_common::transaction::encoding::encode_hook;
///
/// // Hook 0
/// assert_eq!(encode_hook(0), vec![0x01, 0x00]);
///
/// // Hook 127
/// assert_eq!(encode_hook(127), vec![0x01, 0x7F]);
///
/// // Hook 255 (maximum u8)
/// assert_eq!(encode_hook(255), vec![0x01, 0xFF]);
/// ```
pub fn encode_hook(hook_id: u8) -> Vec<u8> {
    vec![HOOK_DISCRIMINATOR, hook_id]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_point_encoding_edge_cases() {
        // Minimum value
        assert_eq!(encode_entry_point(0), vec![0x00, 0x00, 0x00]);

        // Single-byte boundary
        assert_eq!(encode_entry_point(127), vec![0x00, 0x7F, 0x00]);
        assert_eq!(encode_entry_point(128), vec![0x00, 0x80, 0x00]);
        assert_eq!(encode_entry_point(255), vec![0x00, 0xFF, 0x00]);

        // Two-byte boundary
        assert_eq!(encode_entry_point(256), vec![0x00, 0x00, 0x01]);
        assert_eq!(encode_entry_point(257), vec![0x00, 0x01, 0x01]);

        // Maximum value (u16::MAX)
        assert_eq!(encode_entry_point(65535), vec![0x00, 0xFF, 0xFF]);
        assert_eq!(encode_entry_point(u16::MAX), vec![0x00, 0xFF, 0xFF]);
    }

    #[test]
    fn test_entry_point_encoding_common_values() {
        // Common entry point IDs that might be used
        assert_eq!(encode_entry_point(1), vec![0x00, 0x01, 0x00]);
        assert_eq!(encode_entry_point(2), vec![0x00, 0x02, 0x00]);
        assert_eq!(encode_entry_point(10), vec![0x00, 0x0A, 0x00]);
        assert_eq!(encode_entry_point(100), vec![0x00, 0x64, 0x00]);
        assert_eq!(encode_entry_point(1000), vec![0x00, 0xE8, 0x03]);
    }

    #[test]
    fn test_hook_encoding_edge_cases() {
        // Minimum value
        assert_eq!(encode_hook(0), vec![0x01, 0x00]);

        // Mid-range values
        assert_eq!(encode_hook(127), vec![0x01, 0x7F]);
        assert_eq!(encode_hook(128), vec![0x01, 0x80]);

        // Maximum value (u8::MAX)
        assert_eq!(encode_hook(255), vec![0x01, 0xFF]);
        assert_eq!(encode_hook(u8::MAX), vec![0x01, 0xFF]);
    }

    #[test]
    fn test_hook_encoding_common_values() {
        // Common hook IDs that might be used
        assert_eq!(encode_hook(1), vec![0x01, 0x01]);
        assert_eq!(encode_hook(2), vec![0x01, 0x02]);
        assert_eq!(encode_hook(10), vec![0x01, 0x0A]);
        assert_eq!(encode_hook(100), vec![0x01, 0x64]);
    }

    #[test]
    fn test_discriminator_uniqueness() {
        // Ensure entry point and hook have different discriminators
        let entry = encode_entry_point(42);
        let hook = encode_hook(42);

        assert_ne!(entry[0], hook[0]);
        assert_eq!(entry[0], ENTRY_POINT_DISCRIMINATOR);
        assert_eq!(hook[0], HOOK_DISCRIMINATOR);
    }

    #[test]
    fn test_encoding_length() {
        // Entry points always encode to 3 bytes
        assert_eq!(encode_entry_point(0).len(), 3);
        assert_eq!(encode_entry_point(u16::MAX).len(), 3);

        // Hooks always encode to 2 bytes
        assert_eq!(encode_hook(0).len(), 2);
        assert_eq!(encode_hook(u8::MAX).len(), 2);
    }

    #[test]
    fn test_little_endian_byte_order() {
        // Verify little-endian encoding for entry_id = 0x1234
        let encoded = encode_entry_point(0x1234);
        assert_eq!(encoded, vec![0x00, 0x34, 0x12]);

        // Reconstruct to verify
        let reconstructed = u16::from_le_bytes([encoded[1], encoded[2]]);
        assert_eq!(reconstructed, 0x1234);
    }

    #[test]
    fn test_no_data_loss() {
        // Previous implementation cast u16 to u8, losing high byte
        // Verify we can encode and decode the full range

        // Values that would lose data with u8 cast
        let large_ids = [256u16, 300, 1000, 5000, 65535];

        for id in large_ids {
            let encoded = encode_entry_point(id);
            let decoded = u16::from_le_bytes([encoded[1], encoded[2]]);
            assert_eq!(decoded, id, "Data loss detected for entry_id={id}");
        }
    }

    #[test]
    fn test_encoding_determinism() {
        // Same input always produces same output
        let id = 12345u16;
        let encoded1 = encode_entry_point(id);
        let encoded2 = encode_entry_point(id);
        assert_eq!(encoded1, encoded2);

        let hook = 42u8;
        let hook1 = encode_hook(hook);
        let hook2 = encode_hook(hook);
        assert_eq!(hook1, hook2);
    }
}
