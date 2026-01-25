//! Core types for Transaction Fixture Testing.
//!
//! Defines the declarative input/output format for fixture-based tests.
//! Test authors specify initial state, transaction sequence, expected final
//! state, and invariants. The framework handles setup, execution, and
//! verification automatically.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

/// Parsed fixture definition - the top-level YAML structure.
#[derive(Debug, Clone, Deserialize)]
pub struct Fixture {
    /// Fixture metadata
    pub fixture: FixtureMeta,
    /// Initial state setup
    pub setup: FixtureSetup,
    /// Transaction sequence to execute
    pub transactions: Vec<Step>,
    /// Expected state after all transactions
    pub expected: Option<ExpectedState>,
    /// Invariants to verify
    #[serde(default)]
    pub invariants: Vec<Invariant>,
    /// Fee model configuration
    pub fee_model: Option<FeeModel>,
}

/// Fixture metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureMeta {
    /// Human-readable fixture name
    pub name: String,
    /// Fixture format version
    pub version: Option<String>,
    /// Description of what this fixture tests
    pub description: Option<String>,
    /// Which tiers this fixture can run on (1, 2, 3)
    #[serde(default = "default_tiers")]
    pub tier: Vec<u8>,
}

fn default_tiers() -> Vec<u8> {
    vec![1, 2, 3]
}

/// Initial state setup for a fixture.
#[derive(Debug, Clone, Deserialize)]
pub struct FixtureSetup {
    /// Network-level configuration
    pub network: Option<NetworkConfig>,
    /// Asset definitions (UNO tokens)
    pub assets: Option<Vec<AssetDef>>,
    /// Account definitions with initial state
    pub accounts: HashMap<String, AccountDef>,
}

/// Network-level configuration for fixture setup.
#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    /// Total energy weight in the network
    pub total_energy_weight: Option<String>,
    /// Total energy limit
    pub total_energy_limit: Option<u64>,
    /// Block time duration
    pub block_time: Option<String>,
}

/// Asset (UNO token) definition.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetDef {
    /// Asset identifier
    pub id: String,
    /// Human-readable name
    pub name: Option<String>,
    /// Total supply
    pub supply: String,
    /// Decimal places
    pub decimals: Option<u8>,
}

/// Account definition with initial state.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountDef {
    /// Initial TOS balance
    pub balance: String,
    /// Initial nonce
    pub nonce: Option<u64>,
    /// UNO token balances (asset_id -> amount)
    pub uno_balances: Option<HashMap<String, u64>>,
    /// Frozen TOS balance
    pub frozen_balance: Option<String>,
    /// Energy state
    pub energy: Option<EnergyDef>,
    /// Outgoing delegations (delegate_to -> amount)
    pub delegations_out: Option<HashMap<String, String>>,
    /// Incoming delegations (delegator -> amount)
    pub delegations_in: Option<HashMap<String, String>>,
    /// Template name to inherit from
    pub template: Option<String>,
}

/// Energy state definition.
#[derive(Debug, Clone, Deserialize)]
pub struct EnergyDef {
    /// Energy limit
    pub limit: Option<u64>,
    /// Current usage
    pub usage: Option<u64>,
    /// Available energy
    pub available: Option<u64>,
    /// Last usage timestamp
    pub last_usage_time: Option<u64>,
}

/// A single step in the transaction sequence.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Step {
    /// A transaction step
    Transaction(Box<TransactionStep>),
    /// A checkpoint for intermediate verification
    Checkpoint(CheckpointStep),
}

/// A transaction step to execute.
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionStep {
    /// Step number (for ordering)
    pub step: Option<u32>,
    /// Human-readable step name
    pub name: Option<String>,
    /// Transaction type
    #[serde(rename = "type")]
    pub tx_type: TransactionType,
    /// Sender account name
    pub from: Option<String>,
    /// Receiver account name
    pub to: Option<String>,
    /// Amount to transfer/freeze/delegate
    pub amount: Option<String>,
    /// Fee for this transaction
    pub fee: Option<String>,
    /// UNO asset identifier
    pub asset: Option<String>,
    /// Explicit nonce override
    pub nonce: Option<u64>,
    /// Time duration for advance_time steps
    pub duration: Option<String>,
    /// Contract code for deploy
    pub code: Option<String>,
    /// Contract address for calls
    pub contract: Option<String>,
    /// Function name for contract calls
    pub function: Option<String>,
    /// Arguments for contract calls
    pub args: Option<Vec<String>>,
    /// Expected transaction status
    #[serde(default = "default_expect_success")]
    pub expect_status: ExpectStatus,
    /// Expected error type (when expect_status = error)
    pub expect_error: Option<String>,
}

