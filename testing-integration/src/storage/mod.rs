//! Mock storage backend for testing
//!
//! This module provides an in-memory storage implementation that avoids
//! the sled deadlock issues encountered in tests.

mod mock_storage;

pub use mock_storage::MockStorage;
