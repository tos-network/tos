use super::types::ParsedAllocEntry;
use std::collections::BTreeMap;
use tos_common::{
    crypto::{hash, Hash},
    serializer::Writer,
};

/// Parsed config values for state hash computation
#[derive(Debug)]
pub struct ParsedConfig {
    pub chain_id: u64,
    pub network: String,
    pub genesis_timestamp_ms: u64,
    pub dev_public_key: [u8; 32],
    pub forks: BTreeMap<String, u64>,
}

/// Parsed asset data for state hash computation
#[derive(Debug)]
pub struct ParsedAsset {
    pub hash: [u8; 32],
    pub decimals: u8,
    pub name: String,
    pub ticker: String,
    pub max_supply: Option<u64>,
}

/// Compute the canonical state hash for genesis state verification.
///
/// The hash is computed using blake3 over a canonical big-endian serialization:
/// - format_version (u32, BE)
/// - config_bytes (chain_id u64 BE, network u8+utf8, timestamp u64 BE, dev_public_key 32 bytes, forks)
/// - assets_bytes (sorted by asset hash)
/// - alloc_bytes (sorted by public_key)
pub fn compute_state_hash(
    format_version: u32,
    config: &ParsedConfig,
    assets: &[ParsedAsset],
    alloc: &[ParsedAllocEntry],
) -> Hash {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);

    // 1. Format version (u32, BE)
    writer.write_u32(&format_version);

    // 2. Config bytes
    write_config_bytes(&mut writer, config);

    // 3. Assets bytes (sorted by hash)
    write_assets_bytes(&mut writer, assets);

    // 4. Alloc bytes (must be sorted by public_key for determinism)
    write_alloc_bytes(&mut writer, alloc);

    hash(&bytes)
}

/// Serialize config section in canonical order (PLAN-B v1.5 spec)
fn write_config_bytes(writer: &mut Writer, config: &ParsedConfig) {
    // chain_id (u64, BE) - parsed from string
    writer.write_u64(&config.chain_id);

    // network_len (u8) || network_utf8
    writer.write_u8(config.network.len() as u8);
    writer.write_bytes(config.network.as_bytes());

    // genesis_timestamp_ms (u64, BE) - parsed from string
    writer.write_u64(&config.genesis_timestamp_ms);

    // dev_public_key (32 bytes) - Ristretto compressed
    writer.write_bytes(&config.dev_public_key);

    // forks_count (u32, BE)
    writer.write_u32(&(config.forks.len() as u32));

    // forks sorted by name: fork_name_len (u8) || fork_name_utf8 || activation_height (u64, BE)
    let mut sorted_forks: Vec<_> = config.forks.iter().collect();
    sorted_forks.sort_by_key(|(name, _)| *name);
    for (fork_name, fork_height) in sorted_forks {
        writer.write_u8(fork_name.len() as u8);
        writer.write_bytes(fork_name.as_bytes());
        writer.write_u64(fork_height);
    }
}

/// Serialize assets section in canonical order (sorted by hash)
fn write_assets_bytes(writer: &mut Writer, assets: &[ParsedAsset]) {
    // Sort assets by hash
    let mut sorted_assets: Vec<_> = assets.iter().collect();
    sorted_assets.sort_by_key(|a| a.hash);

    // asset_count (u32, BE)
    writer.write_u32(&(sorted_assets.len() as u32));

    for asset in sorted_assets {
        // hash (32 bytes)
        writer.write_bytes(&asset.hash);

        // decimals (u8)
        writer.write_u8(asset.decimals);

        // name_len (u8) || name_utf8
        writer.write_u8(asset.name.len() as u8);
        writer.write_bytes(asset.name.as_bytes());

        // ticker_len (u8) || ticker_utf8
        writer.write_u8(asset.ticker.len() as u8);
        writer.write_bytes(asset.ticker.as_bytes());

        // has_max_supply (u8: 0 or 1) || if present: max_supply (u64, BE)
        match asset.max_supply {
            Some(supply) => {
                writer.write_u8(1);
                writer.write_u64(&supply);
            }
            None => {
                writer.write_u8(0);
            }
        }
    }
}

/// Serialize alloc section in canonical order (sorted by public_key)
fn write_alloc_bytes(writer: &mut Writer, alloc: &[ParsedAllocEntry]) {
    // Create a sorted copy by public key bytes
    let mut sorted_alloc: Vec<&ParsedAllocEntry> = alloc.iter().collect();
    sorted_alloc.sort_by_key(|entry| entry.public_key.as_bytes());

    // account_count (u32, BE)
    writer.write_u32(&(sorted_alloc.len() as u32));

    // alloc entries
    for entry in sorted_alloc {
        // public_key (32 bytes)
        writer.write_bytes(entry.public_key.as_bytes());

        // nonce (u64, BE)
        writer.write_u64(&entry.nonce);

        // balance (u64, BE)
        writer.write_u64(&entry.balance);
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use tos_common::{crypto::elgamal::CompressedPublicKey, serializer::Serializer};

    fn create_test_config() -> ParsedConfig {
        let mut forks = BTreeMap::new();
        forks.insert("test_fork".to_string(), 100u64);

        ParsedConfig {
            chain_id: 1,
            network: "devnet".to_string(),
            genesis_timestamp_ms: 1700000000000,
            dev_public_key: [0xaa; 32],
            forks,
        }
    }

    fn create_test_assets() -> Vec<ParsedAsset> {
        vec![ParsedAsset {
            hash: [0u8; 32],
            decimals: 8,
            name: "TOS".to_string(),
            ticker: "TOS".to_string(),
            max_supply: Some(100000000),
        }]
    }

    fn create_test_alloc() -> Vec<ParsedAllocEntry> {
        let pub_key = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        vec![ParsedAllocEntry {
            public_key: pub_key,
            nonce: 0,
            balance: 1000,
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
