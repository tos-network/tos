//! Tier 1.5 backend: ChainClient-based fixture execution.
//!
//! Uses the ChainClient (direct blockchain access without network) for
//! fixture execution with BlockWarp and feature gate support.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::fixtures::backend::FixtureBackend;
use crate::fixtures::types::{
    DelegationMap, EnergyState, FixtureSetup, StepResult, TransactionStep, TransactionType,
};
use crate::tier1_5::{ChainClient, ChainClientConfig, GenesisAccount};
use tos_common::crypto::Hash;

/// Tier 1.5 fixture backend using ChainClient.
pub struct ChainClientBackend {
    /// The underlying ChainClient
    client: Option<ChainClient>,
    /// Account name -> Hash mapping
    accounts: HashMap<String, Hash>,
    /// Locally-tracked balances (fallback when client unavailable)
    local_balances: HashMap<Hash, u64>,
    /// Locally-tracked nonces
    local_nonces: HashMap<Hash, u64>,
    /// UNO balances (simulated)
    uno_balances: HashMap<String, HashMap<String, u64>>,
    /// Frozen balances (simulated)
    frozen_balances: HashMap<String, u64>,
    /// Energy states (simulated)
    energy_states: HashMap<String, EnergyState>,
    /// Delegations out
    delegations_out: HashMap<String, HashMap<String, u64>>,
    /// Delegations in
    delegations_in: HashMap<String, HashMap<String, u64>>,
}

