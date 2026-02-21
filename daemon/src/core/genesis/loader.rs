use super::{
    error::GenesisError,
    state_hash::{compute_state_hash, ParsedAsset, ParsedConfig},
    types::{AllocEntry, AssetConfig, GenesisConfig, GenesisState, ParsedAllocEntry},
};
use crate::core::{
    error::BlockchainError,
    storage::{AccountProvider, BalanceProvider, NonceProvider},
};
use std::{collections::HashSet, path::Path};
use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    config::{TOS_ASSET, UNO_ASSET},
    crypto::{Address, AddressType, Hash, PublicKey},
    serializer::Serializer,
};

/// Maximum string length for fields with u8 length prefix (PLAN-B v1.5: reject if exceeded)
const MAX_STRING_LENGTH: usize = 255;

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

    // Validate required assets: both TOS and UNO must be present (keyed by name)
    if !state.assets.contains_key("TOS") {
        return Err(GenesisError::MissingRequiredAsset("TOS".to_string()));
    }
    if !state.assets.contains_key("UNO") {
        return Err(GenesisError::MissingRequiredAsset("UNO".to_string()));
    }

    Ok(state)
}

/// Parse and validate allocations from the genesis state
pub fn parse_allocations(
    alloc: &[AllocEntry],
    is_mainnet: bool,
    tos_max_supply: Option<u64>,
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

        // Check against TOS max_supply from genesis config (if specified)
        if let Some(max_supply) = tos_max_supply {
            if total_balance > max_supply as u128 {
                return Err(GenesisError::BalanceOverflow);
            }
        }

        parsed.push(ParsedAllocEntry {
            public_key,
            nonce,
            balance,
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
    S: AccountProvider + BalanceProvider + NonceProvider,
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
    }

    Ok(())
}

/// Validate genesis state and compute/verify state hash
pub fn validate_genesis_state(
    state: &GenesisState,
) -> Result<(Hash, ValidatedGenesisData), GenesisError> {
    // Validate network - must be exactly lowercase as per PLAN-B spec
    let network = &state.config.network;
    if !["mainnet", "testnet", "devnet"].contains(&network.as_str()) {
        return Err(GenesisError::InvalidNetwork(state.config.network.clone()));
    }

    // Validate string lengths (PLAN-B v1.5: reject if > 255 bytes)
    validate_string_length(&state.config.network, "network")?;
    for (fork_name, _) in &state.config.forks {
        validate_string_length(fork_name, "fork name")?;
    }

    // Parse chain_id
    let chain_id = state
        .config
        .chain_id
        .parse::<u64>()
        .map_err(|_| GenesisError::InvalidChainId(state.config.chain_id.clone()))?;

    // Parse genesis_timestamp_ms
    let genesis_timestamp_ms = state
        .config
        .genesis_timestamp_ms
        .parse::<u64>()
        .map_err(|_| GenesisError::InvalidTimestamp(state.config.genesis_timestamp_ms.clone()))?;

    // Validate and parse dev_public_key (64 hex chars)
    let dev_public_key = parse_public_key(&state.config.dev_public_key)?;
    let dev_public_key_bytes: [u8; 32] = dev_public_key.as_bytes().to_owned();

    // Parse fork heights
    let mut parsed_forks = std::collections::BTreeMap::new();
    for (fork_name, fork_height) in &state.config.forks {
        let height = fork_height
            .parse::<u64>()
            .map_err(|_| GenesisError::InvalidForkHeight {
                fork: fork_name.clone(),
                value: fork_height.clone(),
            })?;
        parsed_forks.insert(fork_name.clone(), height);
    }

    // Parse and validate assets
    let parsed_assets = parse_assets(&state.assets)?;

    // Validate TOS asset hash and decimals match required constants
    let tos_asset = state
        .assets
        .get("TOS")
        .ok_or_else(|| GenesisError::MissingRequiredAsset("TOS".to_string()))?;
    let tos_hash = parse_hash(&tos_asset.hash)?;
    if tos_hash != TOS_ASSET.to_bytes() {
        return Err(GenesisError::AssetHashMismatch {
            asset: "TOS".to_string(),
            expected: TOS_ASSET.to_hex(),
            provided: tos_asset.hash.clone(),
        });
    }
    // TOS decimals must be 8 (COIN_DECIMALS)
    if tos_asset.decimals != 8 {
        return Err(GenesisError::InvalidAssetDecimals {
            asset: "TOS".to_string(),
            expected: 8,
            provided: tos_asset.decimals,
        });
    }

    // Validate UNO asset hash and decimals match required constants
    let uno_asset = state
        .assets
        .get("UNO")
        .ok_or_else(|| GenesisError::MissingRequiredAsset("UNO".to_string()))?;
    let uno_hash = parse_hash(&uno_asset.hash)?;
    if uno_hash != UNO_ASSET.to_bytes() {
        return Err(GenesisError::AssetHashMismatch {
            asset: "UNO".to_string(),
            expected: UNO_ASSET.to_hex(),
            provided: uno_asset.hash.clone(),
        });
    }
    // UNO decimals must be 8 (COIN_DECIMALS)
    if uno_asset.decimals != 8 {
        return Err(GenesisError::InvalidAssetDecimals {
            asset: "UNO".to_string(),
            expected: 8,
            provided: uno_asset.decimals,
        });
    }

    // Get TOS max_supply for balance validation
    let tos_max_supply = tos_asset
        .max_supply
        .as_ref()
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| GenesisError::InvalidBalance("TOS max_supply overflow".to_string()))?;

    // Parse allocations
    let is_mainnet = network == "mainnet";
    let parsed_alloc = parse_allocations(&state.alloc, is_mainnet, tos_max_supply)?;

    // Build ParsedConfig for state hash computation
    let parsed_config = ParsedConfig {
        chain_id,
        network: network.clone(),
        genesis_timestamp_ms,
        dev_public_key: dev_public_key_bytes,
        forks: parsed_forks.clone(),
    };

    // Compute state hash
    let computed_hash = compute_state_hash(
        state.format_version,
        &parsed_config,
        &parsed_assets,
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

    // Return validated data for use in genesis block construction
    let validated_data = ValidatedGenesisData {
        genesis_timestamp_ms,
        dev_public_key,
        parsed_alloc,
        parsed_assets,
    };

    Ok((computed_hash, validated_data))
}

/// Validated genesis data returned from validation
#[derive(Debug)]
pub struct ValidatedGenesisData {
    pub genesis_timestamp_ms: u64,
    pub dev_public_key: PublicKey,
    pub parsed_alloc: Vec<ParsedAllocEntry>,
    pub parsed_assets: Vec<ParsedAsset>,
}

/// Get whether the network is mainnet based on network string (exact match, lowercase only)
pub fn is_mainnet_network(network: &str) -> bool {
    network == "mainnet"
}

/// Parse config from GenesisConfig for use elsewhere
pub fn parse_config(config: &GenesisConfig) -> Result<ParsedConfig, GenesisError> {
    // Validate string lengths (PLAN-B v1.5: reject if > 255 bytes)
    validate_string_length(&config.network, "network")?;
    for (fork_name, _) in &config.forks {
        validate_string_length(fork_name, "fork name")?;
    }

    let chain_id = config
        .chain_id
        .parse::<u64>()
        .map_err(|_| GenesisError::InvalidChainId(config.chain_id.clone()))?;

    let genesis_timestamp_ms = config
        .genesis_timestamp_ms
        .parse::<u64>()
        .map_err(|_| GenesisError::InvalidTimestamp(config.genesis_timestamp_ms.clone()))?;

    let dev_public_key = parse_public_key(&config.dev_public_key)?;
    let dev_public_key_bytes: [u8; 32] = dev_public_key.as_bytes().to_owned();

    let mut parsed_forks = std::collections::BTreeMap::new();
    for (fork_name, fork_height) in &config.forks {
        let height = fork_height
            .parse::<u64>()
            .map_err(|_| GenesisError::InvalidForkHeight {
                fork: fork_name.clone(),
                value: fork_height.clone(),
            })?;
        parsed_forks.insert(fork_name.clone(), height);
    }

    Ok(ParsedConfig {
        chain_id,
        network: config.network.clone(),
        genesis_timestamp_ms,
        dev_public_key: dev_public_key_bytes,
        forks: parsed_forks,
    })
}

// Helper functions

/// Validate string length does not exceed 255 bytes (u8 length prefix limit)
fn validate_string_length(s: &str, field_name: &str) -> Result<(), GenesisError> {
    if s.len() > MAX_STRING_LENGTH {
        return Err(GenesisError::StringTooLong {
            field: field_name.to_string(),
            length: s.len(),
            max: MAX_STRING_LENGTH,
        });
    }
    Ok(())
}

/// Parse and validate assets from the genesis state
fn parse_assets(
    assets: &std::collections::BTreeMap<String, AssetConfig>,
) -> Result<Vec<ParsedAsset>, GenesisError> {
    let mut parsed = Vec::with_capacity(assets.len());

    for (asset_name, asset_config) in assets {
        // Validate asset name length
        validate_string_length(asset_name, "asset name")?;
        validate_string_length(&asset_config.name, "asset config name")?;
        validate_string_length(&asset_config.ticker, "asset ticker")?;

        // Parse and validate asset hash (64 hex chars = 32 bytes)
        let hash_bytes = parse_hash(&asset_config.hash)?;

        // Parse max_supply if present
        let max_supply =
            match &asset_config.max_supply {
                Some(supply_str) => Some(supply_str.parse::<u64>().map_err(|_| {
                    GenesisError::InvalidBalance(format!("{} max_supply", asset_name))
                })?),
                None => None,
            };

        parsed.push(ParsedAsset {
            hash: hash_bytes,
            decimals: asset_config.decimals,
            name: asset_config.name.clone(),
            ticker: asset_config.ticker.clone(),
            max_supply,
        });
    }

    Ok(parsed)
}

/// Parse a 64 hex char string into a 32-byte hash
fn parse_hash(hex: &str) -> Result<[u8; 32], GenesisError> {
    if hex.len() != 64 {
        return Err(GenesisError::InvalidAssetHash(format!(
            "expected 64 hex chars, got {}",
            hex.len()
        )));
    }

    // Validate hex characters
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GenesisError::InvalidAssetHash(
            "contains non-hex characters".to_string(),
        ));
    }

    let bytes = hex::decode(hex).map_err(|e| GenesisError::InvalidAssetHash(e.to_string()))?;

    bytes
        .try_into()
        .map_err(|_| GenesisError::InvalidAssetHash("invalid length".to_string()))
}

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
            },
            AllocEntry {
                public_key: "01".repeat(32), // Duplicate
                address: None,
                nonce: "0".to_string(),
                balance: "2000".to_string(),
            },
        ];

        let result = parse_allocations(&alloc, false, None);
        assert!(matches!(result, Err(GenesisError::DuplicatePublicKey(_))));
    }

    #[test]
    fn test_parse_allocations_overflow_check() {
        // Create entries that would overflow max_supply
        let alloc = vec![
            AllocEntry {
                public_key: "01".repeat(32),
                address: None,
                nonce: "0".to_string(),
                balance: "1000".to_string(),
            },
            AllocEntry {
                public_key: "02".repeat(32),
                address: None,
                nonce: "0".to_string(),
                balance: "1000".to_string(),
            },
        ];

        // With max_supply of 1500, total 2000 should fail
        let result = parse_allocations(&alloc, false, Some(1500));
        assert!(matches!(result, Err(GenesisError::BalanceOverflow)));
    }

    #[test]
    fn test_is_mainnet_network() {
        // Only exact lowercase matches should work (PLAN-B spec)
        assert!(is_mainnet_network("mainnet"));
        assert!(!is_mainnet_network("MAINNET")); // Reject non-lowercase
        assert!(!is_mainnet_network("Mainnet")); // Reject non-lowercase
        assert!(!is_mainnet_network("testnet"));
        assert!(!is_mainnet_network("devnet"));
    }

    #[test]
    fn test_validate_string_length() {
        // Valid string
        assert!(validate_string_length("devnet", "network").is_ok());

        // Exactly 255 chars is OK
        let max_string = "a".repeat(255);
        assert!(validate_string_length(&max_string, "field").is_ok());

        // 256 chars should fail
        let too_long = "a".repeat(256);
        assert!(matches!(
            validate_string_length(&too_long, "field"),
            Err(GenesisError::StringTooLong { .. })
        ));
    }

    #[test]
    fn test_parse_hash_valid() {
        let hex = "00".repeat(32);
        let result = parse_hash(&hex);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), [0u8; 32]);
    }

    #[test]
    fn test_parse_hash_invalid_length() {
        let hex = "0102030405";
        let result = parse_hash(hex);
        assert!(matches!(result, Err(GenesisError::InvalidAssetHash(_))));
    }

    #[test]
    fn test_load_genesis_state_from_json() {
        use std::io::Write;

        // Create a temporary JSON file with PLAN-B v1.5 compliant schema
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
                "TOS": {
                    "hash": "0000000000000000000000000000000000000000000000000000000000000000",
                    "decimals": 8,
                    "name": "TOS",
                    "ticker": "TOS",
                    "max_supply": "2100000000000000"
                },
                "UNO": {
                    "hash": "0000000000000000000000000000000000000000000000000000000000000001",
                    "decimals": 8,
                    "name": "UNO",
                    "ticker": "UNO",
                    "max_supply": null
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
        assert!(state.assets.contains_key("TOS"));
        assert!(state.assets.contains_key("UNO"));

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

    #[test]
    fn test_load_genesis_state_missing_uno() {
        use std::io::Write;

        // Missing UNO asset
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
                "TOS": {
                    "hash": "0000000000000000000000000000000000000000000000000000000000000000",
                    "decimals": 8,
                    "name": "TOS",
                    "ticker": "TOS",
                    "max_supply": "2100000000000000"
                }
            },
            "alloc": []
        }"#;

        let mut temp_path = std::env::temp_dir();
        temp_path.push("test_genesis_state_missing_uno.json");

        {
            let mut file = std::fs::File::create(&temp_path).unwrap();
            file.write_all(json_content.as_bytes()).unwrap();
        }

        let result = load_genesis_state(&temp_path);
        assert!(matches!(
            result,
            Err(GenesisError::MissingRequiredAsset(ref s)) if s == "UNO"
        ));

        let _ = std::fs::remove_file(&temp_path);
    }
}