fn default_expect_success() -> ExpectStatus {
    ExpectStatus::Success
}

/// Transaction types supported by fixture testing.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionType {
    /// TOS native transfer
    Transfer,
    /// UNO asset transfer
    UnoTransfer,
    /// Freeze TOS for energy
    Freeze,
    /// Unfreeze TOS
    Unfreeze,
    /// Delegate frozen TOS
    Delegate,
    /// Remove delegation
    Undelegate,
    /// Register new account
    Register,
    /// Mine a block
    MineBlock,
    /// Advance clock time
    AdvanceTime,
    /// Deploy TAKO contract
    DeployContract,
    /// Call existing contract
    CallContract,
}

/// Expected transaction status.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpectStatus {
    /// Transaction should succeed
    Success,
    /// Transaction should fail with error
    Error,
}

/// Intermediate checkpoint for state verification.
#[derive(Debug, Clone, Deserialize)]
pub struct CheckpointStep {
    /// Checkpoint name
    pub checkpoint: String,
    /// Whether to mine a block at this checkpoint
    pub mine_block: Option<bool>,
    /// State to verify at this point
    pub verify: Option<HashMap<String, AccountExpected>>,
}

/// Expected final state after fixture execution.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedState {
    /// Expected account states
    pub accounts: HashMap<String, AccountExpected>,
}

/// Expected account state for verification.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountExpected {
    /// Expected TOS balance
    pub balance: Option<String>,
    /// Expected nonce
    pub nonce: Option<u64>,
    /// Expected UNO balances
    pub uno_balances: Option<HashMap<String, u64>>,
    /// Expected frozen balance
    pub frozen_balance: Option<String>,
    /// Expected energy state
    pub energy: Option<EnergyExpected>,
    /// Expected outgoing delegations
    pub delegations_out: Option<HashMap<String, String>>,
    /// Expected incoming delegations
    pub delegations_in: Option<HashMap<String, String>>,
}

/// Expected energy state for verification.
#[derive(Debug, Clone, Deserialize)]
pub struct EnergyExpected {
    /// Expected energy limit
    pub limit: Option<u64>,
    /// Expected usage
    pub usage: Option<u64>,
    /// Expected available energy
    pub available: Option<u64>,
}

/// Invariant definitions for fixture verification.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Invariant {
    /// Total balance conservation check
    BalanceConservation {
        /// Balance conservation parameters
        balance_conservation: BalanceConservationDef,
    },
    /// Nonce monotonicity check
    NonceMonotonicity {
        /// Whether to check nonce monotonicity
        nonce_monotonicity: bool,
    },
    /// Energy weight consistency check
    EnergyWeightConsistency {
        /// Whether to check energy weight consistency
        energy_weight_consistency: bool,
    },
    /// UNO asset supply conservation check
    UnoSupplyConservation {
        /// UNO supply conservation parameters
        uno_supply_conservation: UnoSupplyDef,
    },
    /// Delegation bounds check
    DelegationBounds {
        /// Delegation bounds parameters
        delegation_bounds: DelegationBoundsDef,
    },
    /// No negative balances check
    NoNegativeBalances {
        /// Whether to check for negative balances
        no_negative_balances: bool,
    },
    /// Receiver account registration check
    ReceiverRegistered {
        /// Whether to check receivers are registered
        receiver_registered: bool,
    },
    /// Custom invariant with formula
    Custom {
        /// Custom invariant definition
        custom: CustomInvariantDef,
    },
}

/// Balance conservation invariant parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct BalanceConservationDef {
    /// Where fees go (e.g., "miner" or "burned")
    pub fee_recipient: Option<String>,
    /// Expected total supply change (due to fees)
    pub total_supply_change: Option<String>,
}

/// UNO supply conservation invariant parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct UnoSupplyDef {
    /// Asset to check
    pub asset: String,
    /// Expected total supply across all holders
    pub total: u64,
}

/// Delegation bounds invariant parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct DelegationBoundsDef {
    /// Account to check
    pub account: Option<String>,
    /// Maximum delegation amount
    pub max_delegation: Option<String>,
}

/// Custom invariant definition.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomInvariantDef {
    /// Invariant name
    pub name: String,
    /// Formula/expression to evaluate
    pub formula: Option<String>,
}

