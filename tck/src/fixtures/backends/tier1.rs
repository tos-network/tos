//! Tier 1 backend: TestBlockchain-based fixture execution.
//!
//! Fast, in-memory execution with no network overhead.
//! Provides the fastest feedback loop for fixture tests.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::fixtures::backend::FixtureBackend;
use crate::fixtures::types::{
    DelegationMap, EnergyState, FixtureSetup, StepResult, TransactionStep, TransactionType,
};
use crate::tier1_component::{TestBlockchain, TestBlockchainBuilder, TestTransaction};
use tos_common::crypto::Hash;

/// Tier 1 fixture backend using TestBlockchain.
pub struct TestBlockchainBackend {
    /// The underlying test blockchain
    blockchain: Option<TestBlockchain>,
    /// Account name -> Hash mapping
    accounts: HashMap<String, Hash>,
    /// Locally-tracked balances (since TestBlockchain only handles transfers)
    local_balances: HashMap<Hash, u64>,
    /// Locally-tracked nonces
    local_nonces: HashMap<Hash, u64>,
    /// UNO balances (account_name -> (asset_id -> balance))
    uno_balances: HashMap<String, HashMap<String, u64>>,
    /// Frozen balances (account_name -> balance)
    frozen_balances: HashMap<String, u64>,
    /// Energy state (account_name -> EnergyState)
    energy_states: HashMap<String, EnergyState>,
    /// Delegations out (account_name -> (delegate_to -> amount))
    delegations_out: HashMap<String, HashMap<String, u64>>,
    /// Delegations in (account_name -> (delegator -> amount))
    delegations_in: HashMap<String, HashMap<String, u64>>,
    /// Nonce counter for generating unique hashes
    tx_counter: u64,
}

impl TestBlockchainBackend {
    /// Create a new Tier 1 backend (use setup() to initialize state).
    pub fn new() -> Self {
        Self {
            blockchain: None,
            accounts: HashMap::new(),
            local_balances: HashMap::new(),
            local_nonces: HashMap::new(),
            uno_balances: HashMap::new(),
            frozen_balances: HashMap::new(),
            energy_states: HashMap::new(),
            delegations_out: HashMap::new(),
            delegations_in: HashMap::new(),
            tx_counter: 0,
        }
    }

    /// Generate a deterministic Hash from account name.
    fn name_to_hash(name: &str) -> Hash {
        let mut bytes = [0u8; 32];
        let name_bytes = name.as_bytes();
        let copy_len = name_bytes.len().min(32);
        bytes[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        Hash::new(bytes)
    }

    /// Generate a unique transaction hash.
    fn next_tx_hash(&mut self) -> Hash {
        self.tx_counter = self.tx_counter.saturating_add(1);
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&self.tx_counter.to_le_bytes());
        Hash::new(bytes)
    }

    /// Look up account Hash by name.
    fn resolve_account(&self, name: &str) -> Result<Hash> {
        self.accounts
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown account: {}", name))
    }

    /// Get local balance for an account hash.
    fn get_local_balance(&self, hash: &Hash) -> u64 {
        self.local_balances.get(hash).copied().unwrap_or(0)
    }

    /// Get local nonce for an account hash (stored nonce, starts at 0).
    fn get_local_nonce(&self, hash: &Hash) -> u64 {
        self.local_nonces.get(hash).copied().unwrap_or(0)
    }

    /// Get the next valid transaction nonce (stored_nonce + 1).
    fn next_tx_nonce(&self, hash: &Hash) -> u64 {
        self.get_local_nonce(hash).saturating_add(1)
    }

    /// Set local balance.
    fn set_local_balance(&mut self, hash: &Hash, balance: u64) {
        self.local_balances.insert(hash.clone(), balance);
    }

    /// Increment local nonce.
    fn increment_nonce(&mut self, hash: &Hash) {
        let nonce = self.get_local_nonce(hash);
        self.local_nonces
            .insert(hash.clone(), nonce.saturating_add(1));
    }
}

