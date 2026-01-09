//! Conformance specification parsing and validation

use super::*;
use anyhow::{Context, Result};
use std::path::Path;
use walkdir::WalkDir;

/// Load a conformance spec from a YAML file
pub fn load_spec(path: &Path) -> Result<ConformanceSpec> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read spec file: {}", path.display()))?;

    let spec: ConformanceSpec = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse spec file: {}", path.display()))?;

    validate_spec(&spec)?;

    Ok(spec)
}

/// Maximum length for spec names
const MAX_SPEC_NAME_LENGTH: usize = 256;

/// Maximum length for account names
const MAX_ACCOUNT_NAME_LENGTH: usize = 128;

/// Maximum number of preconditions/postconditions
const MAX_CONDITIONS: usize = 100;

/// Maximum number of test cases
const MAX_TEST_CASES: usize = 1000;

/// Maximum balance value (prevent overflow)
const MAX_BALANCE: u64 = u64::MAX / 2;

/// Validate a conformance spec for correctness
pub fn validate_spec(spec: &ConformanceSpec) -> Result<()> {
    // Spec must have a name
    if spec.spec.name.is_empty() {
        anyhow::bail!("Spec name cannot be empty");
    }

    // Validate spec name length
    if spec.spec.name.len() > MAX_SPEC_NAME_LENGTH {
        anyhow::bail!(
            "Spec name exceeds maximum length of {} characters",
            MAX_SPEC_NAME_LENGTH
        );
    }

    // Validate spec name contains only valid characters (alphanumeric, underscore, hyphen)
    if !spec
        .spec
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "Spec name '{}' contains invalid characters. Use only alphanumeric, underscore, or hyphen",
            spec.spec.name
        );
    }

    // Must have either action or test_cases
    if spec.action.is_none() && spec.test_cases.is_none() {
        anyhow::bail!("Spec must have either 'action' or 'test_cases'");
    }

    // Validate number of preconditions
    if spec.preconditions.len() > MAX_CONDITIONS {
        anyhow::bail!(
            "Too many preconditions: {} (max {})",
            spec.preconditions.len(),
            MAX_CONDITIONS
        );
    }

    // Validate preconditions
    for (i, cond) in spec.preconditions.iter().enumerate() {
        validate_condition(cond, &format!("Precondition {}", i))?;
    }

    // Validate number of postconditions
    if spec.postconditions.len() > MAX_CONDITIONS {
        anyhow::bail!(
            "Too many postconditions: {} (max {})",
            spec.postconditions.len(),
            MAX_CONDITIONS
        );
    }

    // Validate postconditions
    for (i, cond) in spec.postconditions.iter().enumerate() {
        validate_condition(cond, &format!("Postcondition {}", i))?;
    }

    // Validate test cases if present
    if let Some(test_cases) = &spec.test_cases {
        if test_cases.len() > MAX_TEST_CASES {
            anyhow::bail!(
                "Too many test cases: {} (max {})",
                test_cases.len(),
                MAX_TEST_CASES
            );
        }
    }

    Ok(())
}

/// Validate a condition (precondition or postcondition)
fn validate_condition(cond: &Condition, context: &str) -> Result<()> {
    // Must have account or assertion
    if cond.account.is_none() && cond.assertion.is_none() {
        anyhow::bail!("{} must have 'account' or 'assertion'", context);
    }

    // Validate account name if present
    if let Some(account) = &cond.account {
        if account.is_empty() {
            anyhow::bail!("{}: account name cannot be empty", context);
        }
        if account.len() > MAX_ACCOUNT_NAME_LENGTH {
            anyhow::bail!(
                "{}: account name exceeds maximum length of {} characters",
                context,
                MAX_ACCOUNT_NAME_LENGTH
            );
        }
        // Account names should be valid identifiers
        if !account.chars().all(|c| c.is_alphanumeric() || c == '_') {
            anyhow::bail!(
                "{}: account name '{}' contains invalid characters",
                context,
                account
            );
        }
    }

    // Validate balance is within reasonable range
    if let Some(balance) = cond.balance {
        if balance > MAX_BALANCE {
            anyhow::bail!(
                "{}: balance {} exceeds maximum allowed value {}",
                context,
                balance,
                MAX_BALANCE
            );
        }
    }

    Ok(())
}

