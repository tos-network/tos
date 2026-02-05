use super::{
    error::GenesisError,
    state_hash::compute_state_hash,
    types::{AllocEntry, GenesisState, ParsedAllocEntry},
};
use crate::core::{
    error::BlockchainError,
    storage::{AccountProvider, BalanceProvider, EnergyProvider, NonceProvider},
};
use std::{collections::HashSet, path::Path};
use tos_common::{
    account::{EnergyResource, VersionedBalance, VersionedNonce},
    config::{MAXIMUM_SUPPLY, TOS_ASSET},
    crypto::{Address, AddressType, Hash, PublicKey},
    serializer::Serializer,
};

/// Load genesis state from a JSON file
pub fn load_genesis_state(path: &Path) -> Result<GenesisState, GenesisError> {
    if !path.exists() {
        return Err(GenesisError::FileNotFound(
            path.to_string_lossy().to_string(),
        ));
    }

    let content = std::fs::read_to_string(path)?;
    let state: GenesisState = serde_json::from_str(&content)?;

    // Validate format version
    if state.format_version != 1 {
        return Err(GenesisError::InvalidFormatVersion(
            state.format_version.to_string(),
        ));
    }

    // Validate required assets
    if !state.assets.contains_key(&TOS_ASSET.to_hex()) {
        return Err(GenesisError::MissingRequiredAsset("TOS".to_string()));
    }

    Ok(state)
}

/// Parse and validate allocations from the genesis state
pub fn parse_allocations(
    alloc: &[AllocEntry],
    is_mainnet: bool,
) -> Result<Vec<ParsedAllocEntry>, GenesisError> {
    let mut parsed = Vec::with_capacity(alloc.len());
    let mut seen_keys = HashSet::new();
    let mut total_balance: u128 = 0;

    for entry in alloc {
        // Parse and validate public key (64 hex chars = 32 bytes)
        let public_key = parse_public_key(&entry.public_key)?;

        // Check for duplicate public keys
        let key_hex = entry.public_key.to_lowercase();
        if seen_keys.contains(&key_hex) {
            return Err(GenesisError::DuplicatePublicKey(entry.public_key.clone()));
        }
        seen_keys.insert(key_hex);

        // Validate address if provided
        if let Some(ref addr_str) = entry.address {
            validate_address(&public_key, addr_str, is_mainnet)?;
        }

        // Parse nonce
        let nonce = parse_u64(&entry.nonce, "nonce")?;

        // Parse balance
        let balance = parse_u64(&entry.balance, "balance")?;

        // Track total balance for overflow check
        total_balance = total_balance
            .checked_add(balance as u128)
            .ok_or(GenesisError::BalanceOverflow)?;

        if total_balance > MAXIMUM_SUPPLY as u128 {
            return Err(GenesisError::BalanceOverflow);
        }

        // Parse energy (default to 0 if not specified)
        let energy_available = match &entry.energy {
            Some(energy_config) => parse_u64(&energy_config.available, "energy.available")?,
            None => 0,
        };

        parsed.push(ParsedAllocEntry {
            public_key,
            nonce,
            balance,
            energy_available,
        });
    }

    Ok(parsed)
}

/// Apply genesis state allocations to storage at topoheight 0
pub async fn apply_genesis_state<S>(
    storage: &mut S,
    alloc: &[ParsedAllocEntry],
) -> Result<(), BlockchainError>
where
    S: AccountProvider + BalanceProvider + NonceProvider + EnergyProvider,
{
    for entry in alloc {
        // Register account at topoheight 0
        storage
            .set_account_registration_topoheight(&entry.public_key, 0)
            .await?;

        // Set TOS balance
        let balance = VersionedBalance::new(entry.balance, None);
        storage
            .set_last_balance_to(&entry.public_key, &TOS_ASSET, 0, &balance)
            .await?;

        // Set nonce
        let nonce = VersionedNonce::new(entry.nonce, None);
        storage
            .set_last_nonce_to(&entry.public_key, 0, &nonce)
            .await?;

        // Set energy resource if non-zero
        if entry.energy_available > 0 {
            let mut energy = EnergyResource::new();
            energy.energy = entry.energy_available;
            energy.last_update = 0;
            storage
                .set_energy_resource(&entry.public_key, 0, &energy)
                .await?;
        }
    }

    Ok(())
}