impl Default for TestBlockchainBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FixtureBackend for TestBlockchainBackend {
    async fn setup(&mut self, setup: &FixtureSetup) -> Result<()> {
        let mut builder = TestBlockchainBuilder::new();

        for (name, account_def) in &setup.accounts {
            let hash = Self::name_to_hash(name);

            let balance = crate::fixtures::types::parse_amount(&account_def.balance)
                .map_err(|e| anyhow!("{}", e))?;
            builder = builder.with_funded_account(hash.clone(), balance);

            // Initialize local state tracking
            self.local_balances.insert(hash.clone(), balance);
            self.local_nonces
                .insert(hash.clone(), account_def.nonce.unwrap_or(0));
            self.accounts.insert(name.clone(), hash);

            // Track UNO balances
            if let Some(uno) = &account_def.uno_balances {
                self.uno_balances.insert(name.clone(), uno.clone());
            }

            // Track frozen balances
            if let Some(frozen_str) = &account_def.frozen_balance {
                let frozen = crate::fixtures::types::parse_amount(frozen_str)
                    .map_err(|e| anyhow!("{}", e))?;
                self.frozen_balances.insert(name.clone(), frozen);
            }

            // Track energy state
            if let Some(energy) = &account_def.energy {
                self.energy_states.insert(
                    name.clone(),
                    EnergyState {
                        limit: energy.limit.unwrap_or(0),
                        usage: energy.usage.unwrap_or(0),
                        available: energy.available.unwrap_or(0),
                    },
                );
            }

            // Track delegations
            if let Some(delegations) = &account_def.delegations_out {
                let mut map = HashMap::new();
                for (to, amount_str) in delegations {
                    let amount = crate::fixtures::types::parse_amount(amount_str)
                        .map_err(|e| anyhow!("{}", e))?;
                    map.insert(to.clone(), amount);
                }
                self.delegations_out.insert(name.clone(), map);
            }
            if let Some(delegations) = &account_def.delegations_in {
                let mut map = HashMap::new();
                for (from, amount_str) in delegations {
                    let amount = crate::fixtures::types::parse_amount(amount_str)
                        .map_err(|e| anyhow!("{}", e))?;
                    map.insert(from.clone(), amount);
                }
                self.delegations_in.insert(name.clone(), map);
            }
        }

        let blockchain = builder.build().await?;
        self.blockchain = Some(blockchain);
        Ok(())
    }