/// Fee model configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeeModel {
    /// Fixed fee per transaction
    Fixed {
        /// Base fee amount
        base_fee: String,
    },
    /// Calculated fee based on TX properties
    Calculated {
        /// Base fee
        base_fee: String,
        /// Fee per byte of TX data
        per_byte: Option<String>,
        /// Additional fee per contract call
        per_contract_call: Option<String>,
    },
    /// No fees (for internal logic testing)
    None,
}

/// Result of executing a single step.
#[derive(Debug, Clone)]
pub struct StepResult {
    /// Step number
    pub step: Option<u32>,
    /// Whether the step succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Error code if failed
    pub error_code: Option<String>,
    /// State changes caused by this step
    pub state_changes: Vec<StateChange>,
}

/// A state change caused by a transaction step.
#[derive(Debug, Clone)]
pub enum StateChange {
    /// Balance changed
    BalanceChange {
        /// Account name
        account: String,
        /// Previous balance
        old_value: u64,
        /// New balance
        new_value: u64,
    },
    /// Nonce changed
    NonceChange {
        /// Account name
        account: String,
        /// Previous nonce
        old_value: u64,
        /// New nonce
        new_value: u64,
    },
    /// UNO balance changed
    UnoBalanceChange {
        /// Account name
        account: String,
        /// Asset ID
        asset: String,
        /// Previous balance
        old_value: u64,
        /// New balance
        new_value: u64,
    },
    /// Frozen balance changed
    FrozenBalanceChange {
        /// Account name
        account: String,
        /// Previous frozen balance
        old_value: u64,
        /// New frozen balance
        new_value: u64,
    },
}

/// Result of a complete fixture execution.
#[derive(Debug, Clone)]
pub struct FixtureResult {
    /// Fixture name
    pub fixture_name: String,
    /// Whether the fixture passed
    pub success: bool,
    /// Per-step results
    pub step_results: Vec<StepResult>,
    /// Verification errors (if any)
    pub verification_errors: Vec<String>,
    /// Invariant errors (if any)
    pub invariant_errors: Vec<String>,
    /// Final account states
    pub final_state: HashMap<String, AccountState>,
}

impl FixtureResult {
    /// Create a successful fixture result.
    pub fn success(name: &str, final_state: HashMap<String, AccountState>) -> Self {
        Self {
            fixture_name: name.to_string(),
            success: true,
            step_results: Vec::new(),
            verification_errors: Vec::new(),
            invariant_errors: Vec::new(),
            final_state,
        }
    }

    /// Create a failed fixture result.
    pub fn failure(name: &str, errors: Vec<String>) -> Self {
        Self {
            fixture_name: name.to_string(),
            success: false,
            step_results: Vec::new(),
            verification_errors: errors,
            invariant_errors: Vec::new(),
            final_state: HashMap::new(),
        }
    }

    /// Check if all verifications passed.
    pub fn all_passed(&self) -> bool {
        self.success && self.verification_errors.is_empty() && self.invariant_errors.is_empty()
    }
}

/// Captured account state at a point in time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountState {
    /// TOS balance (in atomic units)
    pub balance: u64,
    /// Current nonce
    pub nonce: u64,
    /// UNO token balances
    pub uno_balances: HashMap<String, u64>,
    /// Frozen TOS balance
    pub frozen_balance: u64,
    /// Energy limit
    pub energy_limit: u64,
    /// Energy usage
    pub energy_usage: u64,
    /// Outgoing delegations
    pub delegations_out: HashMap<String, u64>,
    /// Incoming delegations
    pub delegations_in: HashMap<String, u64>,
}

/// Result of cross-tier fixture execution.
#[derive(Debug, Clone)]
pub struct CrossTierResult {
    /// Tier 1 result
    pub tier1: Option<FixtureResult>,
    /// Tier 1.5 result
    pub tier1_5: Option<FixtureResult>,
    /// Tier 2 result
    pub tier2: Option<FixtureResult>,
    /// Tier 3 result
    pub tier3: Option<FixtureResult>,
    /// Whether all tiers produced consistent results
    pub consistent: bool,
    /// Inconsistency details (if any)
    pub inconsistencies: Vec<String>,
}

/// Execution tier for fixture backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Tier 1: In-memory TestBlockchain
    Component = 1,
    /// Tier 1.5: ChainClient (direct blockchain access)
    ChainClient = 15,
    /// Tier 2: Single TestDaemon with storage
    Integration = 2,
    /// Tier 3: Multi-node LocalCluster
    E2E = 3,
}

