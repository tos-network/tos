//! Transaction result types for the ChainClient (Tier 1.5) testing layer.
//!
//! Provides structured error types, inner call tracing, simulation results,
//! and state diff tracking for deterministic transaction testing.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use tos_common::crypto::Hash;

/// Structured error type for transaction execution failures.
/// Enables precise pattern matching in test assertions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum TransactionError {
    /// Transaction signature verification failed
    InvalidSignature,
    /// Referenced account does not exist in state
    AccountNotFound { address: Hash },
    /// Sender lacks sufficient balance for transfer + fee
    InsufficientBalance { have: u64, need: u64, asset: Hash },
    /// Transaction nonce does not match expected value
    InvalidNonce { expected: u64, provided: u64 },
    /// Transaction with this hash was already processed
    AlreadyProcessed { tx_hash: Hash },
    /// Contract execution exceeded gas limit
    OutOfGas { used: u64, limit: u64 },
    /// Contract execution returned an error
    ContractError {
        contract: Hash,
        exit_code: u32,
        message: String,
    },
    /// Target contract does not exist
    ContractNotFound { address: Hash },
    /// Deployed bytecode failed validation
    InvalidBytecode { reason: String },
    /// A nested contract call failed
    InnerCallFailed {
        caller: Hash,
        callee: Hash,
        depth: u32,
        error: Box<TransactionError>,
    },
    /// Arithmetic overflow during execution
    ArithmeticOverflow { operation: String },
    /// Operation requires permission the sender does not have
    PermissionDenied { required: String },
    /// Referenced asset does not exist
    AssetNotFound { asset: Hash },
    /// Transaction structure is invalid
    MalformedTransaction { reason: String },
    /// Transaction uses a feature not yet activated at current height
    FeatureNotActive {
        feature: String,
        activation_height: u64,
    },
    /// Transaction exceeds maximum allowed size
    TransactionTooLarge { size: usize, max: usize },
    /// Application-defined error code
    Custom(u32),
}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "invalid signature"),
            Self::AccountNotFound { address } => {
                write!(f, "account not found: {}", address)
            }
            Self::InsufficientBalance { have, need, asset } => {
                write!(
                    f,
                    "insufficient balance: have {}, need {}, asset {}",
                    have, need, asset
                )
            }
            Self::InvalidNonce { expected, provided } => {
                write!(f, "invalid nonce: expected {}, got {}", expected, provided)
            }
            Self::AlreadyProcessed { tx_hash } => {
                write!(f, "transaction already processed: {}", tx_hash)
            }
            Self::OutOfGas { used, limit } => {
                write!(f, "out of gas: used {}, limit {}", used, limit)
            }
            Self::ContractError {
                contract,
                exit_code,
                message,
            } => {
                write!(
                    f,
                    "contract {} error (code {}): {}",
                    contract, exit_code, message
                )
            }
            Self::ContractNotFound { address } => {
                write!(f, "contract not found: {}", address)
            }
            Self::InvalidBytecode { reason } => {
                write!(f, "invalid bytecode: {}", reason)
            }
            Self::InnerCallFailed {
                caller,
                callee,
                depth,
                error,
            } => {
                write!(
                    f,
                    "inner call failed: {} -> {} at depth {}: {}",
                    caller, callee, depth, error
                )
            }
            Self::ArithmeticOverflow { operation } => {
                write!(f, "arithmetic overflow in: {}", operation)
            }
            Self::PermissionDenied { required } => {
                write!(f, "permission denied: requires {}", required)
            }
            Self::AssetNotFound { asset } => {
                write!(f, "asset not found: {}", asset)
            }
            Self::MalformedTransaction { reason } => {
                write!(f, "malformed transaction: {}", reason)
            }
            Self::FeatureNotActive {
                feature,
                activation_height,
            } => {
                write!(
                    f,
                    "feature '{}' not active until height {}",
                    feature, activation_height
                )
            }
            Self::TransactionTooLarge { size, max } => {
                write!(f, "transaction too large: {} bytes (max {})", size, max)
            }
            Self::Custom(code) => write!(f, "custom error: {}", code),
        }
    }
}

impl std::error::Error for TransactionError {}

