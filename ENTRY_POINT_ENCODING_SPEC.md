# Entry Point and Hook Encoding Specification

**Version**: 1.0
**Date**: 2025-11-08
**Status**: Implemented

---

## Summary

This document specifies the encoding format for contract entry points and hooks passed to TAKO VM during contract invocation. The encoding replaces the previous implementation that cast `u16` entry IDs to `u8`, causing data loss for IDs > 255.

## Motivation

**Problem**: Previous implementation used simple byte arrays:
```rust
// OLD - DATA LOSS!
InvokeContract::Entry(entry_id) => Some(vec![0u8, entry_id as u8]),  // ❌ Loses high byte
InvokeContract::Hook(hook_id) => Some(vec![1u8, hook_id]),
```

**Issues**:
- Entry IDs > 255 lost data (u16 → u8 cast)
- No room for future extensions
- Undocumented format

**Solution**: Proper little-endian encoding with type discriminators and full range support.

---

## Encoding Format

### Entry Point Encoding (3 bytes)

```
[0x00, entry_id_low_byte, entry_id_high_byte]
```

- **Byte 0**: Type discriminator (`0x00` = Entry Point)
- **Byte 1**: Low byte of entry_id (bits 0-7)
- **Byte 2**: High byte of entry_id (bits 8-15)
- **Range**: Supports full u16 range (0 to 65535)
- **Byte Order**: Little-endian (matches x86/ARM architectures)

### Hook Encoding (2 bytes)

```
[0x01, hook_id]
```

- **Byte 0**: Type discriminator (`0x01` = Hook)
- **Byte 1**: Hook ID (u8, 0-255)
- **Range**: Full u8 range (0 to 255)

---

## Examples

### Entry Point Encoding

| Entry ID | Hex   | Encoded Bytes       | Explanation                  |
|----------|-------|---------------------|------------------------------|
| 0        | 0x0000| `[0x00, 0x00, 0x00]`| Minimum value                |
| 255      | 0x00FF| `[0x00, 0xFF, 0x00]`| Fits in one byte             |
| 256      | 0x0100| `[0x00, 0x00, 0x01]`| Requires two bytes           |
| 1000     | 0x03E8| `[0x00, 0xE8, 0x03]`| Little-endian: 0xE8, 0x03    |
| 65535    | 0xFFFF| `[0x00, 0xFF, 0xFF]`| Maximum u16 value            |

### Hook Encoding

| Hook ID | Encoded Bytes  | Explanation       |
|---------|----------------|-------------------|
| 0       | `[0x01, 0x00]` | Minimum value     |
| 127     | `[0x01, 0x7F]` | Mid-range         |
| 255     | `[0x01, 0xFF]` | Maximum u8 value  |

---

## TAKO VM Decoder Implementation

The TAKO VM should decode the parameters as follows:

```rust
/// Decode invocation parameters from TOS blockchain
fn decode_invocation(params: &[u8]) -> Result<InvocationType, Error> {
    match params.first() {
        // Entry Point: [0x00, low_byte, high_byte]
        Some(0x00) if params.len() >= 3 => {
            let entry_id = u16::from_le_bytes([params[1], params[2]]);
            Ok(InvocationType::Entry(entry_id))
        }

        // Hook: [0x01, hook_id]
        Some(0x01) if params.len() >= 2 => {
            let hook_id = params[1];
            Ok(InvocationType::Hook(hook_id))
        }

        // Unknown or malformed
        _ => Err(Error::InvalidInvocationType)
    }
}
```