/// Energy state query result from backends.
#[derive(Debug, Clone, Default)]
pub struct EnergyState {
    /// Energy limit
    pub limit: u64,
    /// Current usage
    pub usage: u64,
    /// Available energy (limit - usage, accounting for recovery)
    pub available: u64,
}

/// Delegation map type (account -> amount).
pub type DelegationMap = HashMap<String, u64>;

/// Parse a TOS amount string into atomic units.
///
/// Supports formats:
/// - "1_000 TOS" -> 100_000 (assuming 3 decimals for simplicity)
/// - "10_000" -> 10_000 (raw value)
/// - "0 TOS" -> 0
pub fn parse_amount(s: &str) -> Result<u64, String> {
    let s = s.trim();

    // Strip "TOS" suffix if present
    let numeric = if let Some(stripped) = s.strip_suffix("TOS") {
        stripped.trim()
    } else {
        s
    };

    // Remove underscores for readability
    let cleaned: String = numeric.chars().filter(|c| *c != '_').collect();

    cleaned
        .parse::<u64>()
        .map_err(|e| format!("Failed to parse amount '{}': {}", s, e))
}

/// Parse a duration string into Duration.
///
/// Supports: "6h", "30m", "15s", "100ms"
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();

    if let Some(hours) = s.strip_suffix('h') {
        let h: u64 = hours
            .parse()
            .map_err(|e| format!("Invalid hours '{}': {}", hours, e))?;
        Ok(Duration::from_secs(h.saturating_mul(3600)))
    } else if let Some(minutes) = s.strip_suffix('m') {
        let m: u64 = minutes
            .parse()
            .map_err(|e| format!("Invalid minutes '{}': {}", minutes, e))?;
        Ok(Duration::from_secs(m.saturating_mul(60)))
    } else if let Some(ms) = s.strip_suffix("ms") {
        let millis: u64 = ms
            .parse()
            .map_err(|e| format!("Invalid milliseconds '{}': {}", ms, e))?;
        Ok(Duration::from_millis(millis))
    } else if let Some(secs) = s.strip_suffix('s') {
        let sec: u64 = secs
            .parse()
            .map_err(|e| format!("Invalid seconds '{}': {}", secs, e))?;
        Ok(Duration::from_secs(sec))
    } else {
        Err(format!(
            "Unknown duration format '{}': expected suffix h/m/s/ms",
            s
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_amount_with_tos() {
        assert_eq!(parse_amount("1_000 TOS").unwrap(), 1_000);
        assert_eq!(parse_amount("10_000 TOS").unwrap(), 10_000);
        assert_eq!(parse_amount("0 TOS").unwrap(), 0);
    }

    #[test]
    fn test_parse_amount_raw() {
        assert_eq!(parse_amount("1000").unwrap(), 1000);
        assert_eq!(parse_amount("10_000").unwrap(), 10_000);
    }

    #[test]
    fn test_parse_amount_errors() {
        assert!(parse_amount("abc").is_err());
        assert!(parse_amount("-100 TOS").is_err());
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("6h").unwrap(), Duration::from_secs(21600));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("15s").unwrap(), Duration::from_secs(15));
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
    }

    #[test]
    fn test_parse_duration_errors() {
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn test_fixture_result_success() {
        let result = FixtureResult::success("test", HashMap::new());
        assert!(result.all_passed());
    }

    #[test]
    fn test_fixture_result_failure() {
        let result = FixtureResult::failure("test", vec!["error".to_string()]);
        assert!(!result.all_passed());
    }

    #[test]
    fn test_account_state_default() {
        let state = AccountState::default();
        assert_eq!(state.balance, 0);
        assert_eq!(state.nonce, 0);
        assert_eq!(state.frozen_balance, 0);
    }

    #[test]
    fn test_step_deserialization() {
        let yaml = r#"
step: 1
name: "Test transfer"
type: transfer
from: alice
to: bob
amount: "1000 TOS"
fee: "10 TOS"
expect_status: success
"#;
        let step: TransactionStep = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(step.step, Some(1));
        assert_eq!(step.tx_type, TransactionType::Transfer);
        assert_eq!(step.from.as_deref(), Some("alice"));
    }

    #[test]
    fn test_fixture_meta_default_tiers() {
        let yaml = r#"
name: "test"
"#;
        let meta: FixtureMeta = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.tier, vec![1, 2, 3]);
    }
}
