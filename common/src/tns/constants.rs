// TNS (TOS Name Service) Constants

/// Minimum name length (3 characters)
pub const MIN_NAME_LENGTH: usize = 3;

/// Maximum name length (64 characters)
pub const MAX_NAME_LENGTH: usize = 64;

/// Minimum TTL for ephemeral messages (~30 minutes)
pub const MIN_TTL: u32 = 100;

/// Maximum TTL for ephemeral messages (~3 days)
pub const MAX_TTL: u32 = 86_400;

/// Default TTL for ephemeral messages (~30 minutes)
pub const DEFAULT_TTL: u32 = 100;

/// Maximum message size in bytes (SMS standard)
pub const MAX_MESSAGE_SIZE: usize = 140;

/// Maximum encrypted message size (plaintext + encryption overhead)
/// Overhead: 16 bytes (Poly1305 MAC) + 32 bytes (receiver_handle) = 48 bytes
pub const MAX_ENCRYPTED_SIZE: usize = MAX_MESSAGE_SIZE + 48;

/// Base fee for ephemeral messages (same as transfer fee: 0.00005 TOS)
pub const BASE_MESSAGE_FEE: u64 = 5000;

/// Registration fee for TNS names (0.1 TOS = 10_000_000 atomic units)
pub const REGISTRATION_FEE: u64 = 10_000_000;