/// Load all specs from a directory (recursively)
///
/// Recursively walks through all subdirectories to find YAML spec files.
/// This ensures specs in nested directories like `specs/syscalls/`,
/// `specs/consensus/`, etc. are all discovered.
pub fn load_specs_from_dir(dir: &Path) -> Result<Vec<ConformanceSpec>> {
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    if !dir.is_dir() {
        anyhow::bail!("Not a directory: {}", dir.display());
    }

    // Use walkdir crate for recursive directory traversal
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip directories, only process files
        if !path.is_file() {
            continue;
        }

        // Check for YAML extension
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");

        if is_yaml {
            match load_spec(path) {
                Ok(spec) => {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!("Loaded spec: {} from {}", spec.spec.name, path.display());
                    }
                    specs.push(spec);
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Warn) {
                        log::warn!("Failed to load spec {}: {}", path.display(), e);
                    }
                    errors.push((path.to_path_buf(), e));
                }
            }
        }
    }

    // Log summary
    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Loaded {} specs from {} ({} errors)",
            specs.len(),
            dir.display(),
            errors.len()
        );
    }

    // If no specs found but there were errors, report them
    if specs.is_empty() && !errors.is_empty() {
        let error_summary: Vec<String> = errors
            .iter()
            .take(5)
            .map(|(p, e)| format!("{}: {}", p.display(), e))
            .collect();
        anyhow::bail!(
            "No specs loaded from {}. Errors:\n{}",
            dir.display(),
            error_summary.join("\n")
        );
    }

    Ok(specs)
}

/// Get statistics about specs in a directory
pub fn get_spec_stats(dir: &Path) -> Result<SpecStats> {
    let mut stats = SpecStats::default();

    if !dir.is_dir() {
        anyhow::bail!("Not a directory: {}", dir.display());
    }

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");

        if is_yaml {
            stats.total_files += 1;

            match load_spec(path) {
                Ok(spec) => {
                    stats.valid_specs += 1;
                    match spec.spec.category {
                        Category::Syscalls => stats.by_category.syscalls += 1,
                        Category::Consensus => stats.by_category.consensus += 1,
                        Category::Api => stats.by_category.api += 1,
                        Category::P2p => stats.by_category.p2p += 1,
                        Category::Security => stats.by_category.security += 1,
                    }
                }
                Err(_) => {
                    stats.invalid_specs += 1;
                }
            }
        }
    }

    Ok(stats)
}

/// Statistics about loaded specs
#[derive(Debug, Default)]
pub struct SpecStats {
    /// Total YAML files found
    pub total_files: usize,
    /// Successfully parsed specs
    pub valid_specs: usize,
    /// Failed to parse
    pub invalid_specs: usize,
    /// Breakdown by category
    pub by_category: CategoryStats,
}

/// Category breakdown
#[derive(Debug, Default)]
pub struct CategoryStats {
    /// Number of syscall specs
    pub syscalls: usize,
    /// Number of consensus specs
    pub consensus: usize,
    /// Number of API specs
    pub api: usize,
    /// Number of P2P specs
    pub p2p: usize,
    /// Number of security specs
    pub security: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_spec() {
        let yaml = r#"
spec:
  name: test_transfer
  version: "1.0"
  category: syscalls

preconditions:
  - account: sender
    balance: 1000

action:
  type: transfer
  from: sender
  to: receiver
  amount: 100

expected:
  status: success

postconditions:
  - account: sender
    balance: 900
"#;

        let spec: ConformanceSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.spec.name, "test_transfer");
        assert_eq!(spec.spec.category, Category::Syscalls);
    }
}
