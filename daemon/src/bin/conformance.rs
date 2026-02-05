use actix_web::{web, App, HttpResponse, HttpServer};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use tos_common::account::{EnergyResource, VersionedBalance, VersionedNonce};
use tos_common::asset::{AssetData, VersionedAssetData};
use tos_common::block::Block;
use tos_common::config::{COIN_DECIMALS, MAXIMUM_SUPPLY, TOS_ASSET, UNO_ASSET};
use tos_common::crypto::{hash as blake3_hash, Hash, Hashable, PublicKey, Signature};
use tos_common::network::Network;
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    extra_data::UnknownExtraDataFormat, FeeType, Reference, Transaction, TransactionType, TxVersion,
};

use tos_common::crypto::elgamal::KeyPair;
use tos_crypto::curve25519_dalek::ristretto::CompressedRistretto;
use tos_daemon::core::blockchain::Blockchain;
use tos_daemon::core::blockchain::BroadcastOption;
use tos_daemon::core::config::Config;
use tos_daemon::core::error::BlockchainError;
use tos_daemon::core::state::ApplicableChainState;
use tos_daemon::core::storage::rocksdb::RocksStorage;
use tos_daemon::core::storage::{
    AccountProvider, AssetProvider, BalanceProvider, EnergyProvider, NonceProvider,
};
use tos_daemon::vrf::WrappedMinerSecret;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct GlobalState {
    #[serde(default)]
    total_supply: u64,
    #[serde(default)]
    total_burned: u64,
    #[serde(default)]
    total_energy: u64,
    #[serde(default)]
    block_height: u64,
    #[serde(default)]
    timestamp: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct AccountState {
    address: String,
    #[serde(default)]
    balance: u64,
    #[serde(default)]
    nonce: u64,
    #[serde(default)]
    frozen: u64,
    #[serde(default)]
    energy: u64,
    #[serde(default)]
    flags: u64,
    #[serde(default)]
    data: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PreState {
    #[serde(default)]
    network_chain_id: u64,
    #[serde(default)]
    global_state: GlobalState,
    #[serde(default)]
    accounts: Vec<AccountState>,
}

#[derive(Clone, Debug, Default)]
struct MetaState {
    network_chain_id: u64,
    global_state: GlobalState,
    account_meta: HashMap<String, AccountState>,
}

#[derive(Clone)]
struct Engine {
    base_dir: PathBuf,
    network: Network,
    reset_nonce: u64,
    blockchain: Arc<Blockchain<RocksStorage>>,
    meta: MetaState,
}

#[derive(Clone)]
struct AppState {
    engine: Arc<Mutex<Engine>>,
}

#[derive(Deserialize)]
struct TxExecuteRequest {
    #[serde(default)]
    wire_hex: String,
    #[serde(default)]
    tx: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct BlockExecuteRequest {
    #[serde(default)]
    wire_hex: String,
}

#[derive(Serialize)]
struct ExecResult {
    success: bool,
    error_code: u16,
    #[serde(default)]
    state_digest: String,
}

#[derive(Deserialize)]
struct AccountsFile {
    accounts: Vec<AccountEntry>,
}

#[derive(Deserialize)]
struct AccountEntry {
    name: String,
    private_key: String,
    public_key: String,
    address: String,
}

fn default_accounts_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LABU_ACCOUNTS_PATH") {
        return Some(PathBuf::from(path));
    }
    let cwd = std::env::current_dir().ok()?;
    let candidate = cwd.join("../tos-spec/vectors/accounts.json");
    if candidate.exists() {
        return Some(candidate);
    }
    None
}

fn load_accounts() -> Vec<AccountEntry> {
    let Some(path) = default_accounts_path() else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<AccountsFile>(&raw)
        .map(|f| f.accounts)
        .unwrap_or_default()
}

fn miner_private_key_hex() -> String {
    if let Ok(value) = std::env::var("MINER_PRIVATE_KEY") {
        return value;
    }
    for acc in load_accounts() {
        if acc.name == "Miner" && !acc.private_key.is_empty() {
            return acc.private_key;
        }
    }
    "0100000000000000000000000000000000000000000000000000000000000000".to_string()
}

fn miner_public_key() -> PublicKey {
    let miner_secret = miner_private_key_hex();
    WrappedMinerSecret::from_str(&miner_secret)
        .map(|k| k.keypair().get_public_key().compress())
        .unwrap_or_else(|_| KeyPair::new().get_public_key().compress())
}

fn ensure_trailing_slash(mut path: String) -> String {
    if !path.ends_with('/') {
        path.push('/');
    }
    path
}

fn to_public_key(addr_hex: &str) -> Result<PublicKey, String> {
    let bytes = hex::decode(addr_hex).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err(format!("address must be 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let compressed = CompressedRistretto::from_slice(&arr).map_err(|_| "invalid address bytes")?;
    Ok(PublicKey::new(compressed))
}

fn public_key_to_hex(key: &PublicKey) -> String {
    hex::encode(key.as_bytes())
}

fn compute_state_digest(state: &PreState) -> String {
    let mut buf = Vec::new();
    let gs = &state.global_state;
    for value in [
        gs.total_supply,
        gs.total_burned,
        gs.total_energy,
        gs.block_height,
        gs.timestamp,
    ] {
        buf.extend_from_slice(&value.to_be_bytes());
    }
    let mut accounts = state.accounts.clone();
    accounts.sort_by_key(|a| hex::decode(&a.address).unwrap_or_default());
    for acc in accounts {
        let addr = hex::decode(acc.address).unwrap_or_default();
        buf.extend_from_slice(&addr);
        for value in [acc.balance, acc.nonce, acc.frozen, acc.energy, acc.flags] {
            buf.extend_from_slice(&value.to_be_bytes());
        }
        let data = hex::decode(acc.data).unwrap_or_default();
        buf.extend_from_slice(&(data.len() as u64).to_be_bytes());
        buf.extend_from_slice(&data);
    }
    blake3_hash(&buf).to_hex()
}

fn map_error_code(err: &BlockchainError) -> u16 {
    match err {
        BlockchainError::AccountNotFound(_) | BlockchainError::NoBalance(_) => 0x0400,
        BlockchainError::InvalidNonce(_, got, expected) => {
            if got > expected {
                0x0111
            } else {
                0x0110
            }
        }
        BlockchainError::InvalidTxNonce(_, got, expected, _) => {
            if got > expected {
                0x0111
            } else {
                0x0110
            }
        }
        BlockchainError::InvalidTransactionNonce(got, expected) => {
            if got > expected {
                0x0111
            } else {
                0x0110
            }
        }
        BlockchainError::InvalidTransactionSignature => 0x0103,
        BlockchainError::InvalidTransactionFormat => 0x0100,
        BlockchainError::InvalidTxFee(_, _) => 0x0301,
        BlockchainError::InvalidTxVersion => 0x0101,
        BlockchainError::InvalidTransactionToSender(_) => 0x0409,
        BlockchainError::TxTooBig(_, _) => 0x0100,
        BlockchainError::InvalidTxInBlock(_) => 0x0107,
        BlockchainError::InvalidTransactionExtraData => 0x0107,
        BlockchainError::InvalidTransferExtraData => 0x0107,
        BlockchainError::Any(err) => {
            let msg = err.to_string();
            if msg.contains("Insufficient funds") {
                0x0400
            } else if msg.contains("Invalid chain ID") {
                0x0102
            } else {
                0xFFFF
            }
        }
        _ => 0xFFFF,
    }
}

fn map_verify_error_code(
    err: &tos_common::transaction::verify::VerificationError<BlockchainError>,
) -> u16 {
    use tos_common::transaction::verify::VerificationError as VE;
    match err {
        VE::State(inner) => map_error_code(inner),
        VE::InvalidNonce(_, got, expected) => {
            if got > expected {
                0x0111
            } else {
                0x0110
            }
        }
        VE::InvalidSignature => 0x0103,
        VE::InvalidChainId { .. } => 0x0102,
        VE::InvalidFee(_, _) => 0x0301,
        VE::InsufficientFunds { .. } => 0x0400,
        VE::TransferExtraDataSize | VE::TransactionExtraDataSize | VE::InvalidFormat => 0x0100,
        _ => 0xFFFF,
    }
}

async fn create_blockchain(
    base_dir: &PathBuf,
    network: Network,
    reset_nonce: u64,
) -> anyhow::Result<Arc<Blockchain<RocksStorage>>> {
    let cfg_value = json!({
        "rpc": {
            "disable": true,
            "threads": 1,
            "getwork": { "disable": true },
            "prometheus": { "enable": false }
        },
        "p2p": { "disable": true, "proxy": { "address": null, "kind": null, "username": null, "password": null } },
        "rocksdb": {},
        "vrf": {}
    });
    let mut config: Config = serde_json::from_value(cfg_value)?;
    config.skip_pow_verification = true;
    let miner_key = miner_private_key_hex();
    config.vrf.miner_private_key = WrappedMinerSecret::from_str(&miner_key).ok();

    let run_dir = base_dir.join(format!("run{}", reset_nonce));
    fs::create_dir_all(&run_dir)?;
    config.dir_path = Some(ensure_trailing_slash(run_dir.display().to_string()));

    let storage = RocksStorage::new(config.dir_path.as_ref().unwrap(), network, &config.rocksdb);
    let blockchain = Blockchain::new(config, network, storage).await?;
    Ok(blockchain)
}

async fn reset_engine(state: &AppState) -> anyhow::Result<()> {
    let mut engine = state.engine.lock().await;
    if !engine.base_dir.exists() {
        fs::create_dir_all(&engine.base_dir)?;
    }
    engine.reset_nonce = engine.reset_nonce.wrapping_add(1);
    engine.blockchain =
        create_blockchain(&engine.base_dir, engine.network, engine.reset_nonce).await?;
    engine.meta = MetaState::default();
    Ok(())
}

async fn handle_health() -> HttpResponse {
    HttpResponse::Ok().body("OK\n")
}

async fn handle_state_reset(state: web::Data<AppState>) -> HttpResponse {
    match reset_engine(&state).await {
        Ok(()) => HttpResponse::Ok().json(json!({ "success": true })),
        Err(err) => HttpResponse::InternalServerError()
            .json(json!({ "success": false, "error": err.to_string() })),
    }
}

async fn handle_state_load(state: web::Data<AppState>, body: web::Json<PreState>) -> HttpResponse {
    if let Err(err) = reset_engine(&state).await {
        return HttpResponse::InternalServerError()
            .json(json!({ "success": false, "error": err.to_string() }));
    }

    let pre_state = body.into_inner();
    let mut engine = state.engine.lock().await;
    engine.meta.network_chain_id = pre_state.network_chain_id;
    engine.meta.global_state = pre_state.global_state.clone();
    engine.meta.account_meta = pre_state
        .accounts
        .iter()
        .map(|acc| (acc.address.clone(), acc.clone()))
        .collect();

    let topoheight = 0u64;
    {
        let mut storage = engine.blockchain.get_storage().write().await;

        if !storage.has_asset(&TOS_ASSET).await.unwrap_or(false) {
            let _ = storage
                .add_asset(
                    &TOS_ASSET,
                    0,
                    VersionedAssetData::new(
                        AssetData::new(
                            COIN_DECIMALS,
                            "TOS".to_owned(),
                            "TOS".to_owned(),
                            Some(MAXIMUM_SUPPLY),
                            None,
                        ),
                        None,
                    ),
                )
                .await;
        }
        if !storage.has_asset(&UNO_ASSET).await.unwrap_or(false) {
            let _ = storage
                .add_asset(
                    &UNO_ASSET,
                    1,
                    VersionedAssetData::new(
                        AssetData::new(
                            COIN_DECIMALS,
                            "UNO".to_owned(),
                            "UNO".to_owned(),
                            None,
                            None,
                        ),
                        None,
                    ),
                )
                .await;
        }

        let mut loaded_accounts = Vec::with_capacity(pre_state.accounts.len());
        for acc in &pre_state.accounts {
            let key = match to_public_key(&acc.address) {
                Ok(key) => key,
                Err(err) => {
                    return HttpResponse::BadRequest()
                        .json(json!({ "success": false, "error": err }));
                }
            };
            loaded_accounts.push((key, acc.clone()));
        }

        for (key, acc) in &loaded_accounts {
            if acc.frozen > 0 {
                return HttpResponse::BadRequest().json(json!({
                    "success": false,
                    "error": "frozen_tos is not supported in conformance state/load (genesis semantics require frozen_tos = 0)"
                }));
            }
            if let Err(err) = storage
                .set_account_registration_topoheight(key, topoheight)
                .await
            {
                return HttpResponse::InternalServerError()
                    .json(json!({ "success": false, "error": err.to_string() }));
            }
            let nonce = VersionedNonce::new(acc.nonce, None);
            if let Err(err) = storage.set_last_nonce_to(key, topoheight, &nonce).await {
                return HttpResponse::InternalServerError()
                    .json(json!({ "success": false, "error": err.to_string() }));
            }
            let balance = VersionedBalance::new(acc.balance, None);
            if let Err(err) = storage
                .set_last_balance_to(key, &TOS_ASSET, topoheight, &balance)
                .await
            {
                return HttpResponse::InternalServerError()
                    .json(json!({ "success": false, "error": err.to_string() }));
            }
            let mut energy = EnergyResource::new();
            energy.energy = acc.energy;
            energy.last_update = topoheight;
            if let Err(err) = storage.set_energy_resource(&key, topoheight, &energy).await {
                return HttpResponse::InternalServerError()
                    .json(json!({ "success": false, "error": err.to_string() }));
            }
        }

        let miner_key = miner_public_key();
        let _ = storage
            .set_account_registration_topoheight(&miner_key, topoheight)
            .await;
        let miner_nonce = VersionedNonce::new(0, None);
        let _ = storage
            .set_last_nonce_to(&miner_key, topoheight, &miner_nonce)
            .await;
        let miner_balance = VersionedBalance::new(0, None);
        let _ = storage
            .set_last_balance_to(&miner_key, &TOS_ASSET, topoheight, &miner_balance)
            .await;
        let mut miner_energy = EnergyResource::new();
        miner_energy.last_update = topoheight;
        let _ = storage
            .set_energy_resource(&miner_key, topoheight, &miner_energy)
            .await;
    }
    if let Err(err) = engine.blockchain.reload_from_disk().await {
        return HttpResponse::InternalServerError()
            .json(json!({ "success": false, "error": err.to_string() }));
    }

    let export = build_export(&engine).await;
    let digest = compute_state_digest(&export);
    HttpResponse::Ok().json(json!({ "success": true, "state_digest": digest }))
}

async fn handle_state_export(state: web::Data<AppState>) -> HttpResponse {
    let engine = state.engine.lock().await;
    let export = build_export(&engine).await;
    HttpResponse::Ok().json(export)
}

async fn handle_state_digest(state: web::Data<AppState>) -> HttpResponse {
    let engine = state.engine.lock().await;
    let export = build_export(&engine).await;
    let digest = compute_state_digest(&export);
    HttpResponse::Ok().json(json!({ "state_digest": digest }))
}

async fn handle_tx_execute(
    state: web::Data<AppState>,
    body: web::Json<TxExecuteRequest>,
) -> HttpResponse {
    let tx = if !body.wire_hex.trim().is_empty() {
        match Transaction::from_hex(body.wire_hex.trim()) {
            Ok(tx) => tx,
            Err(err) => {
                return HttpResponse::BadRequest().json(json!({
                    "success": false,
                    "error": err.to_string(),
                    "error_code": 0x0100
                }));
            }
        }
    } else if let Some(tx_json) = &body.tx {
        match tx_from_json(tx_json) {
            Ok(tx) => tx,
            Err(err) => {
                return HttpResponse::BadRequest().json(json!({
                    "success": false,
                    "error": err,
                    "error_code": 0x0100
                }));
            }
        }
    } else {
        return HttpResponse::BadRequest().json(
            json!({ "success": false, "error": "missing wire_hex or tx", "error_code": 0xFF00 }),
        );
    };

    let tx_hash = tx.hash();
    let tx_for_apply = tx.clone();

    let mut engine = state.engine.lock().await;
    if let TransactionType::Transfers(payloads) = tx_for_apply.get_data() {
        let source_addr = public_key_to_hex(tx_for_apply.get_source());
        engine
            .meta
            .account_meta
            .entry(source_addr.clone())
            .or_insert_with(|| AccountState {
                address: source_addr,
                ..AccountState::default()
            });
        for payload in payloads {
            let dest_addr = public_key_to_hex(payload.get_destination());
            engine
                .meta
                .account_meta
                .entry(dest_addr.clone())
                .or_insert_with(|| AccountState {
                    address: dest_addr,
                    ..AccountState::default()
                });
        }
    }
    if let Err(err) = engine.blockchain.add_tx_to_mempool(tx, false).await {
        eprintln!("conformance tx_execute add_tx_to_mempool error: {err}");
        let code = map_error_code(&err);
        return HttpResponse::Ok().json(ExecResult {
            success: false,
            error_code: code,
            state_digest: String::new(),
        });
    }

    let miner_key = miner_public_key();
    let header = {
        let storage = engine.blockchain.get_storage().read().await;
        match engine
            .blockchain
            .get_block_template_for_storage(&*storage, miner_key)
            .await
        {
            Ok(header) => header,
            Err(err) => {
                eprintln!("conformance tx_execute get_block_template error: {err}");
                let code = map_error_code(&err);
                return HttpResponse::Ok().json(ExecResult {
                    success: false,
                    error_code: code,
                    state_digest: String::new(),
                });
            }
        }
    };

    let block = match engine
        .blockchain
        .build_block_from_header(tos_common::immutable::Immutable::Owned(header))
        .await
    {
        Ok(block) => block,
        Err(err) => {
            eprintln!("conformance tx_execute build_block_from_header error: {err}");
            let code = map_error_code(&err);
            return HttpResponse::Ok().json(ExecResult {
                success: false,
                error_code: code,
                state_digest: String::new(),
            });
        }
    };

    let block_hash = block.hash();
    let stable_topoheight = engine.blockchain.get_stable_topoheight().await;
    let current_topoheight = engine.blockchain.get_topo_height();
    let next_topoheight = current_topoheight.saturating_add(1);
    {
        let mut storage = engine.blockchain.get_storage().write().await;
        let mut chain_state = ApplicableChainState::new(
            &mut *storage,
            engine.blockchain.get_contract_environment(),
            stable_topoheight,
            next_topoheight,
            block.get_version(),
            0,
            &block_hash,
            &block,
            engine.blockchain.get_executor(),
        );

        let tx_arc = Arc::new(tx_for_apply);
        if let Err(err) = tx_arc
            .apply_with_partial_verify(&tx_hash, &mut chain_state)
            .await
        {
            let code = map_verify_error_code(&err);
            return HttpResponse::Ok().json(ExecResult {
                success: false,
                error_code: code,
                state_digest: String::new(),
            });
        }

        if let Err(err) = chain_state.apply_changes().await {
            let code = map_error_code(&err);
            return HttpResponse::Ok().json(ExecResult {
                success: false,
                error_code: code,
                state_digest: String::new(),
            });
        }
    }

    let export = build_export(&engine).await;
    let digest = compute_state_digest(&export);
    HttpResponse::Ok().json(ExecResult {
        success: true,
        error_code: 0,
        state_digest: digest,
    })
}

fn tx_from_json(value: &serde_json::Value) -> Result<Transaction, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "tx must be object".to_string())?;
    let tx_type = obj
        .get("tx_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "tx_type missing".to_string())?;
    if tx_type != "transfers" {
        return Err("unsupported tx_type".to_string());
    }
    let source = obj
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "source missing".to_string())?;
    let chain_id = obj
        .get("chain_id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "chain_id missing".to_string())?;
    let version = obj
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "version missing".to_string())?;
    let fee = obj
        .get("fee")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "fee missing".to_string())?;
    let fee_type = obj
        .get("fee_type")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "fee_type missing".to_string())?;
    let nonce = obj
        .get("nonce")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "nonce missing".to_string())?;
    let reference_hash = obj
        .get("reference_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "reference_hash missing".to_string())?;
    let reference_topoheight = obj
        .get("reference_topoheight")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "reference_topoheight missing".to_string())?;
    let signature = obj
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "signature missing".to_string())?;

    let mut payloads = Vec::new();
    let payload = obj
        .get("payload")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "payload missing".to_string())?;
    for item in payload {
        let asset = item
            .get("asset")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "payload.asset missing".to_string())?;
        let destination = item
            .get("destination")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "payload.destination missing".to_string())?;
        let amount = item
            .get("amount")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "payload.amount missing".to_string())?;
        let extra_data = item
            .get("extra_data")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let asset_hash = Hash::from_str(asset).map_err(|_| "invalid asset hex")?;
        let destination_key = to_public_key(destination)?;
        let extra = if extra_data.is_empty() {
            None
        } else {
            Some(UnknownExtraDataFormat(
                hex::decode(extra_data).map_err(|_| "invalid extra_data hex")?,
            ))
        };
        payloads.push(tos_common::transaction::TransferPayload::new(
            asset_hash,
            destination_key,
            amount,
            extra,
        ));
    }

    let version = TxVersion::try_from(version as u8).map_err(|_| "invalid version")?;
    let fee_type = match fee_type {
        0 => FeeType::TOS,
        1 => FeeType::Energy,
        2 => FeeType::UNO,
        _ => return Err("invalid fee_type".to_string()),
    };
    let source_key = to_public_key(source)?;
    let signature = Signature::from_hex(signature).map_err(|_| "invalid signature hex")?;
    let reference = Reference {
        hash: Hash::from_str(reference_hash).map_err(|_| "invalid reference_hash")?,
        topoheight: reference_topoheight,
    };

    Ok(Transaction::new(
        version,
        chain_id as u8,
        source_key,
        TransactionType::Transfers(payloads),
        fee,
        fee_type,
        nonce,
        reference,
        None,
        signature,
    ))
}