/// Validate genesis state and compute/verify state hash
pub fn validate_genesis_state(state: &GenesisState) -> Result<Hash, GenesisError> {
    // Validate network
    let network = state.config.network.to_lowercase();
    if !["mainnet", "testnet", "devnet"].contains(&network.as_str()) {
        return Err(GenesisError::InvalidNetwork(state.config.network.clone()));
    }

    // Parse chain_id
    state
        .config
        .chain_id
        .parse::<u64>()
        .map_err(|_| GenesisError::InvalidChainId(state.config.chain_id.clone()))?;

    // Parse genesis_timestamp_ms
    state
        .config
        .genesis_timestamp_ms
        .parse::<u64>()
        .map_err(|_| GenesisError::InvalidTimestamp(state.config.genesis_timestamp_ms.clone()))?;

    // Validate dev_public_key (64 hex chars)
    parse_public_key(&state.config.dev_public_key)?;

    // Validate fork heights
    for (fork_name, fork_height) in &state.config.forks {
        fork_height
            .parse::<u64>()
            .map_err(|_| GenesisError::InvalidForkHeight {
                fork: fork_name.clone(),
                value: fork_height.clone(),
            })?;
    }

    // Parse allocations
    let is_mainnet = network == "mainnet";
    let parsed_alloc = parse_allocations(&state.alloc, is_mainnet)?;

    // Compute state hash
    let computed_hash = compute_state_hash(
        state.format_version,
        &state.config,
        &state.assets,
        &parsed_alloc,
    );

    // Verify against provided hash if present
    if let Some(ref computed) = state.computed {
        if let Some(ref expected_hash) = computed.state_hash {
            if &computed_hash != expected_hash {
                return Err(GenesisError::StateHashMismatch {
                    expected: expected_hash.clone(),
                    computed: computed_hash,
                });
            }
        }
    }

    Ok(computed_hash)
}

/// Get whether the network is mainnet based on network string
pub fn is_mainnet_network(network: &str) -> bool {
    network.to_lowercase() == "mainnet"
}

// Helper functions

fn parse_public_key(hex: &str) -> Result<PublicKey, GenesisError> {
    if hex.len() != 64 {
        return Err(GenesisError::InvalidPublicKey(format!(
            "expected 64 hex chars, got {}",
            hex.len()
        )));
    }

    // Validate hex characters
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GenesisError::InvalidPublicKey(
            "contains non-hex characters".to_string(),
        ));
    }

    let bytes = hex::decode(hex).map_err(|e| GenesisError::InvalidPublicKey(e.to_string()))?;

    let bytes_array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| GenesisError::InvalidPublicKey("invalid length".to_string()))?;

    PublicKey::from_bytes(&bytes_array)
        .map_err(|_| GenesisError::InvalidPublicKey("invalid curve point".to_string()))
}

fn parse_u64(value: &str, field_name: &str) -> Result<u64, GenesisError> {
    value.parse::<u64>().map_err(|_| match field_name {
        "nonce" => GenesisError::InvalidNonce(value.to_string()),
        "balance" => GenesisError::InvalidBalance(value.to_string()),
        "energy.available" => GenesisError::InvalidEnergy(value.to_string()),
        _ => GenesisError::InvalidBalance(format!("{}: {}", field_name, value)),
    })
}

