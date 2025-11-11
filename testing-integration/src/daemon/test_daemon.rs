//! TestDaemon - Wrapper for TOS daemon in tests
//!
//! This module provides a test-friendly wrapper around the TOS daemon with:
//! - Automatic port allocation (no conflicts)
//! - Temporary directory management (auto-cleanup)
//! - Easy lifecycle management (start/stop)
//!
//! # Example
//!
//! ```rust,ignore
//! use tos_testing_integration::TestDaemon;
//!
//! #[tokio::test]
//! async fn test_with_daemon() {
//!     let mut daemon = TestDaemon::new_random();
//!     let rpc_client = daemon.start().await;
//!
//!     // Test with daemon...
//!
//!     daemon.shutdown().await;  // Auto-cleanup on drop too
//! }
//! ```

use tempdir::TempDir;

/// Test-friendly wrapper around TOS daemon
///
/// This struct manages the lifecycle of a TOS daemon instance for testing,
/// with automatic cleanup and port allocation.
///
/// **Note**: This is a placeholder for MVP. Full implementation will require
/// access to tos_daemon::Daemon API which may need refactoring for testability.
pub struct TestDaemon {
    /// Temporary directory (auto-cleanup on drop)
    _temp_dir: TempDir,

    /// Allocated RPC port
    pub rpc_port: u16,

    /// Allocated P2P port
    pub p2p_port: u16,

    /// Data directory path
    pub data_dir: std::path::PathBuf,
}

impl TestDaemon {
    /// Create new test daemon with random ports and temporary storage
    ///
    /// This allocates ports using the OS and creates a temporary directory
    /// that will be automatically cleaned up when the TestDaemon is dropped.
    pub fn new_random() -> Self {
        let temp_dir = TempDir::new("tos_test_daemon").expect("Failed to create temp directory");

        // Allocate random ports using OS
        let rpc_listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind RPC port");
        let rpc_port = rpc_listener.local_addr().unwrap().port();
        drop(rpc_listener);

        let p2p_listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind P2P port");
        let p2p_port = p2p_listener.local_addr().unwrap().port();
        drop(p2p_listener);

        let data_dir = temp_dir.path().to_path_buf();

        Self {
            _temp_dir: temp_dir,
            rpc_port,
            p2p_port,
            data_dir,
        }
    }

    /// Start the daemon
    ///
    /// **Note**: This is a placeholder for MVP. Full implementation requires
    /// daemon API refactoring.
    #[allow(clippy::unimplemented)]
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement daemon start
        // This will require:
        // 1. Create DaemonConfig with self.data_dir, self.rpc_port, self.p2p_port
        // 2. Initialize Daemon instance
        // 3. Start daemon async task
        // 4. Wait for initialization
        // 5. Return RPC client

        unimplemented!(
            "TestDaemon::start() requires daemon API refactoring - use MockStorage for MVP tests"
        )
    }

    /// Shutdown the daemon
    pub async fn shutdown(&mut self) {
        // TODO: Implement graceful shutdown
        // Daemon will be stopped when dropped, but explicit shutdown is cleaner
    }

    /// Get RPC endpoint URL
    pub fn rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.rpc_port)
    }

    /// Get P2P endpoint address
    pub fn p2p_address(&self) -> String {
        format!("127.0.0.1:{}", self.p2p_port)
    }
}

// Note: Full TestDaemon implementation is deferred to Phase 2
// For Phase 1 (parallel execution tests), MockStorage is sufficient