async fn handle_block_execute(
    state: web::Data<AppState>,
    body: web::Json<BlockExecuteRequest>,
) -> HttpResponse {
    let wire_hex = body.wire_hex.trim();
    if wire_hex.is_empty() {
        return HttpResponse::BadRequest()
            .json(json!({ "success": false, "error": "missing wire_hex", "error_code": 0xFF00 }));
    }

    let block = match Block::from_hex(wire_hex) {
        Ok(block) => block,
        Err(err) => {
            return HttpResponse::BadRequest().json(
                json!({ "success": false, "error": format!("{err:?}"), "error_code": 0x0100 }),
            );
        }
    };

    let engine = state.engine.lock().await;
    if let Err(err) = engine
        .blockchain
        .add_new_block(block, None, BroadcastOption::None, false)
        .await
    {
        let code = map_error_code(&err);
        return HttpResponse::Ok().json(ExecResult {
            success: false,
            error_code: code,
            state_digest: String::new(),
        });
    }

    let export = build_export(&engine).await;
    let digest = compute_state_digest(&export);
    HttpResponse::Ok().json(ExecResult {
        success: true,
        error_code: 0,
        state_digest: digest,
    })
}

async fn build_export(engine: &Engine) -> PreState {
    let mut accounts = Vec::new();
    let storage = engine.blockchain.get_storage().read().await;
    if !engine.meta.account_meta.is_empty() {
        for (addr, meta) in &engine.meta.account_meta {
            let key = match to_public_key(addr) {
                Ok(key) => key,
                Err(_) => continue,
            };
            let balance = storage
                .get_last_balance(&key, &TOS_ASSET)
                .await
                .map(|(_, v)| v.get_balance())
                .unwrap_or(0);
            let nonce = storage
                .get_last_nonce(&key)
                .await
                .map(|(_, v)| v.get_nonce())
                .unwrap_or(0);
            accounts.push(AccountState {
                address: addr.clone(),
                balance,
                nonce,
                frozen: meta.frozen,
                energy: meta.energy,
                flags: meta.flags,
                data: meta.data.clone(),
            });
        }
    } else if let Ok(iter) = storage.get_registered_keys(None, None).await {
        for entry in iter {
            let key = match entry {
                Ok(key) => key,
                Err(_) => continue,
            };
            let addr = public_key_to_hex(&key);
            let balance = storage
                .get_last_balance(&key, &TOS_ASSET)
                .await
                .map(|(_, v)| v.get_balance())
                .unwrap_or(0);
            let nonce = storage
                .get_last_nonce(&key)
                .await
                .map(|(_, v)| v.get_nonce())
                .unwrap_or(0);
            let meta = engine
                .meta
                .account_meta
                .get(&addr)
                .cloned()
                .unwrap_or_default();
            accounts.push(AccountState {
                address: addr,
                balance,
                nonce,
                frozen: meta.frozen,
                energy: meta.energy,
                flags: meta.flags,
                data: meta.data,
            });
        }
    }

    PreState {
        network_chain_id: engine.meta.network_chain_id,
        global_state: engine.meta.global_state.clone(),
        accounts,
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state_dir = std::env::var("LABU_STATE_DIR").unwrap_or_else(|_| "/state".to_string());
    let network = std::env::var("LABU_NETWORK").unwrap_or_else(|_| "dev".to_string());
    let network: Network = network.parse().unwrap_or(Network::Devnet);

    let base_dir = PathBuf::from(state_dir);
    if !base_dir.exists() {
        fs::create_dir_all(&base_dir)?;
    }

    let blockchain = create_blockchain(&base_dir, network, 0)
        .await
        .expect("init blockchain");

    let engine = Engine {
        base_dir,
        network,
        reset_nonce: 0,
        blockchain,
        meta: MetaState::default(),
    };

    let app_state = web::Data::new(AppState {
        engine: Arc::new(Mutex::new(engine)),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .route("/health", web::get().to(handle_health))
            .route("/state/reset", web::post().to(handle_state_reset))
            .route("/state/load", web::post().to(handle_state_load))
            .route("/state/export", web::get().to(handle_state_export))
            .route("/state/digest", web::get().to(handle_state_digest))
            .route("/tx/execute", web::post().to(handle_tx_execute))
            .route("/block/execute", web::post().to(handle_block_execute))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
