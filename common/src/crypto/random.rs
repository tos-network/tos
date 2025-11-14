/// Cryptographically secure random number generation
///
/// This module provides secure random number generation using the operating
/// system's CSPRNG (Cryptographically Secure Pseudo-Random Number Generator).
///
/// SECURITY: All production crypto operations MUST use OsRng, not thread_rng()
/// - OsRng uses OS-provided entropy (e.g., /dev/urandom on Unix)
/// - thread_rng() is NOT cryptographically secure and MUST NOT be used for:
///   - Nonce generation
///   - Key generation
///   - Signature randomness
///   - Any consensus-critical operations
use rand::rngs::OsRng;
use rand::RngCore;

/// Generate cryptographically secure random bytes
///
/// SAFETY: Uses OS-provided CSPRNG (OsRng) for true randomness
///
/// # Example
/// ```
/// use tos_common::crypto::random::secure_random_bytes;
///
/// // Generate 32 random bytes for nonce
/// let nonce = secure_random_bytes::<32>();
/// ```
pub fn secure_random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

/// Generate cryptographically secure random u64
///
/// SAFETY: Uses OS-provided CSPRNG (OsRng)
///
/// # Example
/// ```
/// use tos_common::crypto::random::secure_random_u64;
///
/// let random_nonce = secure_random_u64();
/// ```
pub fn secure_random_u64() -> u64 {
    OsRng.next_u64()
}

/// Generate cryptographically secure random u32
///
/// SAFETY: Uses OS-provided CSPRNG (OsRng)
pub fn secure_random_u32() -> u32 {
    OsRng.next_u32()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_random_bytes() {
        let bytes1 = secure_random_bytes::<32>();
        let bytes2 = secure_random_bytes::<32>();

        // Random bytes should be different each time
        assert_ne!(bytes1, bytes2);

        // Should generate correct length
        assert_eq!(bytes1.len(), 32);
    }

    #[test]
    fn test_secure_random_u64() {
        let n1 = secure_random_u64();
        let n2 = secure_random_u64();

        // Should produce different values
        assert_ne!(n1, n2);
    }

    #[test]
    fn test_secure_random_u32() {
        let n1 = secure_random_u32();
        let n2 = secure_random_u32();

        // Should produce different values
        assert_ne!(n1, n2);
    }

    #[test]
    fn test_secure_random_bytes_different_sizes() {
        let bytes8 = secure_random_bytes::<8>();
        let bytes16 = secure_random_bytes::<16>();
        let bytes32 = secure_random_bytes::<32>();
        let bytes64 = secure_random_bytes::<64>();

        assert_eq!(bytes8.len(), 8);
        assert_eq!(bytes16.len(), 16);
        assert_eq!(bytes32.len(), 32);
        assert_eq!(bytes64.len(), 64);
    }
}
