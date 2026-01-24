// Phase 12: P2P Protocol Testing (Real Code)
//
// Three-layer testing strategy:
// - Layer 1 (Unit): Direct imports from tos_daemon, testing pure logic
// - Layer 2 (Medium): Algorithm-level with real data structures
// - Layer 3 (Integration): LocalTosNetwork multi-node real behavior

/// Layer 1: Encryption unit tests (ChaCha20-Poly1305 roundtrip, key rotation, nonce management)
pub mod encryption;

/// Layer 1: Handshake serialization and validation tests
pub mod handshake;

/// Layer 3: Block/TX propagation integration tests using LocalTosNetwork
pub mod propagation;

/// Layer 1.5: ChainClient transaction lifecycle tests (mempool, mine, nonce, batch, fee)
pub mod chain_client_p2p;

/// Layer 3: Network partition and recovery tests using LocalTosNetwork
pub mod partition;
