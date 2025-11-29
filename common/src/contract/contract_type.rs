/// Contract bytecode validation for TOS Kernel(TAKO) (eBPF-based execution)
///
/// This module validates that contract bytecode is in the correct ELF format
/// for execution by TOS Kernel(TAKO). TOS blockchain only supports TOS Kernel(TAKO) contracts.
use anyhow::{anyhow, Result};

/// Validates that contract bytecode is a valid ELF binary for TOS Kernel(TAKO)
///
/// # Contract Format
///
/// TOS Kernel(TAKO) contracts must be:
/// - **ELF format**: Standard Unix executable format
/// - **eBPF bytecode**: Compiled from Rust using TAKO SDK
/// - **Magic number**: Must start with `0x7F 'E' 'L' 'F'`
///
/// # Arguments
///
/// * `bytecode` - The contract bytecode to validate
///
/// # Returns
///
/// - `Ok(())` if bytecode is valid ELF format
/// - `Err(_)` if bytecode is invalid, empty, or too short
///
/// # Examples
///
/// ```
/// # use tos_common::contract::validate_contract_bytecode;
/// // Valid TOS Kernel(TAKO) contract (ELF format with minimum 64 bytes)
/// let mut valid_bytecode = vec![0x7F, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00];
/// valid_bytecode.extend_from_slice(&[0u8; 56]); // Pad to 64 bytes minimum
/// assert!(validate_contract_bytecode(&valid_bytecode).is_ok());
///
/// // Invalid bytecode (not ELF)
/// let invalid_bytecode = b"\x00\x01\x02\x03";
/// assert!(validate_contract_bytecode(invalid_bytecode).is_err());
/// ```
pub fn validate_contract_bytecode(bytecode: &[u8]) -> Result<()> {
    const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
    const MIN_ELF_SIZE: usize = 64; // Minimum ELF header size

    if bytecode.is_empty() {
        return Err(anyhow!("Contract bytecode is empty"));
    }

    if bytecode.len() < 4 {
        return Err(anyhow!(
            "Contract bytecode too short: {} bytes (minimum 4 required for magic number)",
            bytecode.len()
        ));
    }

    // Check for ELF magic number
    if &bytecode[0..4] != ELF_MAGIC {
        return Err(anyhow!(
            "Invalid contract format: expected ELF binary (magic: 0x7F 'E' 'L' 'F'), got: {:02X?}",
            &bytecode[0..4.min(bytecode.len())]
        ));
    }

    // Basic ELF header validation
    if bytecode.len() < MIN_ELF_SIZE {
        return Err(anyhow!(
            "Contract bytecode too short: {} bytes (minimum {} required for ELF header)",
            bytecode.len(),
            MIN_ELF_SIZE
        ));
    }

    Ok(())
}

/// Checks if bytecode is in ELF format (without full validation)
///
/// This is a quick check that only examines the magic number.
/// For full validation, use `validate_contract_bytecode()`.
///
/// # Examples
///
/// ```
/// # use tos_common::contract::is_elf_bytecode;
/// assert!(is_elf_bytecode(b"\x7FELF\x02\x01\x01..."));
/// assert!(!is_elf_bytecode(b"\x00\x01\x02\x03..."));
/// ```
pub fn is_elf_bytecode(bytecode: &[u8]) -> bool {
    const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
    bytecode.len() >= 4 && &bytecode[0..4] == ELF_MAGIC
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_elf() {
        // Minimal valid ELF header (64 bytes)
        let mut elf = vec![0u8; 64];
        elf[0..4].copy_from_slice(b"\x7FELF");

        assert!(validate_contract_bytecode(&elf).is_ok());
    }

    #[test]
    fn test_validate_invalid_magic() {
        let invalid = b"\x00\x01\x02\x03";
        let result = validate_contract_bytecode(invalid);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid contract format"));
    }

    #[test]
    fn test_validate_empty() {
        let empty = b"";
        let result = validate_contract_bytecode(empty);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_too_short_magic() {
        let short = b"\x7FE"; // Only 2 bytes
        let result = validate_contract_bytecode(short);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn test_validate_too_short_header() {
        // Has magic but header too short (only 10 bytes, needs 64)
        let short_header = b"\x7FELF123456";
        let result = validate_contract_bytecode(short_header);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("too short"));
        assert!(error_msg.contains("ELF header"));
    }

    #[test]
    fn test_is_elf_bytecode_valid() {
        let elf = b"\x7FELF\x02\x01\x01\x00";
        assert!(is_elf_bytecode(elf));
    }

    #[test]
    fn test_is_elf_bytecode_invalid() {
        let not_elf = b"\x00\x01\x02\x03";
        assert!(!is_elf_bytecode(not_elf));
    }

    #[test]
    fn test_is_elf_bytecode_too_short() {
        let too_short = b"\x7FE";
        assert!(!is_elf_bytecode(too_short));
    }

    #[test]
    fn test_is_elf_bytecode_empty() {
        let empty = b"";
        assert!(!is_elf_bytecode(empty));
    }
}