### Validation

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_entry_points() {
        // Entry 0
        assert_eq!(decode_invocation(&[0x00, 0x00, 0x00]),
                   Ok(InvocationType::Entry(0)));

        // Entry 255
        assert_eq!(decode_invocation(&[0x00, 0xFF, 0x00]),
                   Ok(InvocationType::Entry(255)));

        // Entry 256 (critical test - would fail with old encoding)
        assert_eq!(decode_invocation(&[0x00, 0x00, 0x01]),
                   Ok(InvocationType::Entry(256)));

        // Entry 65535
        assert_eq!(decode_invocation(&[0x00, 0xFF, 0xFF]),
                   Ok(InvocationType::Entry(65535)));
    }

    #[test]
    fn test_decode_hooks() {
        assert_eq!(decode_invocation(&[0x01, 0x00]),
                   Ok(InvocationType::Hook(0)));
        assert_eq!(decode_invocation(&[0x01, 0xFF]),
                   Ok(InvocationType::Hook(255)));
    }

    #[test]
    fn test_decode_errors() {
        // Too short
        assert!(decode_invocation(&[0x00]).is_err());
        assert!(decode_invocation(&[0x00, 0x00]).is_err());

        // Unknown discriminator
        assert!(decode_invocation(&[0x02, 0x00]).is_err());
    }
}
```

---

## Rationale

### 1. Little-Endian Byte Order
- **Why**: Matches most modern architectures (x86, ARM, RISC-V)
- **Benefit**: Direct memory mapping without byte swapping
- **Example**: `1000 (0x03E8)` → `[0xE8, 0x03]` (low byte first)

### 2. Fixed-Size Encoding
- **Why**: Predictable memory layout, no variable-length parsing
- **Benefit**: Faster decoding, simpler VM implementation
- **Sizes**: Entry = 3 bytes, Hook = 2 bytes

### 3. Type Discriminator
- **Why**: Enables future extension with new invocation types
- **Benefit**: Backward compatibility, versioning support
- **Reserved**: `0x02-0xFF` for future use

### 4. Full Range Support
- **Why**: Contracts may have hundreds of entry points
- **Benefit**: No artificial limitations, no data loss
- **Example**: DeFi protocol with 300+ entry points (transfer, swap, stake, unstake, etc.)

---

## Future Extensions

The discriminator byte allows for future invocation types:

| Discriminator | Type                     | Format (proposed)                    |
|---------------|--------------------------|--------------------------------------|
| `0x00`        | Entry Point              | `[0x00, low, high]` ✅ Implemented  |
| `0x01`        | Hook                     | `[0x01, hook_id]` ✅ Implemented    |
| `0x02`        | Constructor with params  | `[0x02, param_len, ...]`             |
| `0x03`        | Upgrade/migration        | `[0x03, version, ...]`               |
| `0x04`        | View-only query          | `[0x04, query_type, ...]`            |
| `0x05-0xFF`   | Reserved                 | Future use                           |

---

## Implementation Status

### TOS Blockchain Side

**Location**: `/Users/tomisetsu/tos-network/tos/common/src/transaction/`

**Files Modified**:
1. `encoding.rs` (NEW) - Encoding helper functions
2. `verify/contract.rs` - Updated to use new encoding
3. `mod.rs` - Exported encoding module

**Functions**:
```rust
pub fn encode_entry_point(entry_id: u16) -> Vec<u8>
pub fn encode_hook(hook_id: u8) -> Vec<u8>
```

**Tests**: 9 comprehensive tests covering:
- Edge cases (0, 255, 256, 65535)
- Common values (1, 10, 100, 1000)
- Determinism, byte order, no data loss

**Status**: ✅ Implemented and tested (182/182 tests passing)

### TAKO VM Side

**Status**: 🔴 **TODO - Decoder needs implementation**

**Action Required**:
1. Add `decode_invocation()` function to TAKO VM
2. Update contract entrypoint to receive decoded parameters
3. Add tests for decoder (see examples above)
4. Document in TAKO VM README

**Suggested Location**:
- `tako/program-runtime/src/invoke_context.rs` (decoder logic)
- `tako/sdk/src/entrypoint.rs` (contract-side API)

---

## Backward Compatibility

### Breaking Change

This is a **breaking change** for any existing deployed contracts that rely on the old encoding format.

**Migration Strategy**:
1. Deploy updated TAKO VM with new decoder
2. Redeploy all contracts to use new encoding
3. Update blockchain to use new encoding
4. No protocol version bump needed (internal implementation detail)

**Impact**: Low - TAKO VM is still in development, no production contracts deployed

---

## Verification Checklist

Before deploying to production:

- [x] Encoding functions implemented
- [x] TOS blockchain updated to use new encoding
- [x] Comprehensive tests added (9 tests)
- [x] All tests passing (182/182)
- [x] Code formatted (`cargo fmt`)
- [x] No clippy warnings in new code
- [ ] TAKO VM decoder implemented (TODO)
- [ ] TAKO VM decoder tests added (TODO)
- [ ] End-to-end integration test (TODO)
- [ ] Documentation updated (This file)

---

## References

**Code Locations**:
- Encoding: `/tos/common/src/transaction/encoding.rs`
- Usage: `/tos/common/src/transaction/verify/contract.rs:98-100`
- Tests: `/tos/common/src/transaction/encoding.rs:133-247`

**Related Documents**:
- TAKO VM Architecture: `~/tos-network/memo/15-TAKO-VM/ARCHITECTURE.md`
- Integration Guide: `~/tos-network/memo/15-TAKO-VM/INTEGRATION_GUIDE.md`

**Git History**:
- Implementation Date: 2025-11-08
- Commit: (Not committed - implementation only)

---

## Contact

For questions or issues with this encoding specification:
- Review code in `/tos/common/src/transaction/encoding.rs`
- Check tests for usage examples
- See TAKO VM integration in `/tako/program-runtime/`

**Last Updated**: 2025-11-08
**Next Review**: When implementing TAKO VM decoder
