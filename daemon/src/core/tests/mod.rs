// Test Modules for TOS Daemon Core
// Organizes all test files

#[cfg(test)]
mod performance_tests;

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod property_tests;

// Mock storage for integration tests
// Note: Disabled until full Storage trait is stabilized
// #[cfg(test)]
// mod mock_storage;