    async fn execute_step(&mut self, step: &TransactionStep) -> Result<StepResult> {
        match &step.tx_type {
            TransactionType::Transfer => {
                let from_name = step.from.as_deref().unwrap_or("");
                let to_name = step.to.as_deref().unwrap_or("");
                let from_hash = self.resolve_account(from_name)?;
                let to_hash = self.resolve_account(to_name)?;

                let amount = step
                    .amount
                    .as_ref()
                    .map(|a| crate::fixtures::types::parse_amount(a))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                let fee = step
                    .fee
                    .as_ref()
                    .map(|f| crate::fixtures::types::parse_amount(f))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                // Check sufficient balance
                let sender_balance = self.get_local_balance(&from_hash);
                let total_cost = amount.saturating_add(fee);
                if sender_balance < total_cost {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient balance".to_string()),
                        error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                // Check zero amount
                if amount == 0 {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Zero amount transfer".to_string()),
                        error_code: Some("ZERO_AMOUNT".to_string()),
                        state_changes: vec![],
                    });
                }

                let nonce = if let Some(n) = step.nonce {
                    n
                } else {
                    self.next_tx_nonce(&from_hash)
                };

                let tx_hash = self.next_tx_hash();
                let tx = TestTransaction {
                    hash: tx_hash,
                    sender: from_hash.clone(),
                    recipient: to_hash.clone(),
                    amount,
                    fee,
                    nonce,
                };

                if let Some(blockchain) = &self.blockchain {
                    match blockchain.submit_transaction(tx).await {
                        Ok(_) => {
                            // Update local state tracking
                            self.set_local_balance(
                                &from_hash,
                                sender_balance.saturating_sub(total_cost),
                            );
                            let receiver_balance = self.get_local_balance(&to_hash);
                            self.set_local_balance(
                                &to_hash,
                                receiver_balance.saturating_add(amount),
                            );
                            self.increment_nonce(&from_hash);

                            Ok(StepResult {
                                step: step.step,
                                success: true,
                                error: None,
                                error_code: None,
                                state_changes: vec![],
                            })
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            let error_code = classify_error(&error_msg);
                            Ok(StepResult {
                                step: step.step,
                                success: false,
                                error: Some(error_msg),
                                error_code: Some(error_code),
                                state_changes: vec![],
                            })
                        }
                    }
                } else {
                    // No blockchain initialized - simulate locally
                    self.set_local_balance(&from_hash, sender_balance.saturating_sub(total_cost));
                    let receiver_balance = self.get_local_balance(&to_hash);
                    self.set_local_balance(&to_hash, receiver_balance.saturating_add(amount));
                    self.increment_nonce(&from_hash);

                    Ok(StepResult {
                        step: step.step,
                        success: true,
                        error: None,
                        error_code: None,
                        state_changes: vec![],
                    })
                }
            }
            TransactionType::Freeze => {
                let from_name = step.from.as_deref().unwrap_or("");
                let amount = step
                    .amount
                    .as_ref()
                    .map(|a| crate::fixtures::types::parse_amount(a))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);
                let fee = step
                    .fee
                    .as_ref()
                    .map(|f| crate::fixtures::types::parse_amount(f))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                let from_hash = self.resolve_account(from_name)?;
                let current_balance = self.get_local_balance(&from_hash);
                let total_cost = amount.saturating_add(fee);

                if current_balance < total_cost {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient balance for freeze".to_string()),
                        error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                // Deduct amount + fee from balance, add to frozen
                self.set_local_balance(&from_hash, current_balance.saturating_sub(total_cost));
                let current_frozen = self.frozen_balances.get(from_name).copied().unwrap_or(0);
                self.frozen_balances
                    .insert(from_name.to_string(), current_frozen.saturating_add(amount));
                self.increment_nonce(&from_hash);

                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::Unfreeze => {
                let from_name = step.from.as_deref().unwrap_or("");
                let amount = step
                    .amount
                    .as_ref()
                    .map(|a| crate::fixtures::types::parse_amount(a))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);
                let fee = step
                    .fee
                    .as_ref()
                    .map(|f| crate::fixtures::types::parse_amount(f))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                let from_hash = self.resolve_account(from_name)?;
                let current_frozen = self.frozen_balances.get(from_name).copied().unwrap_or(0);

                if current_frozen < amount {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient frozen balance".to_string()),
                        error_code: Some("INSUFFICIENT_FROZEN".to_string()),
                        state_changes: vec![],
                    });
                }