/// A traced inner (cross-contract) call during transaction execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnerCall {
    /// Contract that initiated the call
    pub caller: Hash,
    /// Contract being called
    pub callee: Hash,
    /// Entry point identifier invoked on the callee
    pub entry_id: u16,
    /// Serialized call arguments
    pub data: Vec<u8>,
    /// Assets deposited with the call
    pub deposits: Vec<CallDeposit>,
    /// Gas consumed by this call (excluding sub-calls)
    pub gas_used: u64,
    /// Whether the call completed successfully
    pub success: bool,
    /// Nesting depth (0 = direct call from transaction)
    pub depth: u32,
    /// Return data from the call
    pub return_data: Vec<u8>,
    /// Events emitted during this call
    pub events: Vec<ContractEvent>,
}

/// A deposit made as part of a contract call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallDeposit {
    /// Asset hash (native TOS uses Hash::zero())
    pub asset: Hash,
    /// Amount deposited
    pub amount: u64,
}

/// An event emitted by a contract during execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractEvent {
    /// Contract that emitted the event
    pub contract: Hash,
    /// Event topic/identifier
    pub topic: String,
    /// Serialized event data
    pub data: Vec<u8>,
}

/// Result of processing a transaction through ChainClient.
#[derive(Debug, Clone)]
pub struct TxResult {
    /// Whether the transaction executed successfully
    pub success: bool,
    /// Transaction hash
    pub tx_hash: Hash,
    /// Block hash containing this transaction (if mined)
    pub block_hash: Option<Hash>,
    /// Topoheight at which the transaction was included
    pub topoheight: Option<u64>,
    /// Structured error (None if success)
    pub error: Option<TransactionError>,
    /// Gas consumed by execution
    pub gas_used: u64,
    /// Gas refunded after execution
    pub gas_refunded: u64,
    /// Exit code from contract execution (None for non-contract transactions)
    pub exit_code: Option<u32>,
    /// Events emitted during execution
    pub events: Vec<ContractEvent>,
    /// Log messages produced during execution
    pub log_messages: Vec<String>,
    /// Traced inner (cross-contract) calls
    pub inner_calls: Vec<InnerCall>,
    /// Return data from contract execution
    pub return_data: Vec<u8>,
    /// Sender nonce after this transaction
    pub new_nonce: u64,
}

impl TxResult {
    /// Returns true if the transaction executed without error.
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// Returns the error if the transaction failed.
    pub fn error(&self) -> Option<&TransactionError> {
        self.error.as_ref()
    }

    /// Asserts the transaction succeeded, panics with error details if not.
    pub fn assert_success(&self) {
        assert!(
            self.success,
            "Expected transaction success, got error: {:?}",
            self.error
        );
    }

    /// Asserts the transaction failed with a specific error variant.
    pub fn assert_error(&self, expected: &TransactionError) {
        assert!(
            !self.success,
            "Expected transaction failure, but it succeeded"
        );
        assert_eq!(
            self.error.as_ref(),
            Some(expected),
            "Error mismatch: expected {:?}, got {:?}",
            expected,
            self.error
        );
    }

    /// Asserts the transaction failed (any error).
    pub fn assert_failed(&self) {
        assert!(
            !self.success,
            "Expected transaction failure, but it succeeded"
        );
    }

    /// Returns events matching a specific topic.
    pub fn events_by_topic(&self, topic: &str) -> Vec<&ContractEvent> {
        self.events.iter().filter(|e| e.topic == topic).collect()
    }

    /// Returns inner calls to a specific contract.
    pub fn calls_to(&self, contract: &Hash) -> Vec<&InnerCall> {
        self.inner_calls
            .iter()
            .filter(|c| &c.callee == contract)
            .collect()
    }

    /// Returns the maximum call depth reached.
    pub fn max_call_depth(&self) -> u32 {
        self.inner_calls.iter().map(|c| c.depth).max().unwrap_or(0)
    }
}

