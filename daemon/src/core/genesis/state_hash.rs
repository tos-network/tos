use super::types::{AssetConfig, GenesisConfig, ParsedAllocEntry};
use std::collections::BTreeMap;
use tos_common::{
    crypto::{hash, Hash},
    serializer::Writer,
};

/// Compute the canonical state hash for genesis state verification.
///
/// The hash is computed using blake3 over a canonical big-endian serialization:
/// - format_version (u32, BE)
/// - config_bytes (chain_id, network, timestamp, dev_public_key, forks)
/// - assets_bytes (sorted by asset hash)
/// - alloc_bytes (sorted by public_key hex)
pub fn compute_state_hash(
    format_version: u32,
    config: &GenesisConfig,
    assets: &BTreeMap<String, AssetConfig>,
    alloc: &[ParsedAllocEntry],
) -> Hash {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);

    // 1. Format version (u32, BE)
    writer.write_u32(&format_version);

    // 2. Config bytes
    write_config_bytes(&mut writer, config);

    // 3. Assets bytes (BTreeMap is already sorted by key)
    write_assets_bytes(&mut writer, assets);

    // 4. Alloc bytes (must be sorted by public_key for determinism)
    write_alloc_bytes(&mut writer, alloc);

    hash(&bytes)
}

/// Serialize config section in canonical order
fn write_config_bytes(writer: &mut Writer, config: &GenesisConfig) {
    // chain_id as string
    writer.write_string(&config.chain_id);

    // network as string
    writer.write_string(&config.network);

    // genesis_timestamp_ms as string
    writer.write_string(&config.genesis_timestamp_ms);

    // dev_public_key as string (hex)
    writer.write_string(&config.dev_public_key);

    // forks count
    writer.write_u32(&(config.forks.len() as u32));

    // forks (BTreeMap is already sorted by key)
    for (fork_name, fork_height) in &config.forks {
        writer.write_string(fork_name);
        writer.write_string(fork_height);
    }
}

/// Serialize assets section in canonical order
fn write_assets_bytes(writer: &mut Writer, assets: &BTreeMap<String, AssetConfig>) {
    // assets count
    writer.write_u32(&(assets.len() as u32));

    // assets (BTreeMap is already sorted by key)
    for (asset_hash, asset_config) in assets {
        // asset hash as string
        writer.write_string(asset_hash);

        // decimals
        writer.write_u8(asset_config.decimals);

        // name
        writer.write_string(&asset_config.name);

        // ticker
        writer.write_string(&asset_config.ticker);

        // max_supply (optional)
        match &asset_config.max_supply {
            Some(supply) => {
                writer.write_u8(1); // present
                writer.write_string(supply);
            }
            None => {
                writer.write_u8(0); // not present
            }
        }
    }
}

/// Serialize alloc section in canonical order (sorted by public_key)
fn write_alloc_bytes(writer: &mut Writer, alloc: &[ParsedAllocEntry]) {
    // Create a sorted copy by public key bytes
    let mut sorted_alloc: Vec<&ParsedAllocEntry> = alloc.iter().collect();
    sorted_alloc.sort_by_key(|entry| entry.public_key.as_bytes());

    // alloc count
    writer.write_u32(&(sorted_alloc.len() as u32));

    // alloc entries
    for entry in sorted_alloc {
        // public_key (32 bytes)
        writer.write_bytes(entry.public_key.as_bytes());

        // nonce (u64, BE)
        writer.write_u64(&entry.nonce);

        // balance (u64, BE)
        writer.write_u64(&entry.balance);

        // energy_available (u64, BE)
        writer.write_u64(&entry.energy_available);
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use tos_common::{crypto::elgamal::CompressedPublicKey, serializer::Serializer};

    fn create_test_config() -> GenesisConfig {
        let mut forks = BTreeMap::new();
        forks.insert("test_fork".to_string(), "100".to_string());

        GenesisConfig {
            chain_id: "1".to_string(),
            network: "devnet".to_string(),
            genesis_timestamp_ms: "1700000000000".to_string(),
            dev_public_key: "a".repeat(64),
            forks,
        }
    }

    fn create_test_assets() -> BTreeMap<String, AssetConfig> {
        let mut assets = BTreeMap::new();
        assets.insert(
            "0".repeat(64),
            AssetConfig {
                decimals: 8,
                name: "TOS".to_string(),
                ticker: "TOS".to_string(),
                max_supply: Some("100000000".to_string()),
            },
        );
        assets
    }

    fn create_test_alloc() -> Vec<ParsedAllocEntry> {
        let pub_key = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        vec![ParsedAllocEntry {
            public_key: pub_key,
            nonce: 0,
            balance: 1000,
            energy_available: 0,
        }]
    }

    #[test]
    fn test_state_hash_deterministic() {
        let config = create_test_config();
        let assets = create_test_assets();
        let alloc = create_test_alloc();

        let hash1 = compute_state_hash(1, &config, &assets, &alloc);
        let hash2 = compute_state_hash(1, &config, &assets, &alloc);

        assert_eq!(hash1, hash2, "State hash should be deterministic");
    }

    #[test]
    fn test_state_hash_differs_with_different_input() {
        let config = create_test_config();
        let assets = create_test_assets();
        let alloc = create_test_alloc();

        let hash1 = compute_state_hash(1, &config, &assets, &alloc);
        let hash2 = compute_state_hash(2, &config, &assets, &alloc);

        assert_ne!(
            hash1, hash2,
            "State hash should differ with different format_version"
        );
    }
}