                let current_balance = self.get_local_balance(&from_hash);
                if current_balance < fee {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient balance for fee".to_string()),
                        error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                self.frozen_balances
                    .insert(from_name.to_string(), current_frozen.saturating_sub(amount));
                self.set_local_balance(
                    &from_hash,
                    current_balance.saturating_add(amount).saturating_sub(fee),
                );
                self.increment_nonce(&from_hash);

                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::Delegate => {
                let from_name = step.from.as_deref().unwrap_or("");
                let to_name = step.to.as_deref().unwrap_or("");
                let amount = step
                    .amount
                    .as_ref()
                    .map(|a| crate::fixtures::types::parse_amount(a))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);
                let fee = step
                    .fee
                    .as_ref()
                    .map(|f| crate::fixtures::types::parse_amount(f))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                if from_name == to_name {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Self-delegation not allowed".to_string()),
                        error_code: Some("SELF_DELEGATION".to_string()),
                        state_changes: vec![],
                    });
                }

                let from_hash = self.resolve_account(from_name)?;
                let current_balance = self.get_local_balance(&from_hash);
                if current_balance < fee {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient balance for fee".to_string()),
                        error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                let current_frozen = self.frozen_balances.get(from_name).copied().unwrap_or(0);
                let current_delegated: u64 = self
                    .delegations_out
                    .get(from_name)
                    .map(|d| d.values().sum())
                    .unwrap_or(0);

                if current_delegated.saturating_add(amount) > current_frozen {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Delegation exceeds frozen balance".to_string()),
                        error_code: Some("EXCEEDS_FROZEN".to_string()),
                        state_changes: vec![],
                    });
                }

                let delegations = self
                    .delegations_out
                    .entry(from_name.to_string())
                    .or_default();
                let existing = delegations.get(to_name).copied().unwrap_or(0);
                delegations.insert(to_name.to_string(), existing.saturating_add(amount));

                let delegations_in = self.delegations_in.entry(to_name.to_string()).or_default();
                let existing_in = delegations_in.get(from_name).copied().unwrap_or(0);
                delegations_in.insert(from_name.to_string(), existing_in.saturating_add(amount));

                self.set_local_balance(&from_hash, current_balance.saturating_sub(fee));
                self.increment_nonce(&from_hash);

                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::Undelegate => {
                let from_name = step.from.as_deref().unwrap_or("");
                let to_name = step.to.as_deref().unwrap_or("");
                let amount = step
                    .amount
                    .as_ref()
                    .map(|a| crate::fixtures::types::parse_amount(a))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);
                let fee = step
                    .fee
                    .as_ref()
                    .map(|f| crate::fixtures::types::parse_amount(f))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                let from_hash = self.resolve_account(from_name)?;
                let current_balance = self.get_local_balance(&from_hash);
                if current_balance < fee {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient balance for fee".to_string()),
                        error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                let current_delegated = self
                    .delegations_out
                    .get(from_name)
                    .and_then(|d| d.get(to_name))
                    .copied()
                    .unwrap_or(0);

                if current_delegated < amount {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Undelegation exceeds delegation".to_string()),
                        error_code: Some("EXCEEDS_DELEGATION".to_string()),
                        state_changes: vec![],
                    });
                }

                if let Some(delegations) = self.delegations_out.get_mut(from_name) {
                    let new_val = current_delegated.saturating_sub(amount);
                    if new_val == 0 {
                        delegations.remove(to_name);
                    } else {
                        delegations.insert(to_name.to_string(), new_val);
                    }
                }
                if let Some(delegations_in) = self.delegations_in.get_mut(to_name) {
                    let existing = delegations_in.get(from_name).copied().unwrap_or(0);
                    let new_val = existing.saturating_sub(amount);
                    if new_val == 0 {
                        delegations_in.remove(from_name);
                    } else {
                        delegations_in.insert(from_name.to_string(), new_val);
                    }
                }

                self.set_local_balance(&from_hash, current_balance.saturating_sub(fee));
                self.increment_nonce(&from_hash);

                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::UnoTransfer => {
                let from_name = step.from.as_deref().unwrap_or("");
                let to_name = step.to.as_deref().unwrap_or("");
                let asset = step.asset.as_deref().unwrap_or("");
                let amount = step
                    .amount
                    .as_ref()
                    .map(|a| crate::fixtures::types::parse_amount(a))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);
                let fee = step
                    .fee
                    .as_ref()
                    .map(|f| crate::fixtures::types::parse_amount(f))
                    .transpose()
                    .map_err(|e| anyhow!("{}", e))?
                    .unwrap_or(0);

                let from_hash = self.resolve_account(from_name)?;
                let current_balance = self.get_local_balance(&from_hash);
                if current_balance < fee {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient balance for fee".to_string()),
                        error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                let from_uno = self
                    .uno_balances
                    .get(from_name)
                    .and_then(|m| m.get(asset))
                    .copied()
                    .unwrap_or(0);

                if from_uno < amount {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Insufficient UNO balance".to_string()),
                        error_code: Some("INSUFFICIENT_UNO_BALANCE".to_string()),
                        state_changes: vec![],
                    });
                }

                self.uno_balances
                    .entry(from_name.to_string())
                    .or_default()
                    .insert(asset.to_string(), from_uno.saturating_sub(amount));

                let to_uno = self
                    .uno_balances
                    .get(to_name)
                    .and_then(|m| m.get(asset))
                    .copied()
                    .unwrap_or(0);
                self.uno_balances
                    .entry(to_name.to_string())
                    .or_default()
                    .insert(asset.to_string(), to_uno.saturating_add(amount));

                self.set_local_balance(&from_hash, current_balance.saturating_sub(fee));
                self.increment_nonce(&from_hash);

                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::MineBlock => {
                if let Some(blockchain) = &self.blockchain {
                    let _ = blockchain.mine_block().await;
                }
                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::AdvanceTime => Ok(StepResult {
                step: step.step,
                success: true,
                error: None,
                error_code: None,
                state_changes: vec![],
            }),
            TransactionType::Register
            | TransactionType::DeployContract
            | TransactionType::CallContract => Ok(StepResult {
                step: step.step,
                success: true,
                error: None,
                error_code: None,
                state_changes: vec![],
            }),
        }
    }

    async fn mine_block(&mut self) -> Result<()> {
        if let Some(blockchain) = &self.blockchain {
            blockchain.mine_block().await?;
        }
        Ok(())
    }

    async fn get_balance(&self, account: &str) -> Result<u64> {
        let hash = self
            .accounts
            .get(account)
            .ok_or_else(|| anyhow!("Unknown account: {}", account))?;
        Ok(self.get_local_balance(hash))
    }

    async fn get_uno_balance(&self, account: &str, asset: &str) -> Result<u64> {
        Ok(self
            .uno_balances
            .get(account)
            .and_then(|m| m.get(asset))
            .copied()
            .unwrap_or(0))
    }

    async fn get_nonce(&self, account: &str) -> Result<u64> {
        let hash = self
            .accounts
            .get(account)
            .ok_or_else(|| anyhow!("Unknown account: {}", account))?;
        Ok(self.get_local_nonce(hash))
    }

    async fn get_energy(&self, account: &str) -> Result<EnergyState> {
        Ok(self.energy_states.get(account).cloned().unwrap_or_default())
    }

    async fn get_frozen_balance(&self, account: &str) -> Result<u64> {
        Ok(self.frozen_balances.get(account).copied().unwrap_or(0))
    }

    async fn get_delegations_out(&self, account: &str) -> Result<DelegationMap> {
        Ok(self
            .delegations_out
            .get(account)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_delegations_in(&self, account: &str) -> Result<DelegationMap> {
        Ok(self
            .delegations_in
            .get(account)
            .cloned()
            .unwrap_or_default())
    }

    async fn advance_time(&mut self, _duration: Duration) -> Result<()> {
        Ok(())
    }

    fn account_names(&self) -> Vec<String> {
        self.accounts.keys().cloned().collect()
    }

    fn tier_name(&self) -> &str {
        "tier1_component"
    }
}

/// Classify an error message into a standard error code.
fn classify_error(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("insufficient") && lower.contains("balance") {
        "INSUFFICIENT_BALANCE".to_string()
    } else if lower.contains("nonce") {
        "INVALID_NONCE".to_string()
    } else if lower.contains("not found") || lower.contains("unknown") {
        "ACCOUNT_NOT_FOUND".to_string()
    } else if lower.contains("zero") && lower.contains("amount") {
        "ZERO_AMOUNT".to_string()
    } else {
        "UNKNOWN_ERROR".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::types::{AccountDef, ExpectStatus, FixtureSetup};

    fn basic_setup() -> FixtureSetup {
        let mut accounts = HashMap::new();
        accounts.insert(
            "alice".to_string(),
            AccountDef {
                balance: "10000".to_string(),
                nonce: Some(0),
                uno_balances: None,
                frozen_balance: None,
                energy: None,
                delegations_out: None,
                delegations_in: None,
                template: None,
            },
        );
        accounts.insert(
            "bob".to_string(),
            AccountDef {
                balance: "1000".to_string(),
                nonce: Some(0),
                uno_balances: None,
                frozen_balance: None,
                energy: None,
                delegations_out: None,
                delegations_in: None,
                template: None,
            },
        );
        FixtureSetup {
            network: None,
            assets: None,
            accounts,
        }
    }

    #[tokio::test]
    async fn test_setup_and_balance() {
        let mut backend = TestBlockchainBackend::new();
        backend.setup(&basic_setup()).await.unwrap();

        assert_eq!(backend.get_balance("alice").await.unwrap(), 10000);
        assert_eq!(backend.get_balance("bob").await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn test_transfer_step() {
        let mut backend = TestBlockchainBackend::new();
        backend.setup(&basic_setup()).await.unwrap();

        let step = TransactionStep {
            step: Some(1),
            name: Some("transfer".to_string()),
            tx_type: TransactionType::Transfer,
            from: Some("alice".to_string()),
            to: Some("bob".to_string()),
            amount: Some("2000".to_string()),
            fee: Some("10".to_string()),
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Success,
            expect_error: None,
        };

        let result = backend.execute_step(&step).await.unwrap();
        assert!(result.success, "Transfer failed: {:?}", result.error);

        assert_eq!(backend.get_balance("alice").await.unwrap(), 7990);
        assert_eq!(backend.get_balance("bob").await.unwrap(), 3000);
        assert_eq!(backend.get_nonce("alice").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_freeze_step() {
        let mut backend = TestBlockchainBackend::new();
        backend.setup(&basic_setup()).await.unwrap();

        let step = TransactionStep {
            step: Some(1),
            name: Some("freeze".to_string()),
            tx_type: TransactionType::Freeze,
            from: Some("alice".to_string()),
            to: None,
            amount: Some("1000".to_string()),
            fee: Some("10".to_string()),
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Success,
            expect_error: None,
        };

        let result = backend.execute_step(&step).await.unwrap();
        assert!(result.success);

        assert_eq!(backend.get_balance("alice").await.unwrap(), 8990);
        assert_eq!(backend.get_frozen_balance("alice").await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn test_self_delegation_error() {
        let mut backend = TestBlockchainBackend::new();
        backend.setup(&basic_setup()).await.unwrap();

        let step = TransactionStep {
            step: Some(1),
            name: Some("self_delegate".to_string()),
            tx_type: TransactionType::Delegate,
            from: Some("alice".to_string()),
            to: Some("alice".to_string()),
            amount: Some("100".to_string()),
            fee: Some("10".to_string()),
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Error,
            expect_error: Some("SELF_DELEGATION".to_string()),
        };

        let result = backend.execute_step(&step).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.error_code.as_deref(), Some("SELF_DELEGATION"));
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let mut backend = TestBlockchainBackend::new();
        backend.setup(&basic_setup()).await.unwrap();

        let step = TransactionStep {
            step: Some(1),
            name: Some("overdraft".to_string()),
            tx_type: TransactionType::Transfer,
            from: Some("alice".to_string()),
            to: Some("bob".to_string()),
            amount: Some("999999".to_string()),
            fee: None,
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Error,
            expect_error: None,
        };

        let result = backend.execute_step(&step).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.error_code.as_deref(), Some("INSUFFICIENT_BALANCE"));
    }

    #[test]
    fn test_classify_error() {
        assert_eq!(
            classify_error("Insufficient balance"),
            "INSUFFICIENT_BALANCE"
        );
        assert_eq!(classify_error("Invalid nonce"), "INVALID_NONCE");
        assert_eq!(classify_error("Account not found"), "ACCOUNT_NOT_FOUND");
        assert_eq!(classify_error("Something else"), "UNKNOWN_ERROR");
    }
}
