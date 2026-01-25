//! Template loading and interpolation for fixture definitions.
//!
//! Provides reusable account templates and scenario templates that can be
//! referenced from fixture YAML files. Templates reduce duplication and
//! enable composable test scenarios.
//!
//! # Account Templates
//!
//! Account templates define reusable account configurations:
//! ```yaml
//! templates:
//!   whale:
//!     balance: "1_000_000 TOS"
//!     frozen_balance: "500_000 TOS"
//!     energy:
//!       limit: 920_000_000
//!   regular_user:
//!     balance: "10_000 TOS"
//! ```
//!
//! # Scenario Templates
//!
//! Scenario templates define reusable transaction sequences with parameters:
//! ```yaml
//! templates:
//!   basic_transfer:
//!     params: [sender, receiver, amount]
//!     transactions:
//!       - type: transfer
//!         from: "{{sender}}"
//!         to: "{{receiver}}"
//!         amount: "{{amount}}"
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::Deserialize;

use super::types::{AccountDef, EnergyDef, ExpectStatus, Step, TransactionStep, TransactionType};

/// Account template definition.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountTemplate {
    /// Initial TOS balance
    pub balance: Option<String>,
    /// Initial nonce
    pub nonce: Option<u64>,
    /// Frozen TOS balance
    pub frozen_balance: Option<String>,
    /// Energy configuration
    pub energy: Option<EnergyDef>,
    /// UNO token balances
    pub uno_balances: Option<HashMap<String, u64>>,
    /// Outgoing delegations
    pub delegations_out: Option<HashMap<String, String>>,
    /// Incoming delegations
    pub delegations_in: Option<HashMap<String, String>>,
}

impl AccountTemplate {
    /// Apply this template to create an AccountDef.
    /// Fields specified in the override take precedence over template values.
    pub fn apply(&self, overrides: &AccountDef) -> AccountDef {
        AccountDef {
            balance: if overrides.balance.is_empty() || overrides.balance == "0" {
                self.balance.clone().unwrap_or_else(|| "0".to_string())
            } else {
                overrides.balance.clone()
            },
            nonce: overrides.nonce.or(self.nonce),
            frozen_balance: overrides
                .frozen_balance
                .clone()
                .or_else(|| self.frozen_balance.clone()),
            energy: overrides.energy.clone().or_else(|| self.energy.clone()),
            uno_balances: overrides
                .uno_balances
                .clone()
                .or_else(|| self.uno_balances.clone()),
            delegations_out: overrides
                .delegations_out
                .clone()
                .or_else(|| self.delegations_out.clone()),
            delegations_in: overrides
                .delegations_in
                .clone()
                .or_else(|| self.delegations_in.clone()),
            template: None,
        }
    }

    /// Convert template directly to AccountDef (no overrides).
    pub fn to_account_def(&self) -> AccountDef {
        AccountDef {
            balance: self.balance.clone().unwrap_or_else(|| "0".to_string()),
            nonce: self.nonce,
            frozen_balance: self.frozen_balance.clone(),
            energy: self.energy.clone(),
            uno_balances: self.uno_balances.clone(),
            delegations_out: self.delegations_out.clone(),
            delegations_in: self.delegations_in.clone(),
            template: None,
        }
    }
}

/// Scenario template transaction step definition.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioStepDef {
    /// Transaction type
    #[serde(rename = "type")]
    pub tx_type: String,
    /// Sender (may contain {{param}} placeholders)
    pub from: Option<String>,
    /// Receiver (may contain {{param}} placeholders)
    pub to: Option<String>,
    /// Amount (may contain {{param}} placeholders)
    pub amount: Option<String>,
    /// Fee (may contain {{param}} placeholders)
    pub fee: Option<String>,
    /// Asset identifier
    pub asset: Option<String>,
    /// Duration for advance_time
    pub duration: Option<String>,
    /// Expected status
    pub expect_status: Option<String>,
}

/// Scenario template definition with parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioTemplate {
    /// Parameter names for interpolation
    pub params: Vec<String>,
    /// Transaction steps with placeholder references
    pub transactions: Vec<ScenarioStepDef>,
}