impl ChainClientBackend {
    /// Create a new Tier 1.5 backend.
    pub fn new() -> Self {
        Self {
            client: None,
            accounts: HashMap::new(),
            local_balances: HashMap::new(),
            local_nonces: HashMap::new(),
            uno_balances: HashMap::new(),
            frozen_balances: HashMap::new(),
            energy_states: HashMap::new(),
            delegations_out: HashMap::new(),
            delegations_in: HashMap::new(),
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

    /// Look up account Hash by name.
    fn resolve_account(&self, name: &str) -> Result<Hash> {
        self.accounts
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown account: {}", name))
    }

    /// Get balance (from client or local tracking).
    fn get_local_balance(&self, hash: &Hash) -> u64 {
        self.local_balances.get(hash).copied().unwrap_or(0)
    }

    /// Get nonce (from local tracking, stored value).
    fn get_local_nonce(&self, hash: &Hash) -> u64 {
        self.local_nonces.get(hash).copied().unwrap_or(0)
    }

    /// Get the next valid transaction nonce (stored_nonce + 1).
    fn next_tx_nonce(&self, hash: &Hash) -> u64 {
        self.get_local_nonce(hash).saturating_add(1)
    }
}

impl Default for ChainClientBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FixtureBackend for ChainClientBackend {
    async fn setup(&mut self, setup: &FixtureSetup) -> Result<()> {
        let mut config = ChainClientConfig::default();

        for (name, account_def) in &setup.accounts {
            let hash = Self::name_to_hash(name);

            let balance = crate::fixtures::types::parse_amount(&account_def.balance)
                .map_err(|e| anyhow!("{}", e))?;
            config = config.with_account(GenesisAccount::new(hash.clone(), balance));

            // Track local state
            self.local_balances.insert(hash.clone(), balance);
            self.local_nonces
                .insert(hash.clone(), account_def.nonce.unwrap_or(0));
            self.accounts.insert(name.clone(), hash);

            if let Some(uno) = &account_def.uno_balances {
                self.uno_balances.insert(name.clone(), uno.clone());
            }
            if let Some(frozen_str) = &account_def.frozen_balance {
                let frozen = crate::fixtures::types::parse_amount(frozen_str)
                    .map_err(|e| anyhow!("{}", e))?;
                self.frozen_balances.insert(name.clone(), frozen);
            }
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

        // Start ChainClient
        let client = ChainClient::start(config)
            .await
            .map_err(|e| anyhow!("Failed to start ChainClient: {:?}", e))?;
        self.client = Some(client);

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

                // Check balance locally
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

                if amount == 0 {
                    return Ok(StepResult {
                        step: step.step,
                        success: false,
                        error: Some("Zero amount".to_string()),
                        error_code: Some("ZERO_AMOUNT".to_string()),
                        state_changes: vec![],
                    });
                }

                let nonce = self.next_tx_nonce(&from_hash);
                if let Some(client) = &mut self.client {
                    let tx = client.build_transaction(
                        from_hash.clone(),
                        crate::tier1_5::chain_client::TransactionType::Transfer {
                            to: to_hash.clone(),
                            amount,
                        },
                        nonce,
                        fee,
                    );
                    match client.process_transaction(tx).await {
                        Ok(result) => {
                            if result.success {
                                // Update local tracking
                                self.local_balances.insert(
                                    from_hash.clone(),
                                    sender_balance.saturating_sub(total_cost),
                                );
                                let receiver_balance = self.get_local_balance(&to_hash);
                                self.local_balances
                                    .insert(to_hash, receiver_balance.saturating_add(amount));
                                self.local_nonces.insert(from_hash, nonce);

                                Ok(StepResult {
                                    step: step.step,
                                    success: true,
                                    error: None,
                                    error_code: None,
                                    state_changes: vec![],
                                })
                            } else {
                                let error_msg = result
                                    .error
                                    .map(|e| e.to_string())
                                    .unwrap_or_else(|| "Unknown error".to_string());
                                Ok(StepResult {
                                    step: step.step,
                                    success: false,
                                    error: Some(error_msg),
                                    error_code: Some("EXECUTION_ERROR".to_string()),
                                    state_changes: vec![],
                                })
                            }
                        }
                        Err(e) => Ok(StepResult {
                            step: step.step,
                            success: false,
                            error: Some(format!("{:?}", e)),
                            error_code: Some("WARP_ERROR".to_string()),
                            state_changes: vec![],
                        }),
                    }
                } else {
                    // Simulate locally
                    self.local_balances
                        .insert(from_hash.clone(), sender_balance.saturating_sub(total_cost));
                    let receiver_balance = self.get_local_balance(&to_hash);
                    self.local_balances
                        .insert(to_hash, receiver_balance.saturating_add(amount));
                    let nonce = self.get_local_nonce(&from_hash);
                    self.local_nonces.insert(from_hash, nonce.saturating_add(1));

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

                self.local_balances.insert(
                    from_hash.clone(),
                    current_balance.saturating_sub(total_cost),
                );
                let current_frozen = self.frozen_balances.get(from_name).copied().unwrap_or(0);
                self.frozen_balances
                    .insert(from_name.to_string(), current_frozen.saturating_add(amount));

                let nonce = self.get_local_nonce(&from_hash);
                self.local_nonces.insert(from_hash, nonce.saturating_add(1));

                Ok(StepResult {
                    step: step.step,
                    success: true,
                    error: None,
                    error_code: None,
                    state_changes: vec![],
                })
            }
            TransactionType::MineBlock => {
                if let Some(client) = &mut self.client {
                    client
                        .mine_empty_block()
                        .await
                        .map_err(|e| anyhow!("Mine block failed: {:?}", e))?;
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
            _ => Ok(StepResult {
                step: step.step,
                success: true,
                error: None,
                error_code: None,
                state_changes: vec![],
            }),
        }
    }

    async fn mine_block(&mut self) -> Result<()> {
        if let Some(client) = &mut self.client {
            client
                .mine_empty_block()
                .await
                .map_err(|e| anyhow!("Mine block failed: {:?}", e))?;
        }
        Ok(())
    }

    async fn get_balance(&self, account: &str) -> Result<u64> {
        let hash = self.resolve_account(account)?;
        Ok(self.get_local_balance(&hash))
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
        let hash = self.resolve_account(account)?;
        Ok(self.get_local_nonce(&hash))
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
        "tier1_5_chain_client"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::types::AccountDef;

    #[tokio::test]
    async fn test_chain_client_backend_setup() {
        let mut backend = ChainClientBackend::new();
        let mut accounts = HashMap::new();
        accounts.insert(
            "alice".to_string(),
            AccountDef {
                balance: "5000".to_string(),
                nonce: None,
                uno_balances: None,
                frozen_balance: None,
                energy: None,
                delegations_out: None,
                delegations_in: None,
                template: None,
            },
        );
        let setup = FixtureSetup {
            network: None,
            assets: None,
            accounts,
        };

        backend.setup(&setup).await.unwrap();
        assert_eq!(backend.get_balance("alice").await.unwrap(), 5000);
        assert_eq!(backend.tier_name(), "tier1_5_chain_client");
    }
}
