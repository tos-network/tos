//! Conformance specification parsing and validation

use super::*;
use anyhow::{Context, Result};
use std::path::Path;

/// Load a conformance spec from a YAML file
pub fn load_spec(path: &Path) -> Result<ConformanceSpec> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read spec file: {}", path.display()))?;

    let spec: ConformanceSpec = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse spec file: {}", path.display()))?;

    validate_spec(&spec)?;

    Ok(spec)
}

/// Validate a conformance spec for correctness
pub fn validate_spec(spec: &ConformanceSpec) -> Result<()> {
    // Spec must have a name
    if spec.spec.name.is_empty() {
        anyhow::bail!("Spec name cannot be empty");
    }

    // Must have either action or test_cases
    if spec.action.is_none() && spec.test_cases.is_none() {
        anyhow::bail!("Spec must have either 'action' or 'test_cases'");
    }

    // Validate preconditions
    for (i, cond) in spec.preconditions.iter().enumerate() {
        if cond.account.is_none() && cond.assertion.is_none() {
            anyhow::bail!("Precondition {} must have 'account' or 'assertion'", i);
        }
    }

    // Validate postconditions
    for (i, cond) in spec.postconditions.iter().enumerate() {
        if cond.account.is_none() && cond.assertion.is_none() {
            anyhow::bail!("Postcondition {} must have 'account' or 'assertion'", i);
        }
    }

    Ok(())
}

/// Load all specs from a directory
pub fn load_specs_from_dir(dir: &Path) -> Result<Vec<ConformanceSpec>> {
    let mut specs = Vec::new();

    if !dir.is_dir() {
        anyhow::bail!("Not a directory: {}", dir.display());
    }

    for entry in walkdir(dir)? {
        let path = entry?;
        if path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            match load_spec(&path) {
                Ok(spec) => specs.push(spec),
                Err(e) => {
                    log::warn!("Failed to load spec {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(specs)
}

/// Simple directory walker
fn walkdir(dir: &Path) -> Result<impl Iterator<Item = Result<std::path::PathBuf>>> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

    Ok(entries.filter_map(|entry| entry.ok().map(|e| Ok(e.path()))))
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