/// Result of simulating a transaction without committing state.
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// Whether the simulation completed without error
    pub success: bool,
    /// Error if simulation failed
    pub error: Option<TransactionError>,
    /// Estimated gas that would be consumed
    pub gas_used: u64,
    /// Events that would be emitted
    pub events: Vec<ContractEvent>,
    /// Log messages that would be produced
    pub log_messages: Vec<String>,
    /// Inner calls that would occur
    pub inner_calls: Vec<InnerCall>,
    /// Return data from execution
    pub return_data: Vec<u8>,
    /// State changes that would occur (if tracking enabled)
    pub state_diff: Option<StateDiff>,
}

impl SimulationResult {
    /// Returns true if the simulation completed without error.
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// Asserts the simulation would succeed.
    pub fn assert_success(&self) {
        assert!(
            self.success,
            "Expected simulation success, got error: {:?}",
            self.error
        );
    }

    /// Asserts the simulation would fail.
    pub fn assert_failed(&self) {
        assert!(
            !self.success,
            "Expected simulation failure, but it would succeed"
        );
    }
}

/// Result of a contract call with gas breakdown.
#[derive(Debug, Clone)]
pub struct ContractCallResult {
    /// The underlying transaction result
    pub tx_result: TxResult,
    /// Parsed return data (if available)
    pub decoded_return: Option<Vec<u8>>,
    /// Gas usage breakdown
    pub gas_breakdown: GasBreakdown,
}

/// Breakdown of gas usage in a contract call.
#[derive(Debug, Clone, Default)]
pub struct GasBreakdown {
    /// Total gas used
    pub total_used: u64,
    /// Gas burned (removed from supply)
    pub burned: u64,
    /// Gas paid to block miner
    pub miner_fee: u64,
    /// Gas refunded to caller
    pub refunded: u64,
}

/// Diff of state changes from a transaction or simulation.
#[derive(Debug, Clone, Default)]
pub struct StateDiff {
    /// Per-account state changes
    pub changes: HashMap<Hash, Vec<StateChange>>,
}

impl StateDiff {
    /// Returns true if no state changes occurred.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns all changes for a specific account.
    pub fn changes_for(&self, account: &Hash) -> &[StateChange] {
        self.changes.get(account).map_or(&[], |v| v.as_slice())
    }

    /// Returns the number of accounts affected.
    pub fn affected_accounts(&self) -> usize {
        self.changes.len()
    }
}