impl ScenarioTemplate {
    /// Instantiate this template with concrete parameter values.
    /// Returns a list of transaction steps with placeholders replaced.
    pub fn instantiate(&self, params: &HashMap<String, String>) -> Result<Vec<Step>> {
        // Verify all required params are provided
        for param in &self.params {
            if !params.contains_key(param) {
                return Err(anyhow!("Missing template parameter: {}", param));
            }
        }

        let mut steps = Vec::new();
        for (idx, step_def) in self.transactions.iter().enumerate() {
            let step_num = idx.checked_add(1).unwrap_or(idx) as u32;

            let tx_type = parse_tx_type(&step_def.tx_type)?;
            let from = step_def.from.as_ref().map(|s| interpolate(s, params));
            let to = step_def.to.as_ref().map(|s| interpolate(s, params));
            let amount = step_def.amount.as_ref().map(|s| interpolate(s, params));
            let fee = step_def.fee.as_ref().map(|s| interpolate(s, params));
            let asset = step_def.asset.as_ref().map(|s| interpolate(s, params));
            let duration = step_def.duration.as_ref().map(|s| interpolate(s, params));
            let expect_status = step_def
                .expect_status
                .as_ref()
                .map(|s| parse_expect_status(s))
                .unwrap_or(ExpectStatus::Success);

            steps.push(Step::Transaction(Box::new(TransactionStep {
                step: Some(step_num),
                name: None,
                tx_type,
                from,
                to,
                amount,
                fee,
                asset,
                nonce: None,
                duration,
                code: None,
                contract: None,
                function: None,
                args: None,
                expect_status,
                expect_error: None,
            })));
        }

        Ok(steps)
    }
}

/// Template registry that holds loaded account and scenario templates.
#[derive(Debug, Clone, Default)]
pub struct TemplateRegistry {
    /// Account templates by name
    pub account_templates: HashMap<String, AccountTemplate>,
    /// Scenario templates by name
    pub scenario_templates: HashMap<String, ScenarioTemplate>,
}

/// Top-level template file structure for deserialization.
#[derive(Debug, Clone, Deserialize)]
struct TemplateFile {
    /// Template definitions (can be account or scenario)
    templates: HashMap<String, serde_yaml::Value>,
}

impl TemplateRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            account_templates: HashMap::new(),
            scenario_templates: HashMap::new(),
        }
    }

    /// Load templates from a YAML file.
    /// Distinguishes account templates from scenario templates by the presence
    /// of a `params` field.
    pub fn load_file(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read template file {:?}: {}", path, e))?;
        self.load_yaml(&content)
    }

    /// Load templates from a YAML string.
    pub fn load_yaml(&mut self, yaml: &str) -> Result<()> {
        let file: TemplateFile = serde_yaml::from_str(yaml)
            .map_err(|e| anyhow!("Failed to parse template YAML: {}", e))?;

        for (name, value) in file.templates {
            // If value has a "params" field, it's a scenario template
            if value.get("params").is_some() {
                let scenario: ScenarioTemplate = serde_yaml::from_value(value.clone())
                    .map_err(|e| anyhow!("Failed to parse scenario template '{}': {}", name, e))?;
                self.scenario_templates.insert(name, scenario);
            } else {
                // Otherwise it's an account template
                let account: AccountTemplate = serde_yaml::from_value(value.clone())
                    .map_err(|e| anyhow!("Failed to parse account template '{}': {}", name, e))?;
                self.account_templates.insert(name, account);
            }
        }

        Ok(())
    }

    /// Load all template files from a directory.
    pub fn load_directory(&mut self, dir: &Path) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| anyhow!("Failed to read template directory {:?}: {}", dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| anyhow!("Failed to read directory entry: {}", e))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml")
                || path.extension().and_then(|e| e.to_str()) == Some("yml")
            {
                self.load_file(&path)?;
            }
        }

        Ok(())
    }

    /// Get an account template by name.
    pub fn get_account_template(&self, name: &str) -> Option<&AccountTemplate> {
        self.account_templates.get(name)
    }

    /// Get a scenario template by name.
    pub fn get_scenario_template(&self, name: &str) -> Option<&ScenarioTemplate> {
        self.scenario_templates.get(name)
    }

    /// Resolve an AccountDef that references a template.
    /// If the AccountDef has a `template` field, applies the template
    /// and returns the merged result.
    pub fn resolve_account(&self, account_def: &AccountDef) -> Result<AccountDef> {
        if let Some(ref template_name) = account_def.template {
            let template = self
                .account_templates
                .get(template_name)
                .ok_or_else(|| anyhow!("Unknown account template: {}", template_name))?;
            Ok(template.apply(account_def))
        } else {
            Ok(account_def.clone())
        }
    }

    /// Resolve all account definitions in a setup, applying templates where referenced.
    pub fn resolve_accounts(
        &self,
        accounts: &HashMap<String, AccountDef>,
    ) -> Result<HashMap<String, AccountDef>> {
        let mut resolved = HashMap::new();
        for (name, def) in accounts {
            resolved.insert(name.clone(), self.resolve_account(def)?);
        }
        Ok(resolved)
    }
}

