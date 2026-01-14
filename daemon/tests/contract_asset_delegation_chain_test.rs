//! Contract Asset delegation chain tests

#![allow(clippy::disallowed_methods)]

use tempdir::TempDir;
use tos_common::{
    contract_asset::{ContractAssetData, Delegation},
    crypto::Hash,
    network::Network,
};
use tos_daemon::{
    core::{
        config::RocksDBConfig,
        storage::{
            rocksdb::{CacheMode, CompressionMode, RocksStorage},
            ContractAssetProvider,
        },
    },
    tako_integration::TosContractAssetAdapter,
};
use tos_program_runtime::storage::ContractAssetProvider as TakoContractAssetProvider;

fn test_rocksdb_config() -> RocksDBConfig {
    RocksDBConfig {
        parallelism: 2,
        max_background_jobs: 2,
        max_subcompaction_jobs: 1,
        low_priority_background_threads: 1,
        max_open_files: 100,
        keep_max_log_files: 1,
        compression_mode: CompressionMode::None,
        cache_mode: CacheMode::None,
        cache_size: 1024 * 1024,
        write_buffer_size: 1024 * 1024,
        write_buffer_shared: false,
    }
}

fn create_test_storage(temp_dir: &TempDir) -> RocksStorage {
    let config = test_rocksdb_config();
    RocksStorage::new(temp_dir.path().to_str().unwrap(), Network::Devnet, &config)
}

fn random_asset() -> Hash {
    Hash::new(rand::random())
}

fn random_account() -> [u8; 32] {
    rand::random()
}

#[tokio::test]
async fn test_delegate_rejects_chain_delegation() {
    let temp_dir =
        TempDir::new("contract_asset_delegation_chain").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let creator = random_account();
    let asset = random_asset();
    let data = ContractAssetData {
        name: "Chain Delegation Test Token".to_string(),
        symbol: "CDT".to_string(),
        decimals: 8,
        total_supply: 0,
        max_supply: None,
        mintable: true,
        burnable: true,
        pausable: false,
        freezable: false,
        governance: true,
        creator,
        admin: creator,
        created_at: 1,
        metadata_uri: None,
    };

    storage
        .set_contract_asset(&asset, &data)
        .await
        .expect("Failed to set asset data");

    let delegator = random_account();
    let delegatee = random_account();
    let final_delegatee = random_account();

    storage
        .set_contract_asset_balance(&asset, &delegator, 100)
        .await
        .expect("Failed to set delegator balance");

    storage
        .set_contract_asset_delegation(
            &asset,
            &delegatee,
            &Delegation {
                delegatee: Some(final_delegatee),
                from_block: 1,
            },
        )
        .await
        .expect("Failed to set delegatee delegation");

    let asset_bytes = *asset.as_bytes();
    let mut adapter = TosContractAssetAdapter::new(&mut storage, 100);

    let err = adapter
        .delegate(&asset_bytes, &delegator, &delegatee)
        .expect_err("Chain delegation should be rejected");
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("chain delegation"),
        "Unexpected error message: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_delegate_allows_direct_delegation() {
    let temp_dir =
        TempDir::new("contract_asset_delegation_direct").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let creator = random_account();
    let asset = random_asset();
    let data = ContractAssetData {
        name: "Direct Delegation Test Token".to_string(),
        symbol: "DDT".to_string(),
        decimals: 8,
        total_supply: 0,
        max_supply: None,
        mintable: true,
        burnable: true,
        pausable: false,
        freezable: false,
        governance: true,
        creator,
        admin: creator,
        created_at: 1,
        metadata_uri: None,
    };

    storage
        .set_contract_asset(&asset, &data)
        .await
        .expect("Failed to set asset data");

    let delegator = random_account();
    let delegatee = random_account();

    storage
        .set_contract_asset_balance(&asset, &delegator, 100)
        .await
        .expect("Failed to set delegator balance");

    let asset_bytes = *asset.as_bytes();
    let mut adapter = TosContractAssetAdapter::new(&mut storage, 100);

    adapter
        .delegate(&asset_bytes, &delegator, &delegatee)
        .expect("Direct delegation should succeed");

    let delegation = storage
        .get_contract_asset_delegation(&asset, &delegator)
        .await
        .expect("Failed to read delegation");
    assert_eq!(delegation.delegatee, Some(delegatee));
}
