#![allow(clippy::disallowed_methods)]

use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;
use tokio::time::sleep;
use tos_common::{
    account::{AgentAccountMeta, VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{Address, Hash, KeyPair},
    network::Network,
    versioned_type::Versioned,
};
use tos_daemon::core::{
    blockchain::Blockchain,
    config::{Config, RocksDBConfig},
    storage::{AgentAccountProvider, AssetProvider, BalanceProvider, NonceProvider, RocksStorage},
};
use tos_daemon::rpc::DaemonRpcServer;

fn get_wallet_binary_path() -> Option<String> {
    if let Ok(path) = std::env::var("TOS_WALLET_BIN") {
        return Some(path);
    }

    let release_path = "../target/release/tos_wallet";
    if std::path::Path::new(release_path).exists() {
        return Some(release_path.to_string());
    }

    let debug_path = "../target/debug/tos_wallet";
    if std::path::Path::new(debug_path).exists() {
        return Some(debug_path.to_string());
    }

    None
}

fn pick_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

async fn start_daemon(
    dir: &TempDir,
    owner: &Address,
) -> anyhow::Result<(std::sync::Arc<DaemonRpcServer<RocksStorage>>, String)> {
    let mut config: Config = serde_json::from_value(serde_json::json!({
        "rpc": { "getwork": {}, "prometheus": {} },
        "p2p": { "proxy": {} },
        "rocksdb": {},
        "vrf": {}
    }))
    .expect("daemon config");
    config.rpc.disable = false;
    config.rpc.getwork.disable = true;
    config.p2p.disable = true;
    config.skip_pow_verification = true;
    config.dir_path = Some(format!("{}/", dir.path().to_string_lossy()));
    config.rocksdb = RocksDBConfig::default();

    let storage = RocksStorage::new(
        &dir.path().to_string_lossy(),
        Network::Devnet,
        &config.rocksdb,
    );
    let blockchain = Blockchain::new(config.clone(), Network::Devnet, storage)
        .await
        .expect("create blockchain");

    {
        let mut storage = blockchain.get_storage().write().await;
        let asset_data = AssetData::new(
            COIN_DECIMALS,
            "TOS".to_string(),
            "TOS".to_string(),
            None,
            None,
        );
        let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
        storage
            .add_asset(&TOS_ASSET, 0, versioned)
            .await
            .expect("register TOS asset");

        let owner_pub = owner.get_public_key();
        storage
            .set_last_nonce_to(owner_pub, 0, &VersionedNonce::new(0, Some(0)))
            .await
            .expect("set nonce");
        storage
            .set_last_balance_to(
                owner_pub,
                &TOS_ASSET,
                0,
                &VersionedBalance::new(1000, Some(0)),
            )
            .await
            .expect("set balance");

        let controller = KeyPair::new().get_public_key().compress();
        let meta = AgentAccountMeta {
            owner: owner_pub.clone(),
            controller,
            policy_hash: Hash::new([1u8; 32]),
            status: 0,
            energy_pool: None,
            session_key_root: None,
        };
        storage
            .set_agent_account_meta(owner_pub, &meta)
            .await
            .expect("set agent meta");
    }

    let mut last_error = None;
    for _ in 0..5 {
        let port = pick_free_port();
        config.rpc.bind_address = format!("127.0.0.1:{port}");
        match DaemonRpcServer::new(blockchain.clone(), config.rpc.clone()).await {
            Ok(rpc_server) => {
                let address = format!("http://127.0.0.1:{port}");
                // Wait for actix server to bind.
                sleep(Duration::from_millis(100)).await;
                return Ok((rpc_server, address));
            }
            Err(err) => {
                if err.to_string().contains("Address already in use") {
                    last_error = Some(err);
                    continue;
                }
                return Err(anyhow::anyhow!("start rpc: {err}"));
            }
        }
    }
    Err(anyhow::anyhow!(
        "start rpc: {}",
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    ))
}

fn parse_wallet_address(output: &str) -> Option<String> {
    let marker = "Wallet address:";
    output.lines().find_map(|line| {
        let start = line.find(marker)?;
        let rest = &line[start + marker.len()..];
        let trimmed = rest.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wallet_agent_show_end_to_end() -> anyhow::Result<()> {
    let Some(wallet_bin) = get_wallet_binary_path() else {
        println!("Wallet binary not found, skipping agent account e2e test");
        return Ok(());
    };

    let wallet_dir = TempDir::new().expect("wallet dir");
    let daemon_dir = TempDir::new().expect("daemon dir");

    let output = Command::new(&wallet_bin)
        .args([
            "--network",
            "devnet",
            "--precomputed-tables-l1",
            "13",
            "--exec",
            "display_address",
            "--wallet-path",
            wallet_dir.path().to_str().expect("wallet path"),
            "--password",
            "test123",
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "wallet display_address failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let address_str = parse_wallet_address(&format!("{stdout}\n{stderr}")).expect("wallet address");
    let address = Address::from_string(&address_str)?;

    let (rpc_server, daemon_addr) = start_daemon(&daemon_dir, &address).await?;

    let output = Command::new(&wallet_bin)
        .args([
            "--network",
            "devnet",
            "--precomputed-tables-l1",
            "13",
            "--daemon-address",
            &daemon_addr,
            "--exec",
            "agent_show",
            "--wallet-path",
            wallet_dir.path().to_str().expect("wallet path"),
            "--password",
            "test123",
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "wallet agent_show failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("AgentAccount:"),
        "expected agent_show output to include AgentAccount"
    );
    assert!(
        stdout.contains("Owner:"),
        "expected agent_show output to include Owner"
    );

    rpc_server.stop().await;

    Ok(())
}