fn validate_address(
    public_key: &PublicKey,
    provided_addr: &str,
    is_mainnet: bool,
) -> Result<(), GenesisError> {
    // Derive address from public key
    let derived_addr = Address::new(is_mainnet, AddressType::Normal, public_key.clone());
    let derived_str = derived_addr
        .as_string()
        .map_err(|e| GenesisError::AddressMismatch {
            public_key: hex::encode(public_key.as_bytes()),
            expected: format!("(derivation error: {})", e),
            provided: provided_addr.to_string(),
        })?;

    // Compare addresses
    if derived_str.to_lowercase() != provided_addr.to_lowercase() {
        return Err(GenesisError::AddressMismatch {
            public_key: hex::encode(public_key.as_bytes()),
            expected: derived_str,
            provided: provided_addr.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_public_key_valid() {
        // Create a valid 64-char hex string
        let hex = "01".repeat(32);
        let result = parse_public_key(&hex);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_public_key_invalid_length() {
        let hex = "0102030405";
        let result = parse_public_key(hex);
        assert!(matches!(result, Err(GenesisError::InvalidPublicKey(_))));
    }

    #[test]
    fn test_parse_public_key_invalid_hex() {
        let hex = "zz".repeat(32);
        let result = parse_public_key(&hex);
        assert!(matches!(result, Err(GenesisError::InvalidPublicKey(_))));
    }

    #[test]
    fn test_parse_u64_valid() {
        assert_eq!(parse_u64("12345", "balance").unwrap(), 12345);
        assert_eq!(parse_u64("0", "nonce").unwrap(), 0);
    }

    #[test]
    fn test_parse_u64_invalid() {
        assert!(matches!(
            parse_u64("-1", "balance"),
            Err(GenesisError::InvalidBalance(_))
        ));
        assert!(matches!(
            parse_u64("abc", "nonce"),
            Err(GenesisError::InvalidNonce(_))
        ));
    }

    #[test]
    fn test_parse_allocations_detects_duplicates() {
        let alloc = vec![
            AllocEntry {
                public_key: "01".repeat(32),
                address: None,
                nonce: "0".to_string(),
                balance: "1000".to_string(),
                energy: None,
            },
            AllocEntry {
                public_key: "01".repeat(32), // Duplicate
                address: None,
                nonce: "0".to_string(),
                balance: "2000".to_string(),
                energy: None,
            },
        ];

        let result = parse_allocations(&alloc, false);
        assert!(matches!(result, Err(GenesisError::DuplicatePublicKey(_))));
    }

    #[test]
    fn test_parse_allocations_overflow_check() {
        // Create entries that would overflow
        let alloc = vec![
            AllocEntry {
                public_key: "01".repeat(32),
                address: None,
                nonce: "0".to_string(),
                balance: u64::MAX.to_string(),
                energy: None,
            },
            AllocEntry {
                public_key: "02".repeat(32),
                address: None,
                nonce: "0".to_string(),
                balance: "1".to_string(),
                energy: None,
            },
        ];

        let result = parse_allocations(&alloc, false);
        assert!(matches!(result, Err(GenesisError::BalanceOverflow)));
    }

    #[test]
    fn test_is_mainnet_network() {
        assert!(is_mainnet_network("mainnet"));
        assert!(is_mainnet_network("MAINNET"));
        assert!(is_mainnet_network("Mainnet"));
        assert!(!is_mainnet_network("testnet"));
        assert!(!is_mainnet_network("devnet"));
    }

    #[test]
    fn test_load_genesis_state_from_json() {
        use std::io::Write;

        // Create a temporary JSON file
        let json_content = r#"{
            "format_version": 1,
            "config": {
                "chain_id": "1",
                "network": "devnet",
                "genesis_timestamp_ms": "1700000000000",
                "dev_public_key": "0101010101010101010101010101010101010101010101010101010101010101",
                "forks": {}
            },
            "assets": {
                "0000000000000000000000000000000000000000000000000000000000000000": {
                    "decimals": 8,
                    "name": "TOS",
                    "ticker": "TOS",
                    "max_supply": "2100000000000000"
                }
            },
            "alloc": [
                {
                    "public_key": "0101010101010101010101010101010101010101010101010101010101010101",
                    "nonce": "0",
                    "balance": "100000000000000"
                }
            ]
        }"#;

        // Write to temp file
        let mut temp_path = std::env::temp_dir();
        temp_path.push("test_genesis_state.json");

        {
            let mut file = std::fs::File::create(&temp_path).unwrap();
            file.write_all(json_content.as_bytes()).unwrap();
        }

        // Test loading
        let result = load_genesis_state(&temp_path);
        assert!(result.is_ok(), "Failed to load genesis state: {:?}", result);

        let state = result.unwrap();
        assert_eq!(state.format_version, 1);
        assert_eq!(state.config.network, "devnet");
        assert_eq!(state.alloc.len(), 1);

        // Test validation
        let validation_result = validate_genesis_state(&state);
        assert!(
            validation_result.is_ok(),
            "Validation failed: {:?}",
            validation_result
        );

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_load_genesis_state_invalid_version() {
        use std::io::Write;

        let json_content = r#"{
            "format_version": 2,
            "config": {
                "chain_id": "1",
                "network": "devnet",
                "genesis_timestamp_ms": "1700000000000",
                "dev_public_key": "0101010101010101010101010101010101010101010101010101010101010101"
            },
            "assets": {},
            "alloc": []
        }"#;

        let mut temp_path = std::env::temp_dir();
        temp_path.push("test_genesis_state_invalid.json");

        {
            let mut file = std::fs::File::create(&temp_path).unwrap();
            file.write_all(json_content.as_bytes()).unwrap();
        }

        let result = load_genesis_state(&temp_path);
        assert!(matches!(result, Err(GenesisError::InvalidFormatVersion(_))));

        let _ = std::fs::remove_file(&temp_path);
    }
}