/// Individual state change within a StateDiff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum StateChange {
    /// Balance changed
    BalanceChange {
        asset: Hash,
        before: u64,
        after: u64,
    },
    /// Nonce incremented
    NonceChange { before: u64, after: u64 },
    /// Contract storage key modified
    StorageWrite {
        key: Vec<u8>,
        old_value: Option<Vec<u8>>,
        new_value: Vec<u8>,
    },
    /// Contract storage key deleted
    StorageDelete { key: Vec<u8>, old_value: Vec<u8> },
    /// Contract deployed
    ContractDeployed { code_hash: Hash },
    /// Frozen balance changed
    FrozenBalanceChange { before: u64, after: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    #[test]
    fn test_tx_result_success() {
        let result = TxResult {
            success: true,
            tx_hash: sample_hash(1),
            block_hash: Some(sample_hash(2)),
            topoheight: Some(100),
            error: None,
            gas_used: 5000,
            gas_refunded: 0,
            exit_code: None,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            new_nonce: 1,
        };
        assert!(result.is_success());
        result.assert_success();
    }

    #[test]
    fn test_tx_result_failure() {
        let result = TxResult {
            success: false,
            tx_hash: sample_hash(1),
            block_hash: None,
            topoheight: None,
            error: Some(TransactionError::InsufficientBalance {
                have: 100,
                need: 200,
                asset: Hash::zero(),
            }),
            gas_used: 0,
            gas_refunded: 0,
            exit_code: None,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            new_nonce: 0,
        };
        assert!(!result.is_success());
        result.assert_failed();
    }

    #[test]
    fn test_tx_result_error_matching() {
        let error = TransactionError::InvalidNonce {
            expected: 5,
            provided: 3,
        };
        let result = TxResult {
            success: false,
            tx_hash: sample_hash(1),
            block_hash: None,
            topoheight: None,
            error: Some(error.clone()),
            gas_used: 0,
            gas_refunded: 0,
            exit_code: None,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            new_nonce: 0,
        };
        result.assert_error(&error);
    }

    #[test]
    fn test_inner_call_filtering() {
        let contract_a = sample_hash(10);
        let contract_b = sample_hash(20);
        let result = TxResult {
            success: true,
            tx_hash: sample_hash(1),
            block_hash: Some(sample_hash(2)),
            topoheight: Some(50),
            error: None,
            gas_used: 10000,
            gas_refunded: 0,
            exit_code: None,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![
                InnerCall {
                    caller: sample_hash(0),
                    callee: contract_a.clone(),
                    entry_id: 1,
                    data: vec![],
                    deposits: vec![],
                    gas_used: 3000,
                    success: true,
                    depth: 0,
                    return_data: vec![],
                    events: vec![],
                },
                InnerCall {
                    caller: contract_a.clone(),
                    callee: contract_b.clone(),
                    entry_id: 2,
                    data: vec![],
                    deposits: vec![],
                    gas_used: 2000,
                    success: true,
                    depth: 1,
                    return_data: vec![],
                    events: vec![],
                },
            ],
            return_data: vec![],
            new_nonce: 1,
        };
        assert_eq!(result.calls_to(&contract_a).len(), 1);
        assert_eq!(result.calls_to(&contract_b).len(), 1);
        assert_eq!(result.max_call_depth(), 1);
    }

    #[test]
    fn test_event_filtering() {
        let result = TxResult {
            success: true,
            tx_hash: sample_hash(1),
            block_hash: Some(sample_hash(2)),
            topoheight: Some(50),
            error: None,
            gas_used: 5000,
            gas_refunded: 0,
            exit_code: None,
            events: vec![
                ContractEvent {
                    contract: sample_hash(10),
                    topic: "Transfer".to_string(),
                    data: vec![1, 2, 3],
                },
                ContractEvent {
                    contract: sample_hash(10),
                    topic: "Approval".to_string(),
                    data: vec![4, 5, 6],
                },
                ContractEvent {
                    contract: sample_hash(10),
                    topic: "Transfer".to_string(),
                    data: vec![7, 8, 9],
                },
            ],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            new_nonce: 1,
        };
        assert_eq!(result.events_by_topic("Transfer").len(), 2);
        assert_eq!(result.events_by_topic("Approval").len(), 1);
        assert_eq!(result.events_by_topic("Unknown").len(), 0);
    }

    #[test]
    fn test_simulation_result() {
        let sim = SimulationResult {
            success: true,
            error: None,
            gas_used: 8000,
            events: vec![],
            log_messages: vec!["debug: entered main".to_string()],
            inner_calls: vec![],
            return_data: vec![42],
            state_diff: Some(StateDiff::default()),
        };
        assert!(sim.is_success());
        sim.assert_success();
    }

    #[test]
    fn test_state_diff() {
        let mut diff = StateDiff::default();
        assert!(diff.is_empty());

        let account = sample_hash(1);
        diff.changes.insert(
            account.clone(),
            vec![
                StateChange::BalanceChange {
                    asset: Hash::zero(),
                    before: 1000,
                    after: 800,
                },
                StateChange::NonceChange {
                    before: 5,
                    after: 6,
                },
            ],
        );

        assert!(!diff.is_empty());
        assert_eq!(diff.affected_accounts(), 1);
        assert_eq!(diff.changes_for(&account).len(), 2);
        assert_eq!(diff.changes_for(&sample_hash(99)).len(), 0);
    }

    #[test]
    fn test_transaction_error_display() {
        let err = TransactionError::InsufficientBalance {
            have: 100,
            need: 200,
            asset: Hash::zero(),
        };
        let display = format!("{}", err);
        assert!(display.contains("insufficient balance"));
        assert!(display.contains("100"));
        assert!(display.contains("200"));
    }

    #[test]
    fn test_nested_transaction_error() {
        let inner_err = TransactionError::OutOfGas {
            used: 50000,
            limit: 40000,
        };
        let outer_err = TransactionError::InnerCallFailed {
            caller: sample_hash(1),
            callee: sample_hash(2),
            depth: 1,
            error: Box::new(inner_err),
        };
        let display = format!("{}", outer_err);
        assert!(display.contains("inner call failed"));
        assert!(display.contains("depth 1"));
    }
}