/// Interpolate {{param}} placeholders in a string.
fn interpolate(template: &str, params: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Parse a transaction type string into TransactionType.
fn parse_tx_type(s: &str) -> Result<TransactionType> {
    match s {
        "transfer" => Ok(TransactionType::Transfer),
        "uno_transfer" => Ok(TransactionType::UnoTransfer),
        "freeze" => Ok(TransactionType::Freeze),
        "unfreeze" => Ok(TransactionType::Unfreeze),
        "delegate" => Ok(TransactionType::Delegate),
        "undelegate" => Ok(TransactionType::Undelegate),
        "register" => Ok(TransactionType::Register),
        "mine_block" => Ok(TransactionType::MineBlock),
        "advance_time" => Ok(TransactionType::AdvanceTime),
        "deploy_contract" => Ok(TransactionType::DeployContract),
        "call_contract" => Ok(TransactionType::CallContract),
        _ => Err(anyhow!("Unknown transaction type: {}", s)),
    }
}

/// Parse an expect_status string.
fn parse_expect_status(s: &str) -> ExpectStatus {
    match s {
        "error" | "fail" | "failure" => ExpectStatus::Error,
        _ => ExpectStatus::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_basic() {
        let mut params = HashMap::new();
        params.insert("sender".to_string(), "alice".to_string());
        params.insert("receiver".to_string(), "bob".to_string());
        params.insert("amount".to_string(), "1000 TOS".to_string());

        assert_eq!(interpolate("{{sender}}", &params), "alice");
        assert_eq!(interpolate("{{receiver}}", &params), "bob");
        assert_eq!(interpolate("{{amount}}", &params), "1000 TOS");
        assert_eq!(
            interpolate("from {{sender}} to {{receiver}}", &params),
            "from alice to bob"
        );
    }

    #[test]
    fn test_interpolate_no_match() {
        let params = HashMap::new();
        assert_eq!(interpolate("no_placeholder", &params), "no_placeholder");
        assert_eq!(interpolate("{{missing}}", &params), "{{missing}}");
    }

    #[test]
    fn test_account_template_to_account_def() {
        let template = AccountTemplate {
            balance: Some("1_000_000 TOS".to_string()),
            nonce: Some(0),
            frozen_balance: Some("500_000 TOS".to_string()),
            energy: Some(EnergyDef {
                limit: Some(920_000_000),
                usage: Some(0),
                available: Some(920_000_000),
                last_usage_time: None,
            }),
            uno_balances: None,
            delegations_out: None,
            delegations_in: None,
        };

        let def = template.to_account_def();
        assert_eq!(def.balance, "1_000_000 TOS");
        assert_eq!(def.nonce, Some(0));
        assert_eq!(def.frozen_balance, Some("500_000 TOS".to_string()));
        assert!(def.energy.is_some());
        assert!(def.template.is_none());
    }

    #[test]
    fn test_account_template_apply_with_override() {
        let template = AccountTemplate {
            balance: Some("1_000_000 TOS".to_string()),
            nonce: Some(0),
            frozen_balance: Some("500_000 TOS".to_string()),
            energy: None,
            uno_balances: None,
            delegations_out: None,
            delegations_in: None,
        };

        let override_def = AccountDef {
            balance: "50_000 TOS".to_string(),
            nonce: Some(5),
            frozen_balance: None,
            energy: None,
            uno_balances: None,
            delegations_out: None,
            delegations_in: None,
            template: Some("whale".to_string()),
        };

        let result = template.apply(&override_def);
        // Override takes precedence
        assert_eq!(result.balance, "50_000 TOS");
        assert_eq!(result.nonce, Some(5));
        // Template value used as fallback
        assert_eq!(result.frozen_balance, Some("500_000 TOS".to_string()));
    }

    #[test]
    fn test_template_registry_load_account_templates() {
        let yaml = r#"
templates:
  whale:
    balance: "1_000_000 TOS"
    frozen_balance: "500_000 TOS"
    nonce: 0
  regular_user:
    balance: "10_000 TOS"
  empty_account:
    balance: "0 TOS"
    nonce: 0
"#;

        let mut registry = TemplateRegistry::new();
        registry.load_yaml(yaml).expect("Should parse templates");

        assert_eq!(registry.account_templates.len(), 3);
        assert!(registry.account_templates.contains_key("whale"));
        assert!(registry.account_templates.contains_key("regular_user"));
        assert!(registry.account_templates.contains_key("empty_account"));

        let whale = registry.get_account_template("whale").unwrap();
        assert_eq!(whale.balance, Some("1_000_000 TOS".to_string()));
        assert_eq!(whale.frozen_balance, Some("500_000 TOS".to_string()));
    }

    #[test]
    fn test_template_registry_load_scenario_templates() {
        let yaml = r#"
templates:
  basic_transfer:
    params: [sender, receiver, amount]
    transactions:
      - type: transfer
        from: "{{sender}}"
        to: "{{receiver}}"
        amount: "{{amount}}"
        expect_status: success
      - type: mine_block
"#;

        let mut registry = TemplateRegistry::new();
        registry.load_yaml(yaml).expect("Should parse templates");

        assert_eq!(registry.scenario_templates.len(), 1);
        let scenario = registry.get_scenario_template("basic_transfer").unwrap();
        assert_eq!(scenario.params, vec!["sender", "receiver", "amount"]);
        assert_eq!(scenario.transactions.len(), 2);
    }

    #[test]
    fn test_scenario_template_instantiate() {
        let yaml = r#"
templates:
  basic_transfer:
    params: [sender, receiver, amount]
    transactions:
      - type: transfer
        from: "{{sender}}"
        to: "{{receiver}}"
        amount: "{{amount}}"
        fee: "10 TOS"
        expect_status: success
      - type: mine_block
"#;

        let mut registry = TemplateRegistry::new();
        registry.load_yaml(yaml).unwrap();

        let scenario = registry.get_scenario_template("basic_transfer").unwrap();
        let mut params = HashMap::new();
        params.insert("sender".to_string(), "alice".to_string());
        params.insert("receiver".to_string(), "bob".to_string());
        params.insert("amount".to_string(), "5_000 TOS".to_string());

        let steps = scenario.instantiate(&params).unwrap();
        assert_eq!(steps.len(), 2);

        if let Step::Transaction(ref step) = steps[0] {
            assert_eq!(step.tx_type, TransactionType::Transfer);
            assert_eq!(step.from.as_deref(), Some("alice"));
            assert_eq!(step.to.as_deref(), Some("bob"));
            assert_eq!(step.amount.as_deref(), Some("5_000 TOS"));
            assert_eq!(step.fee.as_deref(), Some("10 TOS"));
        } else {
            panic!("Expected Transaction step");
        }

        if let Step::Transaction(ref step) = steps[1] {
            assert_eq!(step.tx_type, TransactionType::MineBlock);
        } else {
            panic!("Expected Transaction step for mine_block");
        }
    }

    #[test]
    fn test_scenario_template_missing_param() {
        let scenario = ScenarioTemplate {
            params: vec!["sender".to_string(), "receiver".to_string()],
            transactions: vec![ScenarioStepDef {
                tx_type: "transfer".to_string(),
                from: Some("{{sender}}".to_string()),
                to: Some("{{receiver}}".to_string()),
                amount: None,
                fee: None,
                asset: None,
                duration: None,
                expect_status: None,
            }],
        };

        let mut params = HashMap::new();
        params.insert("sender".to_string(), "alice".to_string());
        // Missing "receiver"

        let result = scenario.instantiate(&params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing template parameter: receiver"));
    }

    #[test]
    fn test_resolve_account_with_template() {
        let yaml = r#"
templates:
  whale:
    balance: "1_000_000 TOS"
    frozen_balance: "500_000 TOS"
"#;

        let mut registry = TemplateRegistry::new();
        registry.load_yaml(yaml).unwrap();

        let account_def = AccountDef {
            balance: "".to_string(),
            nonce: Some(10),
            frozen_balance: None,
            energy: None,
            uno_balances: None,
            delegations_out: None,
            delegations_in: None,
            template: Some("whale".to_string()),
        };

        let resolved = registry.resolve_account(&account_def).unwrap();
        assert_eq!(resolved.balance, "1_000_000 TOS");
        assert_eq!(resolved.nonce, Some(10));
        assert_eq!(resolved.frozen_balance, Some("500_000 TOS".to_string()));
    }

    #[test]
    fn test_resolve_account_without_template() {
        let registry = TemplateRegistry::new();

        let account_def = AccountDef {
            balance: "5_000 TOS".to_string(),
            nonce: Some(0),
            frozen_balance: None,
            energy: None,
            uno_balances: None,
            delegations_out: None,
            delegations_in: None,
            template: None,
        };

        let resolved = registry.resolve_account(&account_def).unwrap();
        assert_eq!(resolved.balance, "5_000 TOS");
    }

    #[test]
    fn test_resolve_account_unknown_template() {
        let registry = TemplateRegistry::new();

        let account_def = AccountDef {
            balance: "".to_string(),
            nonce: None,
            frozen_balance: None,
            energy: None,
            uno_balances: None,
            delegations_out: None,
            delegations_in: None,
            template: Some("nonexistent".to_string()),
        };

        let result = registry.resolve_account(&account_def);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown account template"));
    }

    #[test]
    fn test_resolve_accounts_batch() {
        let yaml = r#"
templates:
  funded:
    balance: "10_000 TOS"
"#;

        let mut registry = TemplateRegistry::new();
        registry.load_yaml(yaml).unwrap();

        let mut accounts = HashMap::new();
        accounts.insert(
            "alice".to_string(),
            AccountDef {
                balance: "".to_string(),
                nonce: None,
                frozen_balance: None,
                energy: None,
                uno_balances: None,
                delegations_out: None,
                delegations_in: None,
                template: Some("funded".to_string()),
            },
        );
        accounts.insert(
            "bob".to_string(),
            AccountDef {
                balance: "5_000 TOS".to_string(),
                nonce: None,
                frozen_balance: None,
                energy: None,
                uno_balances: None,
                delegations_out: None,
                delegations_in: None,
                template: None,
            },
        );

        let resolved = registry.resolve_accounts(&accounts).unwrap();
        assert_eq!(resolved.get("alice").unwrap().balance, "10_000 TOS");
        assert_eq!(resolved.get("bob").unwrap().balance, "5_000 TOS");
    }

    #[test]
    fn test_parse_tx_type_all_variants() {
        assert_eq!(
            parse_tx_type("transfer").unwrap(),
            TransactionType::Transfer
        );
        assert_eq!(
            parse_tx_type("uno_transfer").unwrap(),
            TransactionType::UnoTransfer
        );
        assert_eq!(parse_tx_type("freeze").unwrap(), TransactionType::Freeze);
        assert_eq!(
            parse_tx_type("unfreeze").unwrap(),
            TransactionType::Unfreeze
        );
        assert_eq!(
            parse_tx_type("delegate").unwrap(),
            TransactionType::Delegate
        );
        assert_eq!(
            parse_tx_type("undelegate").unwrap(),
            TransactionType::Undelegate
        );
        assert_eq!(
            parse_tx_type("register").unwrap(),
            TransactionType::Register
        );
        assert_eq!(
            parse_tx_type("mine_block").unwrap(),
            TransactionType::MineBlock
        );
        assert_eq!(
            parse_tx_type("advance_time").unwrap(),
            TransactionType::AdvanceTime
        );
        assert_eq!(
            parse_tx_type("deploy_contract").unwrap(),
            TransactionType::DeployContract
        );
        assert_eq!(
            parse_tx_type("call_contract").unwrap(),
            TransactionType::CallContract
        );
        assert!(parse_tx_type("unknown").is_err());
    }

    #[test]
    fn test_parse_expect_status() {
        assert_eq!(parse_expect_status("success"), ExpectStatus::Success);
        assert_eq!(parse_expect_status("error"), ExpectStatus::Error);
        assert_eq!(parse_expect_status("fail"), ExpectStatus::Error);
        assert_eq!(parse_expect_status("failure"), ExpectStatus::Error);
        assert_eq!(parse_expect_status("anything_else"), ExpectStatus::Success);
    }

    #[test]
    fn test_mixed_templates_file() {
        let yaml = r#"
templates:
  whale:
    balance: "1_000_000 TOS"
    frozen_balance: "500_000 TOS"
  regular_user:
    balance: "10_000 TOS"
  basic_transfer:
    params: [sender, receiver, amount]
    transactions:
      - type: transfer
        from: "{{sender}}"
        to: "{{receiver}}"
        amount: "{{amount}}"
"#;

        let mut registry = TemplateRegistry::new();
        registry.load_yaml(yaml).unwrap();

        assert_eq!(registry.account_templates.len(), 2);
        assert_eq!(registry.scenario_templates.len(), 1);
        assert!(registry.account_templates.contains_key("whale"));
        assert!(registry.account_templates.contains_key("regular_user"));
        assert!(registry.scenario_templates.contains_key("basic_transfer"));
    }

    #[test]
    fn test_template_registry_load_nonexistent_directory() {
        let mut registry = TemplateRegistry::new();
        let result = registry.load_directory(Path::new("/nonexistent/path"));
        // Should succeed (empty directory is OK)
        assert!(result.is_ok());
    }
}
