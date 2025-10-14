// Test Modules for TOS Daemon Core
// Organizes all test files

#[cfg(test)]
mod performance_tests;

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod property_tests;

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod security_tests;

#[cfg(test)]
mod ghostdag_consensus_tests;

#[cfg(test)]
mod ghostdag_dag_tests;

#[cfg(test)]
mod ghostdag_json_loader;

#[cfg(test)]
mod ghostdag_json_tests;

#[cfg(test)]
mod bps_integration_tests;
