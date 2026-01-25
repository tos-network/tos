use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use tempdir::TempDir;
use tokio::time::{sleep, Duration};

use tos_common::a2a::{
    AgentCapabilities, AgentCard, AgentInterface, AgentSkill, TosAgentIdentity, TosSignature,
    TosSignerType, HEADER_VERSION, PROTOCOL_VERSION,
};
use tos_common::account::{AgentAccountMeta, SessionKey};
use tos_common::arbitration::{
    canonical_hash_without_signature, ArbiterAccount, ArbiterStatus, ArbitrationOpen,
    ExpertiseDomain, JurorVote, VoteChoice, VoteRequest, ARBITER_COOLDOWN_TOPOHEIGHT,
};
use tos_common::block::BlockVersion;
use tos_common::config::FEE_PER_KB;
use tos_common::config::{COIN_VALUE, MIN_ARBITER_STAKE, TOS_ASSET};
use tos_common::crypto::{hash, Address, AddressType, Hash, KeyPair, PublicKey, Signature};
use tos_common::escrow::{
    ArbitrationConfig, ArbitrationMode, DisputeInfo, EscrowAccount, EscrowState,
};
use tos_common::kyc::{
    CommitteeApproval, CommitteeMember, KycData, KycRegion, MemberRole, SecurityCommittee,
};
use tos_common::network::Network;
use tos_common::rpc::server::RPCServerHandler;
use tos_common::serializer::Serializer;
use tos_common::tns::{tns_name_hash, MAX_NAME_LENGTH, REGISTRATION_FEE};
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::{
    AppealKycPayload, CancelArbiterExitPayload, CreateEscrowPayload, FeeType, Reference,
    RefundEscrowPayload, RegisterArbiterPayload, RegisterNamePayload, ReleaseEscrowPayload,
    RenewKycPayload, RequestArbiterExitPayload, RevokeKycPayload, SetKycPayload, TransactionType,
    TransferKycPayload, TransferPayload, TxVersion,
};
use tos_daemon::a2a;
use tos_daemon::a2a::arbitration::persistence::load_coordinator_case;
use tos_daemon::core::blockchain::estimate_required_tx_fees;
use tos_daemon::core::blockchain::Blockchain;
use tos_daemon::core::blockchain::BroadcastOption;
use tos_daemon::core::config::{Config, RocksDBConfig};
use tos_daemon::core::storage::BlockDagProvider;
use tos_daemon::core::storage::RocksStorage;
use tos_daemon::core::storage::{
    test_message, test_message_id, AgentAccountProvider, ArbiterProvider, BalanceProvider,
    CommitteeProvider, EscrowProvider, KycProvider, NonceProvider, StateProvider, TnsProvider,
};
use tos_daemon::rpc::agent_registry::{RegisterAgentRequest, UpdateAgentRequest};
use tos_daemon::rpc::DaemonRpcServer;
use tos_daemon::vrf::WrappedMinerSecret;

struct TestRpcServer {
    base_url: String,
    server: std::sync::Arc<DaemonRpcServer<RocksStorage>>,
    miner_keypair: KeyPair,
    miner_pubkey: PublicKey,
    _temp_dir: TempDir,
}

const TEST_FUNDING_BALANCE: u64 = COIN_VALUE * 10;

fn pick_free_port() -> u16 {
    let mut rng = rand::thread_rng();
    rng.gen_range(10000..20000)
}

async fn ensure_account_ready(
    blockchain: &Blockchain<RocksStorage>,
    miner: &KeyPair,
    key: &PublicKey,
    min_balance: u64,
) {
    let miner_pubkey = miner.get_public_key().compress();
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let max_attempts = if min_balance <= COIN_VALUE * 20 {
        200usize
    } else {
        (min_balance / COIN_VALUE) as usize + 500
    };
    let mut attempts = 0usize;
    'funding: loop {
        attempts += 1;
        let topoheight = blockchain.get_topo_height();
        let balance = {
            let storage = blockchain.get_storage().read().await;
            storage
                .get_balance_at_maximum_topoheight(key, &TOS_ASSET, topoheight)
                .await
                .expect("get balance")
                .map(|(_, v)| v.get_balance())
                .unwrap_or(0)
        };
        if balance >= min_balance {
            break;
        }
        if key == &miner_pubkey {
            let block = blockchain
                .mine_block(&miner_pubkey)
                .await
                .expect("mine block");
            blockchain
                .add_new_block(block, None, BroadcastOption::None, true)
                .await
                .expect("add block");
            if attempts > max_attempts {
                panic!(
                    "unable to fund account {} to min balance {}",
                    key.as_address(false),
                    min_balance
                );
            }
            continue;
        }
        let amount_needed = min_balance.saturating_sub(balance);
        let topoheight = blockchain.get_topo_height();
        let (reference_hash, nonce, miner_balance) = {
            let storage = blockchain.get_storage().read().await;
            let (reference_hash, _) = storage
                .get_block_header_at_topoheight(topoheight)
                .await
                .expect("get reference header");
            let nonce = match storage
                .get_nonce_at_maximum_topoheight(&miner_pubkey, topoheight)
                .await
            {
                Ok(entry) => entry.map(|(_, v)| v.get_nonce()).unwrap_or(0),
                Err(_) => {
                    drop(storage);
                    let block = blockchain
                        .mine_block(&miner_pubkey)
                        .await
                        .expect("mine block");
                    blockchain
                        .add_new_block(block, None, BroadcastOption::None, true)
                        .await
                        .expect("add block");
                    if attempts > max_attempts {
                        panic!(
                            "unable to fund account {} to min balance {}",
                            key.as_address(false),
                            min_balance
                        );
                    }
                    continue 'funding;
                }
            };
            let miner_balance = match storage
                .get_balance_at_maximum_topoheight(&miner_pubkey, &TOS_ASSET, topoheight)
                .await
            {
                Ok(entry) => entry.map(|(_, v)| v.get_balance()).unwrap_or(0),
                Err(_) => {
                    drop(storage);
                    let block = blockchain
                        .mine_block(&miner_pubkey)
                        .await
                        .expect("mine block");
                    blockchain
                        .add_new_block(block, None, BroadcastOption::None, true)
                        .await
                        .expect("add block");
                    if attempts > max_attempts {
                        panic!(
                            "unable to fund account {} to min balance {}",
                            key.as_address(false),
                            min_balance
                        );
                    }
                    continue 'funding;
                }
            };
            (reference_hash, nonce, miner_balance)
        };
        let reference = Reference {
            topoheight,
            hash: reference_hash,
        };
        let payload = TransferPayload::new(TOS_ASSET, key.clone(), amount_needed, None);
        let draft = UnsignedTransaction::new_with_fee_type(
            TxVersion::T1,
            chain_id,
            miner_pubkey.clone(),
            TransactionType::Transfers(vec![payload.clone()]),
            0,
            FeeType::TOS,
            nonce,
            reference.clone(),
        );
        let draft_tx = draft.finalize(miner);
        let required_fee = {
            let storage = blockchain.get_storage().read().await;
            estimate_required_tx_fees(&*storage, topoheight, &draft_tx, BlockVersion::Nobunaga)
                .await
                .expect("estimate transfer fee")
        };
        if miner_balance < amount_needed.saturating_add(required_fee) {
            let block = blockchain
                .mine_block(&miner_pubkey)
                .await
                .expect("mine block");
            blockchain
                .add_new_block(block, None, BroadcastOption::None, true)
                .await
                .expect("add block");
            if attempts > max_attempts {
                panic!(
                    "unable to fund account {} to min balance {}",
                    key.as_address(false),
                    min_balance
                );
            }
            continue;
        }
        let amount = amount_needed;
        let send_payload = TransferPayload::new(TOS_ASSET, key.clone(), amount, None);
        let unsigned = UnsignedTransaction::new_with_fee_type(
            TxVersion::T1,
            chain_id,
            miner_pubkey.clone(),
            TransactionType::Transfers(vec![send_payload]),
            required_fee,
            FeeType::TOS,
            nonce,
            reference,
        );
        let tx = unsigned.finalize(miner);
        blockchain
            .add_tx_to_mempool(tx, true)
            .await
            .expect("add funding transfer");
        let block = blockchain
            .mine_block(&miner_pubkey)
            .await
            .expect("mine funding block");
        blockchain
            .add_new_block(block, None, BroadcastOption::None, true)
            .await
            .expect("add funding block");
        if attempts > max_attempts {
            panic!(
                "unable to fund account {} to min balance {}",
                key.as_address(false),
                min_balance
            );
        }
    }
}

async fn bump_account_nonce(
    blockchain: &Blockchain<RocksStorage>,
    miner_pubkey: &PublicKey,
    sender: &KeyPair,
) {
    let sender_pubkey = sender.get_public_key().compress();
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let topoheight = blockchain.get_topo_height();
    let (reference_hash, nonce, balance) = {
        let storage = blockchain.get_storage().read().await;
        let (reference_hash, _) = storage
            .get_block_header_at_topoheight(topoheight)
            .await
            .expect("get reference header");
        let nonce = storage
            .get_nonce_at_maximum_topoheight(&sender_pubkey, topoheight)
            .await
            .expect("get sender nonce")
            .map(|(_, v)| v.get_nonce())
            .unwrap_or(0);
        let balance = storage
            .get_balance_at_maximum_topoheight(&sender_pubkey, &TOS_ASSET, topoheight)
            .await
            .expect("get sender balance")
            .map(|(_, v)| v.get_balance())
            .unwrap_or(0);
        (reference_hash, nonce, balance)
    };
    let reference = Reference {
        topoheight,
        hash: reference_hash,
    };
    let amount = 1u64;
    let draft_payload = TransferPayload::new(TOS_ASSET, miner_pubkey.clone(), amount, None);
    let draft = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        sender_pubkey.clone(),
        TransactionType::Transfers(vec![draft_payload]),
        0,
        FeeType::TOS,
        nonce,
        reference.clone(),
    );
    let draft_tx = draft.finalize(sender);
    let required_fee = {
        let storage = blockchain.get_storage().read().await;
        estimate_required_tx_fees(&*storage, topoheight, &draft_tx, BlockVersion::Nobunaga)
            .await
            .expect("estimate transfer fee")
    };
    if balance < amount.saturating_add(required_fee) {
        panic!(
            "insufficient balance to bump nonce for {}",
            sender_pubkey.as_address(false)
        );
    }
    let payload = TransferPayload::new(TOS_ASSET, miner_pubkey.clone(), amount, None);
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        sender_pubkey,
        TransactionType::Transfers(vec![payload]),
        required_fee,
        FeeType::TOS,
        nonce,
        reference,
    );
    let tx = unsigned.finalize(sender);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add nonce bump transfer");
    let block = blockchain
        .mine_block(miner_pubkey)
        .await
        .expect("mine nonce bump block");
    blockchain
        .add_new_block(block, None, BroadcastOption::None, true)
        .await
        .expect("add nonce bump block");
}

async fn start_rpc_server() -> TestRpcServer {
    for _ in 0..50 {
        let temp_dir = TempDir::new("tck_fullstack_rpc").unwrap();
        let port = pick_free_port();
        let miner_keypair = KeyPair::new();
        let miner_pubkey = miner_keypair.get_public_key().compress();
        let miner_secret_hex = miner_keypair.get_private_key().to_hex();
        let mut config: Config = serde_json::from_value(json!({
            "rpc": { "getwork": {}, "prometheus": {} },
            "p2p": { "proxy": {} },
            "rocksdb": {},
            "vrf": {}
        }))
        .expect("build daemon config");
        config.rpc.bind_address = format!("127.0.0.1:{}", port);
        config.rpc.enable_a2a = true;
        config.rpc.a2a_api_keys = vec!["tck-test-key".to_string()];
        config.rpc.disable = true;
        config.rpc.threads = 1;
        config.rpc.prometheus.enable = false;
        config.rpc.getwork.disable = true;
        config.p2p.disable = true;
        config.skip_pow_verification = true;
        config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
        config.rocksdb = RocksDBConfig::default();
        config.vrf.miner_private_key =
            Some(WrappedMinerSecret::from_str(&miner_secret_hex).expect("miner key"));

        let storage = RocksStorage::new(
            &temp_dir.path().to_string_lossy(),
            Network::Devnet,
            &config.rocksdb,
        );
        let blockchain = match Blockchain::new(config.clone(), Network::Devnet, storage).await {
            Ok(chain) => chain,
            Err(_) => continue,
        };
        ensure_account_ready(&blockchain, &miner_keypair, &miner_pubkey, 0).await;
        let mut rpc_config = config.rpc.clone();
        rpc_config.disable = false;
        match DaemonRpcServer::new(blockchain, rpc_config).await {
            Ok(server) => {
                sleep(Duration::from_millis(100)).await;
                return TestRpcServer {
                    base_url: format!("http://127.0.0.1:{}", port),
                    server,
                    miner_keypair,
                    miner_pubkey,
                    _temp_dir: temp_dir,
                };
            }
            Err(err) => {
                if matches!(
                    err,
                    tos_daemon::core::error::BlockchainError::ErrorStd(ref io_err)
                        if io_err.kind() == std::io::ErrorKind::AddrInUse
                ) {
                    continue;
                }
                panic!("start rpc server: {err:?}");
            }
        }
    }

    panic!("start rpc server: address already in use after retries");
}

async fn mine_block_in_chain(blockchain: &Blockchain<RocksStorage>, miner: &PublicKey) {
    let block = blockchain.mine_block(miner).await.expect("mine block");
    blockchain
        .add_new_block(block, None, BroadcastOption::None, true)
        .await
        .expect("add block");
}

async fn fast_forward_topoheight(blockchain: &Blockchain<RocksStorage>, new_topoheight: u64) {
    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .set_top_topoheight(new_topoheight)
            .await
            .expect("set top topoheight");
        storage
            .set_top_height(new_topoheight)
            .await
            .expect("set top height");
    }
    blockchain.reload_from_disk().await.expect("reload chain");
}

fn extract_error_code(resp: &serde_json::Value) -> i64 {
    resp.get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_i64())
        .expect("error code")
}

fn basic_agent_card(name: &str) -> AgentCard {
    AgentCard {
        protocol_version: PROTOCOL_VERSION.to_string(),
        name: name.to_string(),
        description: "test agent".to_string(),
        version: "0.1.0".to_string(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/a2a".to_string(),
            protocol_binding: "https".to_string(),
            tenant: None,
        }],
        provider: None,
        icon_url: None,
        documentation_url: None,
        capabilities: AgentCapabilities {
            streaming: None,
            push_notifications: None,
            state_transition_history: None,
            extensions: Vec::new(),
            tos_on_chain_settlement: None,
        },
        security_schemes: Default::default(),
        security: Vec::new(),
        default_input_modes: Vec::new(),
        default_output_modes: Vec::new(),
        skills: Vec::<AgentSkill>::new(),
        supports_extended_agent_card: None,
        signatures: Vec::new(),
        tos_identity: None,
        arbitration: None,
    }
}

fn canonicalize_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (k, mut v) in entries {
                canonicalize_json_value(&mut v);
                map.insert(k, v);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                canonicalize_json_value(item);
            }
        }
        _ => {}
    }
}

fn canonical_card_hash(card: &AgentCard) -> Hash {
    let mut value = serde_json::to_value(card).expect("serialize agent card");
    canonicalize_json_value(&mut value);
    let card_bytes = serde_json::to_vec(&value).expect("serialize canonical card");
    hash(&card_bytes)
}

fn build_registration_message(
    chain_id: u64,
    agent_account: &PublicKey,
    endpoint_url: &str,
    agent_card: &AgentCard,
    signature: &TosSignature,
) -> Vec<u8> {
    let card_hash = canonical_card_hash(agent_card);
    let mut message = Vec::with_capacity(
        "TOS_AGENT_REGISTRY_V2".len()
            + 8
            + agent_account.as_bytes().len()
            + endpoint_url.len()
            + card_hash.as_bytes().len()
            + 16,
    );
    message.extend_from_slice(b"TOS_AGENT_REGISTRY_V2");
    message.extend_from_slice(&chain_id.to_le_bytes());
    message.extend_from_slice(agent_account.as_bytes());
    message.extend_from_slice(endpoint_url.as_bytes());
    message.extend_from_slice(card_hash.as_bytes());
    message.extend_from_slice(&signature.timestamp.to_le_bytes());
    message.extend_from_slice(&signature.nonce.to_le_bytes());
    message
}

fn build_tos_auth_headers(
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    signer: &KeyPair,
    timestamp: i64,
    nonce: &str,
) -> (String, String, String, String) {
    let body_hash = hex::encode(Sha256::digest(body));
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        path,
        query,
        timestamp,
        nonce,
        body_hash
    );
    let signature = signer.sign(canonical.as_bytes());
    let pubkey_hex = hex::encode(signer.get_public_key().compress().as_bytes());
    (
        pubkey_hex,
        signature.to_hex(),
        timestamp.to_string(),
        nonce.to_string(),
    )
}

#[tokio::test]
async fn test_fullstack_json_rpc_negative_cases() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "has_kyc",
        "params": { "address": "invalid" }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("send request")
        .json::<serde_json::Value>()
        .await
        .expect("json response");
    assert_eq!(extract_error_code(&resp), -32602);

    let long_name = "a".repeat(MAX_NAME_LENGTH + 1);
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "is_name_available",
        "params": { "name": long_name }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("send request")
        .json::<serde_json::Value>()
        .await
        .expect("json response");

    let result = resp.get("result").expect("result field");
    assert_eq!(
        result.get("valid_format").and_then(|v| v.as_bool()),
        Some(false)
    );

    let req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "get_escrow",
        "params": {}
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("send request")
        .json::<serde_json::Value>()
        .await
        .expect("json response");
    assert_eq!(extract_error_code(&resp), -32602);

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_a2a_register_requires_auth() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/agents:register", test_server.base_url))
        .body("{}")
        .send()
        .await
        .expect("send request");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_ws_json_rpc_get_height() {
    let test_server = start_rpc_server().await;
    let ws_url = format!("{}/json_rpc", test_server.base_url.replace("http", "ws"));

    let (mut ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("ws connect");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_height"
    });
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(
            req.to_string(),
        ))
        .await
        .expect("ws send");

    let text = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let msg = ws_stream
                .next()
                .await
                .expect("ws response")
                .expect("ws response ok");
            match msg {
                tokio_tungstenite::tungstenite::Message::Text(text) if !text.is_empty() => {
                    break text
                }
                tokio_tungstenite::tungstenite::Message::Binary(bytes) if !bytes.is_empty() => {
                    if let Ok(text) = String::from_utf8(bytes) {
                        break text;
                    }
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("ws response timeout");
    let resp: serde_json::Value = serde_json::from_str(&text).expect("json response");

    assert!(resp.get("result").is_some());

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_a2a_registry_roundtrip_with_auth() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();

    let register_req = RegisterAgentRequest {
        agent_card: basic_agent_card("agent-fullstack"),
        endpoint_url: "https://agent.example.com/a2a".to_string(),
        tos_signature: None,
    };

    let resp = client
        .post(format!("{}/agents:register", test_server.base_url))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .json(&register_req)
        .send()
        .await
        .expect("register request")
        .json::<serde_json::Value>()
        .await
        .expect("register response");

    let agent_id = resp
        .get("agentId")
        .and_then(|v| v.as_str())
        .expect("agentId in response")
        .to_string();

    let resp = client
        .get(format!("{}/agents/{}", test_server.base_url, agent_id))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .send()
        .await
        .expect("get agent")
        .json::<serde_json::Value>()
        .await
        .expect("get response");

    assert_eq!(
        resp.get("name").and_then(|v| v.as_str()),
        Some("agent-fullstack")
    );

    let update_req = UpdateAgentRequest {
        agent_id: agent_id.clone(),
        agent_card: basic_agent_card("agent-fullstack-updated"),
        tos_signature: None,
    };

    let resp = client
        .patch(format!("{}/agents/{}", test_server.base_url, agent_id))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .json(&update_req)
        .send()
        .await
        .expect("update agent")
        .json::<serde_json::Value>()
        .await
        .expect("update response");

    assert_eq!(
        resp.get("name").and_then(|v| v.as_str()),
        Some("agent-fullstack-updated")
    );

    let heartbeat_req = json!({
        "agentId": agent_id,
    });

    let resp = client
        .post(format!(
            "{}/agents/{}:heartbeat",
            test_server.base_url, update_req.agent_id
        ))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .json(&heartbeat_req)
        .send()
        .await
        .expect("heartbeat request");

    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .delete(format!(
            "{}/agents/{}",
            test_server.base_url, update_req.agent_id
        ))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .send()
        .await
        .expect("unregister agent");

    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    let resp = client
        .get(format!(
            "{}/agents/{}",
            test_server.base_url, update_req.agent_id
        ))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .send()
        .await
        .expect("get after unregister");

    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_a2a_tos_signature_auth_and_nonce_replay() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();

    let signer = KeyPair::new();
    let request_body = RegisterAgentRequest {
        agent_card: basic_agent_card("agent-tos-sig"),
        endpoint_url: "https://agent.example.com/a2a".to_string(),
        tos_signature: None,
    };
    let body = serde_json::to_vec(&request_body).expect("serialize register request");

    let timestamp = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()) as i64;
    let nonce = "nonce-1";
    let (pubkey_hex, sig_hex, ts_header, nonce_header) = build_tos_auth_headers(
        "POST",
        "/agents:register",
        "",
        &body,
        &signer,
        timestamp,
        nonce,
    );

    let resp = client
        .post(format!("{}/agents:register", test_server.base_url))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("tos-public-key", pubkey_hex)
        .header("tos-signature", sig_hex)
        .header("tos-timestamp", ts_header)
        .header("tos-nonce", nonce_header)
        .header("content-type", "application/json")
        .body(body.clone())
        .send()
        .await
        .expect("register with tos signature");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let (pubkey_hex, sig_hex, ts_header, nonce_header) = build_tos_auth_headers(
        "POST",
        "/agents:register",
        "",
        &body,
        &signer,
        timestamp,
        nonce,
    );
    let resp = client
        .post(format!("{}/agents:register", test_server.base_url))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("tos-public-key", pubkey_hex)
        .header("tos-signature", sig_hex)
        .header("tos-timestamp", ts_header)
        .header("tos-nonce", nonce_header)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .expect("replay register");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_a2a_registry_session_key_signature_replay() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();

    let agent_account = KeyPair::new();
    let controller = KeyPair::new();
    let session_keypair = KeyPair::new();
    let agent_pubkey = agent_account.get_public_key().compress();

    let meta = AgentAccountMeta {
        owner: agent_pubkey.clone(),
        controller: controller.get_public_key().compress(),
        policy_hash: Hash::zero(),
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    let session_key = SessionKey {
        key_id: 7,
        public_key: session_keypair.get_public_key().compress(),
        expiry_topoheight: blockchain.get_topo_height() + 100,
        max_value_per_window: 0,
        allowed_targets: Vec::new(),
        allowed_assets: Vec::new(),
    };

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .set_agent_account_meta(&agent_pubkey, &meta)
            .await
            .expect("set agent meta");
        storage
            .set_session_key(&agent_pubkey, &session_key)
            .await
            .expect("set session key");
    }

    let mut agent_card = basic_agent_card("agent-session-key");
    agent_card.tos_identity = Some(TosAgentIdentity {
        agent_account: agent_pubkey.clone(),
        controller: controller.get_public_key().compress(),
        reputation_score_bps: None,
        identity_proof: None,
    });

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let nonce = 1234_u64;
    let mut signature = TosSignature {
        signer: TosSignerType::SessionKey,
        value: String::new(),
        timestamp,
        nonce,
        session_key_id: Some(session_key.key_id),
    };

    let chain_id = blockchain.get_network().chain_id();
    let message = build_registration_message(
        chain_id,
        &agent_pubkey,
        "https://agent.example.com/a2a",
        &agent_card,
        &signature,
    );
    signature.value = format!("0x{}", session_keypair.sign(&message).to_hex());

    let request = RegisterAgentRequest {
        agent_card,
        endpoint_url: "https://agent.example.com/a2a".to_string(),
        tos_signature: Some(signature.clone()),
    };

    let resp = client
        .post(format!("{}/agents:register", test_server.base_url))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .json(&request)
        .send()
        .await
        .expect("register with session key");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .post(format!("{}/agents:register", test_server.base_url))
        .header(HEADER_VERSION, PROTOCOL_VERSION)
        .header("x-api-key", "tck-test-key")
        .json(&request)
        .send()
        .await
        .expect("replay session key register");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_kyc_tns_positive_cases() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();

    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let user_keypair = KeyPair::new();
    let user_pubkey = user_keypair.get_public_key().compress();
    let user_address = Address::new(false, AddressType::Normal, user_pubkey.clone());
    let committee_id = Hash::new([7u8; 32]);
    let tx_hash = Hash::new([8u8; 32]);
    let name_hash = tns_name_hash("alice");
    let sender_hash = tns_name_hash("bob");
    let message_id = test_message_id(&sender_hash, &name_hash, 42);
    let current_topoheight = blockchain.get_topo_height();

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .register_name(name_hash.clone(), user_pubkey.clone())
            .await
            .expect("register tns name");
        storage
            .store_ephemeral_message(
                message_id.clone(),
                test_message(
                    sender_hash,
                    name_hash.clone(),
                    42,
                    1_000,
                    current_topoheight,
                ),
            )
            .await
            .expect("store ephemeral message");
        storage
            .set_kyc(
                &user_pubkey,
                KycData::new(31, 1_000, Hash::new([9u8; 32])),
                &committee_id,
                1,
                &tx_hash,
            )
            .await
            .expect("set kyc");
    }

    let req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "get_kyc",
        "params": { "address": user_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_kyc request")
        .json::<serde_json::Value>()
        .await
        .expect("get_kyc response");

    let kyc = resp
        .get("result")
        .and_then(|v| v.get("kyc"))
        .expect("kyc in response");
    assert_eq!(kyc.get("level").and_then(|v| v.as_u64()), Some(31));

    let req = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "get_account_name_hash",
        "params": { "address": user_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_account_name_hash request")
        .json::<serde_json::Value>()
        .await
        .expect("get_account_name_hash response");

    let name_hash_resp = resp
        .get("result")
        .and_then(|v| v.get("name_hash"))
        .and_then(|v| v.as_str())
        .expect("name_hash in response");
    assert_eq!(name_hash_resp, name_hash.to_hex());

    let req = json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "get_message_by_id",
        "params": { "message_id": message_id }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_message_by_id request")
        .json::<serde_json::Value>()
        .await
        .expect("get_message_by_id response");

    let message = resp
        .get("result")
        .and_then(|v| v.get("message"))
        .expect("message in response");
    let message_id_hex = message_id.to_hex();
    assert_eq!(
        message.get("message_id").and_then(|v| v.as_str()),
        Some(message_id_hex.as_str())
    );

    test_server.server.stop().await;
}

#[tokio::test]
async fn test_fullstack_arbitration_rpc_success() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();

    let arbiter_keypair = KeyPair::new();
    let arbiter_pubkey = arbiter_keypair.get_public_key().compress();
    let arbiter_address = Address::new(false, AddressType::Normal, arbiter_pubkey.clone());

    let arbiter = ArbiterAccount {
        public_key: arbiter_pubkey.clone(),
        name: "arbiter-one".to_string(),
        status: ArbiterStatus::Active,
        expertise: vec![ExpertiseDomain::General],
        stake_amount: 5_000,
        fee_basis_points: 50,
        min_escrow_value: 100,
        max_escrow_value: 100_000,
        reputation_score: 9000,
        total_cases: 0,
        cases_overturned: 0,
        registered_at: 1,
        last_active_at: 1,
        pending_withdrawal: 0,
        deactivated_at: None,
        active_cases: 0,
        total_slashed: 0,
        slash_count: 0,
    };

    {
        let mut storage = blockchain.get_storage().write().await;
        storage.set_arbiter(&arbiter).await.expect("set arbiter");
    }

    let req = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "get_arbiter_withdraw_status",
        "params": { "address": arbiter_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_arbiter_withdraw_status request")
        .json::<serde_json::Value>()
        .await
        .expect("get_arbiter_withdraw_status response");

    let result = resp.get("result").expect("result field");
    assert_eq!(
        result.get("stake_amount").and_then(|v| v.as_u64()),
        Some(5_000)
    );
    assert_eq!(result.get("active_cases").and_then(|v| v.as_u64()), Some(0));

    let req = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "estimate_withdrawable_amount",
        "params": { "address": arbiter_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("estimate_withdrawable_amount request")
        .json::<serde_json::Value>()
        .await
        .expect("estimate_withdrawable_amount response");

    let available = resp
        .get("result")
        .and_then(|v| v.get("available"))
        .and_then(|v| v.as_u64())
        .expect("available in response");
    assert_eq!(available, 0);

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_arbitration_open_and_vote() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-arbitration-open".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_arbitration_open_and_vote());
        })
        .expect("spawn fullstack arbitration open thread");
    handle
        .join()
        .expect("join fullstack arbitration open thread");
}

async fn run_fullstack_arbitration_open_and_vote() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();
    a2a::set_base_dir(&test_server._temp_dir.path().to_string_lossy());

    let coordinator_keypair = KeyPair::new();
    let coordinator_pubkey = coordinator_keypair.get_public_key().compress();
    std::env::set_var(
        "TOS_ARBITRATION_COORDINATOR_PRIVATE_KEY",
        coordinator_keypair.get_private_key().to_hex(),
    );

    let juror_keypair = KeyPair::new();
    let juror_pubkey = juror_keypair.get_public_key().compress();
    let juror_address = Address::new(false, AddressType::Normal, juror_pubkey.clone()).to_string();

    let committee_id = SecurityCommittee::compute_id(KycRegion::Global, "arb-committee", 1);
    let committee = SecurityCommittee::new(
        committee_id.clone(),
        KycRegion::Global,
        "arb-committee".to_string(),
        vec![CommitteeMember::new(
            juror_pubkey.clone(),
            Some("juror-1".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );

    let coordinator_arbiter = ArbiterAccount {
        public_key: coordinator_pubkey.clone(),
        name: "arbiter-coordinator".to_string(),
        status: ArbiterStatus::Active,
        expertise: vec![ExpertiseDomain::General],
        stake_amount: MIN_ARBITER_STAKE,
        fee_basis_points: 50,
        min_escrow_value: 1,
        max_escrow_value: 1_000_000,
        reputation_score: 9000,
        total_cases: 0,
        cases_overturned: 0,
        registered_at: 1,
        last_active_at: 1,
        pending_withdrawal: 0,
        deactivated_at: None,
        active_cases: 0,
        total_slashed: 0,
        slash_count: 0,
    };
    let juror_arbiter = ArbiterAccount {
        public_key: juror_pubkey.clone(),
        name: "arbiter-juror".to_string(),
        status: ArbiterStatus::Active,
        expertise: vec![ExpertiseDomain::General],
        stake_amount: MIN_ARBITER_STAKE,
        fee_basis_points: 50,
        min_escrow_value: 1,
        max_escrow_value: 1_000_000,
        reputation_score: 9000,
        total_cases: 0,
        cases_overturned: 0,
        registered_at: 1,
        last_active_at: 1,
        pending_withdrawal: 0,
        deactivated_at: None,
        active_cases: 0,
        total_slashed: 0,
        slash_count: 0,
    };

    let escrow_id = Hash::new([1u8; 32]);
    let dispute_id = Hash::new([3u8; 32]);
    let escrow = EscrowAccount {
        id: escrow_id.clone(),
        task_id: "task-1".to_string(),
        payer: juror_pubkey.clone(),
        payee: juror_pubkey.clone(),
        amount: 10_000,
        total_amount: 10_000,
        released_amount: 0,
        refunded_amount: 0,
        pending_release_amount: None,
        challenge_deposit: 0,
        asset: TOS_ASSET,
        state: EscrowState::Challenged,
        dispute_id: Some(dispute_id.clone()),
        dispute_round: None,
        challenge_window: 10,
        challenge_deposit_bps: 0,
        optimistic_release: false,
        release_requested_at: None,
        created_at: 1,
        updated_at: 1,
        timeout_at: 100,
        timeout_blocks: 100,
        arbitration_config: Some(ArbitrationConfig {
            mode: ArbitrationMode::Single,
            arbiters: vec![coordinator_pubkey.clone()],
            threshold: Some(1),
            fee_amount: 0,
            allow_appeal: false,
        }),
        dispute: Some(DisputeInfo {
            initiator: coordinator_pubkey.clone(),
            reason: "dispute".to_string(),
            evidence_hash: None,
            disputed_at: 1,
            deadline: 100,
        }),
        appeal: None,
        resolutions: Vec::new(),
    };

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .import_committee(&committee_id, &committee)
            .await
            .expect("import committee");
        storage
            .set_arbiter(&coordinator_arbiter)
            .await
            .expect("set coordinator arbiter");
        storage
            .set_arbiter(&juror_arbiter)
            .await
            .expect("set juror arbiter");
        storage.set_escrow(&escrow).await.expect("set escrow");
    }
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &coordinator_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    bump_account_nonce(&blockchain, &miner_pubkey, &coordinator_keypair).await;

    let opener = KeyPair::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut open = ArbitrationOpen {
        message_type: "ArbitrationOpen".to_string(),
        version: 1,
        chain_id: blockchain.get_network().chain_id(),
        escrow_id: escrow_id.clone(),
        escrow_hash: Hash::new([2u8; 32]),
        dispute_id: dispute_id.clone(),
        round: 0,
        dispute_open_height: blockchain.get_topo_height(),
        committee_id: committee_id.clone(),
        committee_policy_hash: Hash::zero(),
        payer: juror_address.clone(),
        payee: juror_address.clone(),
        evidence_uri: "https://example.com/evidence".to_string(),
        evidence_hash: Hash::new([4u8; 32]),
        evidence_manifest_uri: "https://example.com/manifest".to_string(),
        evidence_manifest_hash: Hash::new([5u8; 32]),
        client_nonce: "nonce-1".to_string(),
        issued_at: now,
        expires_at: now + 60,
        coordinator_pubkey: coordinator_pubkey.clone(),
        coordinator_account: Address::new(false, AddressType::Normal, coordinator_pubkey.clone())
            .to_string(),
        request_id: Hash::new([6u8; 32]),
        opener_pubkey: opener.get_public_key().compress(),
        signature: Signature::new(
            tos_crypto::curve25519_dalek::Scalar::ZERO,
            tos_crypto::curve25519_dalek::Scalar::ZERO,
        ),
    };
    let open_hash = canonical_hash_without_signature(&open, "signature").expect("hash open");
    open.signature = opener.sign(open_hash.as_bytes());

    let req = json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "arbitration_open",
        "params": open
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("arbitration_open request")
        .json::<serde_json::Value>()
        .await
        .expect("arbitration_open response");

    let vote_request_value = resp
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("arbitration_open missing result, response={:?}", resp));
    let vote_request: VoteRequest =
        serde_json::from_value(vote_request_value).expect("vote request");
    let vote_request_hash =
        canonical_hash_without_signature(&vote_request, "signature").expect("vote request hash");

    let mut vote = JurorVote {
        message_type: "JurorVote".to_string(),
        version: 1,
        request_id: vote_request.request_id.clone(),
        chain_id: vote_request.chain_id,
        escrow_id: vote_request.escrow_id.clone(),
        escrow_hash: vote_request.escrow_hash.clone(),
        dispute_id: vote_request.dispute_id.clone(),
        round: vote_request.round,
        dispute_open_height: vote_request.dispute_open_height,
        committee_id: vote_request.committee_id.clone(),
        selection_block: vote_request.selection_block,
        selection_commitment_id: vote_request.selection_commitment_id.clone(),
        arbitration_open_hash: vote_request.arbitration_open_hash.clone(),
        vote_request_hash: vote_request_hash.clone(),
        evidence_hash: vote_request.evidence_hash.clone(),
        evidence_manifest_hash: vote_request.evidence_manifest_hash.clone(),
        selected_jurors_hash: vote_request.selected_jurors_hash.clone(),
        committee_policy_hash: vote_request.committee_policy_hash.clone(),
        juror_pubkey: juror_pubkey.clone(),
        juror_account: vote_request.selected_jurors[0].clone(),
        vote: VoteChoice::Pay,
        voted_at: now + 1,
        signature: Signature::new(
            tos_crypto::curve25519_dalek::Scalar::ZERO,
            tos_crypto::curve25519_dalek::Scalar::ZERO,
        ),
    };
    let vote_hash = canonical_hash_without_signature(&vote, "signature").expect("vote hash");
    vote.signature = juror_keypair.sign(vote_hash.as_bytes());

    let req = json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "submit_juror_vote",
        "params": vote
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("submit_juror_vote request")
        .json::<serde_json::Value>()
        .await
        .expect("submit_juror_vote response");

    if resp.get("result").is_none() {
        panic!("submit_juror_vote missing result, response={:?}", resp);
    }
    let mempool_size = blockchain.get_mempool_size().await;
    assert!(mempool_size >= 1);

    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "get_escrow",
        "params": { "escrow_id": escrow_id.to_hex() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrow request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrow response");

    let escrow = resp.get("result").expect("result");
    assert_eq!(
        escrow.get("state").and_then(|v| v.as_str()),
        Some("resolved")
    );
    assert_eq!(escrow.get("amount").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(
        escrow.get("releasedAmount").and_then(|v| v.as_u64()),
        Some(10_000)
    );

    let case = load_coordinator_case(&open.request_id)
        .expect("load coordinator case")
        .expect("coordinator case");
    assert!(case.verdict.is_some());
    assert!(case.verdict_submitted);

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_arbiter_exit_and_withdraw_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-arbiter-exit-withdraw".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_arbiter_exit_and_withdraw_on_chain());
        })
        .expect("spawn fullstack arbiter exit withdraw thread");
    handle
        .join()
        .expect("join fullstack arbiter exit withdraw thread");
}

async fn run_fullstack_arbiter_exit_and_withdraw_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();

    let arbiter = KeyPair::new();
    let arbiter_pubkey = arbiter.get_public_key().compress();
    let arbiter_address = Address::new(false, AddressType::Normal, arbiter_pubkey.clone());
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &arbiter_pubkey,
        MIN_ARBITER_STAKE + COIN_VALUE * 10,
    )
    .await;
    let registered_at = blockchain.get_topo_height();
    let mut arbiter_state = ArbiterAccount {
        public_key: arbiter_pubkey.clone(),
        name: "arbiter-exit".to_string(),
        status: ArbiterStatus::Active,
        expertise: vec![ExpertiseDomain::General],
        stake_amount: MIN_ARBITER_STAKE,
        fee_basis_points: 10,
        min_escrow_value: 1,
        max_escrow_value: 1_000_000,
        reputation_score: 0,
        total_cases: 0,
        cases_overturned: 0,
        registered_at,
        last_active_at: registered_at,
        pending_withdrawal: 0,
        deactivated_at: None,
        active_cases: 0,
        total_slashed: 0,
        slash_count: 0,
    };
    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .set_arbiter(&arbiter_state)
            .await
            .expect("set arbiter");
    }

    arbiter_state.status = ArbiterStatus::Exiting;
    arbiter_state.deactivated_at = Some(blockchain.get_topo_height());
    arbiter_state.pending_withdrawal = arbiter_state.stake_amount;
    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .set_arbiter(&arbiter_state)
            .await
            .expect("set arbiter exiting");
    }

    let req = json!({
        "jsonrpc": "2.0",
        "id": 33,
        "method": "get_arbiter_withdraw_status",
        "params": { "address": arbiter_address.to_string() }
    });
    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_arbiter_withdraw_status request")
        .json::<serde_json::Value>()
        .await
        .expect("get_arbiter_withdraw_status response");
    let status = resp
        .get("result")
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .expect("status");
    assert_eq!(status, "exiting");

    let new_topoheight = blockchain
        .get_topo_height()
        .saturating_add(ARBITER_COOLDOWN_TOPOHEIGHT + 1);
    fast_forward_topoheight(&blockchain, new_topoheight).await;

    arbiter_state.status = ArbiterStatus::Removed;
    arbiter_state.stake_amount = 0;
    arbiter_state.pending_withdrawal = 0;
    arbiter_state.deactivated_at = None;
    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .set_arbiter(&arbiter_state)
            .await
            .expect("set arbiter removed");
    }

    let req = json!({
        "jsonrpc": "2.0",
        "id": 34,
        "method": "get_arbiter_withdraw_status",
        "params": { "address": arbiter_address.to_string() }
    });
    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_arbiter_withdraw_status request")
        .json::<serde_json::Value>()
        .await
        .expect("get_arbiter_withdraw_status response");
    let status = resp
        .get("result")
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .expect("status");
    assert_eq!(status, "removed");

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_arbiter_exit_cancel_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-arbiter-exit-cancel".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_arbiter_exit_cancel_on_chain());
        })
        .expect("spawn fullstack arbiter exit cancel thread");
    handle
        .join()
        .expect("join fullstack arbiter exit cancel thread");
}

async fn run_fullstack_arbiter_exit_cancel_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let arbiter = KeyPair::new();
    let arbiter_pubkey = arbiter.get_public_key().compress();
    let arbiter_address = Address::new(false, AddressType::Normal, arbiter_pubkey.clone());

    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &arbiter_pubkey,
        MIN_ARBITER_STAKE + COIN_VALUE * 10,
    )
    .await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");

    let payload = RegisterArbiterPayload::new(
        "arbiter-cancel".to_string(),
        vec![ExpertiseDomain::General],
        MIN_ARBITER_STAKE,
        10,
        1_000_000,
        200,
    );
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        arbiter_pubkey.clone(),
        TransactionType::RegisterArbiter(payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference.clone(),
    );
    let tx = unsigned.finalize(&arbiter);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add register arbiter tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let payload = RequestArbiterExitPayload::new();
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        arbiter_pubkey.clone(),
        TransactionType::RequestArbiterExit(payload),
        FEE_PER_KB,
        FeeType::TOS,
        1,
        reference.clone(),
    );
    let tx = unsigned.finalize(&arbiter);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add request exit tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let payload = CancelArbiterExitPayload::new();
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        arbiter_pubkey.clone(),
        TransactionType::CancelArbiterExit(payload),
        FEE_PER_KB,
        FeeType::TOS,
        2,
        reference,
    );
    let tx = unsigned.finalize(&arbiter);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add cancel exit tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 35,
        "method": "get_arbiter_withdraw_status",
        "params": { "address": arbiter_address.to_string() }
    });
    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_arbiter_withdraw_status request")
        .json::<serde_json::Value>()
        .await
        .expect("get_arbiter_withdraw_status response");
    let status = resp
        .get("result")
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .expect("status");
    assert_eq!(status, "active");

    test_server.server.stop().await;
}

async fn run_fullstack_tns_register_name_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let sender = KeyPair::new();
    let sender_pubkey = sender.get_public_key().compress();
    let sender_address = Address::new(false, AddressType::Normal, sender_pubkey.clone());

    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &sender_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");

    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let payload = RegisterNamePayload::new("alice".to_string());
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        sender_pubkey.clone(),
        TransactionType::RegisterName(payload),
        REGISTRATION_FEE,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&sender);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add register name tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "get_account_name_hash",
        "params": { "address": sender_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_account_name_hash request")
        .json::<serde_json::Value>()
        .await
        .expect("get_account_name_hash response");

    let name_hash_resp = resp
        .get("result")
        .and_then(|v| v.get("name_hash"))
        .and_then(|v| v.as_str())
        .expect("name_hash in response");
    assert_eq!(name_hash_resp, tns_name_hash("alice").to_hex());

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_tns_register_name_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-tns-register".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_tns_register_name_on_chain());
        })
        .expect("spawn fullstack tns register thread");
    handle.join().expect("join fullstack tns register thread");
}

async fn run_fullstack_tns_name_rejects_transfer_attempt() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let owner1 = KeyPair::new();
    let owner2 = KeyPair::new();
    let owner1_pubkey = owner1.get_public_key().compress();
    let owner2_pubkey = owner2.get_public_key().compress();
    let owner1_address = Address::new(false, AddressType::Normal, owner1_pubkey.clone());

    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &owner1_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &owner2_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");

    let payload = RegisterNamePayload::new("alice".to_string());
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        owner1_pubkey.clone(),
        TransactionType::RegisterName(payload),
        REGISTRATION_FEE,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&owner1);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add register name tx");

    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };

    let payload = RegisterNamePayload::new("alice".to_string());
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        owner2_pubkey.clone(),
        TransactionType::RegisterName(payload),
        REGISTRATION_FEE,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&owner2);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect_err("duplicate name should be rejected");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "resolve_name",
        "params": { "name": "alice" }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("resolve_name request")
        .json::<serde_json::Value>()
        .await
        .expect("resolve_name response");

    let resolved = resp
        .get("result")
        .and_then(|v| v.get("address"))
        .and_then(|v| v.as_str())
        .expect("resolved address");
    assert_eq!(resolved, owner1_address.to_string());

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_tns_name_rejects_transfer_attempt() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-tns-rejects-transfer".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_tns_name_rejects_transfer_attempt());
        })
        .expect("spawn fullstack tns rejects transfer thread");
    handle
        .join()
        .expect("join fullstack tns rejects transfer thread");
}

async fn run_fullstack_escrow_create_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let payer = KeyPair::new();
    let provider = KeyPair::new();
    let payer_pubkey = payer.get_public_key().compress();
    let provider_pubkey = provider.get_public_key().compress();
    let payer_address = Address::new(false, AddressType::Normal, payer_pubkey.clone());
    let provider_address = Address::new(false, AddressType::Normal, provider_pubkey.clone());

    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &payer_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");

    let payload = CreateEscrowPayload {
        task_id: "task-escrow-1".to_string(),
        provider: provider_pubkey.clone(),
        amount: 10_000,
        asset: TOS_ASSET,
        timeout_blocks: 100,
        challenge_window: 10,
        challenge_deposit_bps: 0,
        optimistic_release: false,
        arbitration_config: None,
        metadata: None,
    };

    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        payer_pubkey.clone(),
        TransactionType::CreateEscrow(payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&payer);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add create escrow tx");
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &payer_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 50,
        "method": "get_escrows_by_client",
        "params": { "address": payer_address.to_string(), "skip": 0, "maximum": 10 }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrows_by_client request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrows_by_client response");

    let escrows = resp
        .get("result")
        .and_then(|v| v.get("escrows"))
        .and_then(|v| v.as_array())
        .expect("escrows array");
    assert!(escrows
        .iter()
        .any(|e| e.get("taskId") == Some(&json!("task-escrow-1"))));

    let req = json!({
        "jsonrpc": "2.0",
        "id": 51,
        "method": "get_escrows_by_provider",
        "params": { "address": provider_address.to_string(), "skip": 0, "maximum": 10 }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrows_by_provider request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrows_by_provider response");

    let escrows = resp
        .get("result")
        .and_then(|v| v.get("escrows"))
        .and_then(|v| v.as_array())
        .expect("escrows array");
    assert!(escrows
        .iter()
        .any(|e| e.get("taskId") == Some(&json!("task-escrow-1"))));

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_escrow_create_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-escrow-create".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_escrow_create_on_chain());
        })
        .expect("spawn fullstack escrow create thread");
    handle.join().expect("join fullstack escrow create thread");
}

async fn run_fullstack_escrow_release_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let payer = KeyPair::new();
    let provider = KeyPair::new();
    let payer_pubkey = payer.get_public_key().compress();
    let provider_pubkey = provider.get_public_key().compress();
    let payer_address = Address::new(false, AddressType::Normal, payer_pubkey.clone());

    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &payer_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &provider_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");

    let payload = CreateEscrowPayload {
        task_id: "task-escrow-release".to_string(),
        provider: provider_pubkey.clone(),
        amount: 10_000,
        asset: TOS_ASSET,
        timeout_blocks: 100,
        challenge_window: 10,
        challenge_deposit_bps: 0,
        optimistic_release: true,
        arbitration_config: Some(ArbitrationConfig {
            mode: ArbitrationMode::Single,
            arbiters: vec![provider_pubkey.clone()],
            threshold: Some(1),
            fee_amount: 0,
            allow_appeal: false,
        }),
        metadata: None,
    };

    eprintln!("[escrow_release] add create escrow tx");
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        payer_pubkey.clone(),
        TransactionType::CreateEscrow(payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&payer);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add create escrow tx");
    eprintln!("[escrow_release] mine block after create");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    eprintln!("[escrow_release] query escrow list");
    let req = json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "get_escrows_by_client",
        "params": { "address": payer_address.to_string(), "skip": 0, "maximum": 10 }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrows_by_client request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrows_by_client response");

    let escrows = resp
        .get("result")
        .and_then(|v| v.get("escrows"))
        .and_then(|v| v.as_array())
        .expect("escrows array");
    let escrow = escrows
        .iter()
        .find(|e| e.get("taskId") == Some(&json!("task-escrow-release")))
        .expect("escrow entry");
    let escrow_id: Hash = escrow
        .get("id")
        .and_then(|v| v.as_str())
        .expect("escrow id")
        .parse()
        .expect("parse escrow id");

    eprintln!("[escrow_release] add release tx");
    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };

    let release_payload = ReleaseEscrowPayload {
        escrow_id: escrow_id.clone(),
        amount: 4_000,
        completion_proof: None,
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        provider_pubkey.clone(),
        TransactionType::ReleaseEscrow(release_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&provider);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add release escrow tx");
    eprintln!("[escrow_release] mine block after release");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    eprintln!("[escrow_release] get escrow");
    let req = json!({
        "jsonrpc": "2.0",
        "id": 61,
        "method": "get_escrow",
        "params": { "escrow_id": escrow_id.to_hex() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrow request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrow response");

    let escrow = resp.get("result").expect("result");
    assert_eq!(
        escrow.get("state").and_then(|v| v.as_str()),
        Some("pending-release")
    );
    assert_eq!(
        escrow.get("pendingReleaseAmount").and_then(|v| v.as_u64()),
        Some(4_000)
    );

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_escrow_release_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-escrow-release".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_escrow_release_on_chain());
        })
        .expect("spawn fullstack escrow release thread");
    handle.join().expect("join fullstack escrow release thread");
}

async fn run_fullstack_escrow_refund_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let payer = KeyPair::new();
    let provider = KeyPair::new();
    let payer_pubkey = payer.get_public_key().compress();
    let provider_pubkey = provider.get_public_key().compress();
    let payer_address = Address::new(false, AddressType::Normal, payer_pubkey.clone());

    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &payer_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &provider_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");

    let payload = CreateEscrowPayload {
        task_id: "task-escrow-refund".to_string(),
        provider: provider_pubkey.clone(),
        amount: 10_000,
        asset: TOS_ASSET,
        timeout_blocks: 100,
        challenge_window: 10,
        challenge_deposit_bps: 0,
        optimistic_release: false,
        arbitration_config: None,
        metadata: None,
    };

    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        payer_pubkey.clone(),
        TransactionType::CreateEscrow(payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&payer);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add create escrow tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 70,
        "method": "get_escrows_by_client",
        "params": { "address": payer_address.to_string(), "skip": 0, "maximum": 10 }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrows_by_client request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrows_by_client response");

    let escrows = resp
        .get("result")
        .and_then(|v| v.get("escrows"))
        .and_then(|v| v.as_array())
        .expect("escrows array");
    let escrow = escrows
        .iter()
        .find(|e| e.get("taskId") == Some(&json!("task-escrow-refund")))
        .expect("escrow entry");
    let escrow_id: Hash = escrow
        .get("id")
        .and_then(|v| v.as_str())
        .expect("escrow id")
        .parse()
        .expect("parse escrow id");

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };

    let refund_payload = RefundEscrowPayload {
        escrow_id: escrow_id.clone(),
        amount: 10_000,
        reason: Some("client_cancel".to_string()),
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        provider_pubkey.clone(),
        TransactionType::RefundEscrow(refund_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&provider);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add refund escrow tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 71,
        "method": "get_escrow",
        "params": { "escrow_id": escrow_id.to_hex() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrow request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrow response");

    let escrow = resp.get("result").expect("result");
    assert_eq!(
        escrow.get("state").and_then(|v| v.as_str()),
        Some("refunded")
    );
    assert_eq!(escrow.get("amount").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(
        escrow.get("refundedAmount").and_then(|v| v.as_u64()),
        Some(10_000)
    );

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_escrow_refund_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-escrow-refund".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_escrow_refund_on_chain());
        })
        .expect("spawn fullstack escrow refund thread");
    handle.join().expect("join fullstack escrow refund thread");
}

#[test]
fn test_fullstack_kyc_set_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-kyc-set".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_kyc_set_on_chain());
        })
        .expect("spawn fullstack kyc set thread");
    handle.join().expect("join fullstack kyc set thread");
}

async fn run_fullstack_kyc_set_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let committee_member = KeyPair::new();
    let member_pubkey = committee_member.get_public_key().compress();
    let user = KeyPair::new();
    let user_pubkey = user.get_public_key().compress();
    let user_address = Address::new(false, AddressType::Normal, user_pubkey.clone());

    let committee_id = SecurityCommittee::compute_id(KycRegion::Global, "kyc-committee", 1);
    let committee = SecurityCommittee::new(
        committee_id.clone(),
        KycRegion::Global,
        "kyc-committee".to_string(),
        vec![CommitteeMember::new(
            member_pubkey.clone(),
            Some("member-1".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .import_committee(&committee_id, &committee)
            .await
            .expect("import committee");
    }
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &member_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let verified_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let approval_ts = verified_at;
    let data_hash = Hash::new([9u8; 32]);
    let message = CommitteeApproval::build_set_kyc_message(
        blockchain.get_network(),
        &committee_id,
        &user_pubkey,
        31,
        &data_hash,
        verified_at,
        approval_ts,
    );
    let approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&message),
        approval_ts,
    );

    let payload = SetKycPayload::new(
        user_pubkey.clone(),
        31,
        verified_at,
        data_hash,
        committee_id.clone(),
        vec![approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::SetKyc(payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add set kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "get_kyc",
        "params": { "address": user_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_kyc request")
        .json::<serde_json::Value>()
        .await
        .expect("get_kyc response");

    let kyc = resp
        .get("result")
        .and_then(|v| v.get("kyc"))
        .expect("kyc in response");
    assert_eq!(kyc.get("level").and_then(|v| v.as_u64()), Some(31));

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_kyc_revoke_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-kyc-revoke".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_kyc_revoke_on_chain());
        })
        .expect("spawn fullstack kyc revoke thread");
    handle.join().expect("join fullstack kyc revoke thread");
}

async fn run_fullstack_kyc_revoke_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let committee_member = KeyPair::new();
    let member_pubkey = committee_member.get_public_key().compress();
    let user = KeyPair::new();
    let user_pubkey = user.get_public_key().compress();
    let user_address = Address::new(false, AddressType::Normal, user_pubkey.clone());

    let committee_id = SecurityCommittee::compute_id(KycRegion::Global, "kyc-revoke", 1);
    let committee = SecurityCommittee::new(
        committee_id.clone(),
        KycRegion::Global,
        "kyc-revoke".to_string(),
        vec![CommitteeMember::new(
            member_pubkey.clone(),
            Some("member-1".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .import_committee(&committee_id, &committee)
            .await
            .expect("import committee");
    }
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &member_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let verified_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let approval_ts = verified_at;
    let data_hash = Hash::new([1u8; 32]);
    let set_message = CommitteeApproval::build_set_kyc_message(
        blockchain.get_network(),
        &committee_id,
        &user_pubkey,
        31,
        &data_hash,
        verified_at,
        approval_ts,
    );
    let set_approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&set_message),
        approval_ts,
    );

    let set_payload = SetKycPayload::new(
        user_pubkey.clone(),
        31,
        verified_at,
        data_hash,
        committee_id.clone(),
        vec![set_approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::SetKyc(set_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add set kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let reason_hash = Hash::new([2u8; 32]);
    let revoke_ts = verified_at + 1;
    let revoke_message = CommitteeApproval::build_revoke_kyc_message(
        blockchain.get_network(),
        &committee_id,
        &user_pubkey,
        &reason_hash,
        revoke_ts,
    );
    let revoke_approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&revoke_message),
        revoke_ts,
    );
    let revoke_payload = RevokeKycPayload::new(
        user_pubkey.clone(),
        reason_hash,
        committee_id.clone(),
        vec![revoke_approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::RevokeKyc(revoke_payload),
        FEE_PER_KB,
        FeeType::TOS,
        1,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add revoke kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 61,
        "method": "get_kyc",
        "params": { "address": user_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_kyc request")
        .json::<serde_json::Value>()
        .await
        .expect("get_kyc response");

    let kyc = resp
        .get("result")
        .and_then(|v| v.get("kyc"))
        .expect("kyc in response");
    assert_eq!(kyc.get("status").and_then(|v| v.as_str()), Some("Revoked"));

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_kyc_renew_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-kyc-renew".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_kyc_renew_on_chain());
        })
        .expect("spawn fullstack kyc renew thread");
    handle.join().expect("join fullstack kyc renew thread");
}

async fn run_fullstack_kyc_renew_on_chain() {
    let test_server = start_rpc_server().await;
    let client = reqwest::Client::new();
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let committee_member = KeyPair::new();
    let member_pubkey = committee_member.get_public_key().compress();
    let user = KeyPair::new();
    let user_pubkey = user.get_public_key().compress();
    let user_address = Address::new(false, AddressType::Normal, user_pubkey.clone());

    let committee_id = SecurityCommittee::compute_id(KycRegion::Global, "kyc-renew", 1);
    let committee = SecurityCommittee::new(
        committee_id.clone(),
        KycRegion::Global,
        "kyc-renew".to_string(),
        vec![CommitteeMember::new(
            member_pubkey.clone(),
            Some("member-1".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .import_committee(&committee_id, &committee)
            .await
            .expect("import committee");
    }
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &member_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let verified_at = now.saturating_sub(10);
    let data_hash = Hash::new([3u8; 32]);
    let set_message = CommitteeApproval::build_set_kyc_message(
        blockchain.get_network(),
        &committee_id,
        &user_pubkey,
        31,
        &data_hash,
        verified_at,
        verified_at,
    );
    let set_approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&set_message),
        verified_at,
    );
    let set_payload = SetKycPayload::new(
        user_pubkey.clone(),
        31,
        verified_at,
        data_hash,
        committee_id.clone(),
        vec![set_approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::SetKyc(set_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add set kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let new_verified_at = now;
    let new_data_hash = Hash::new([4u8; 32]);
    let renew_message = CommitteeApproval::build_renew_kyc_message(
        blockchain.get_network(),
        &committee_id,
        &user_pubkey,
        &new_data_hash,
        new_verified_at,
        new_verified_at,
    );
    let renew_approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&renew_message),
        new_verified_at,
    );
    let renew_payload = RenewKycPayload::new(
        user_pubkey.clone(),
        new_verified_at,
        new_data_hash,
        committee_id.clone(),
        vec![renew_approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::RenewKyc(renew_payload),
        FEE_PER_KB,
        FeeType::TOS,
        1,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add renew kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 62,
        "method": "get_kyc",
        "params": { "address": user_address.to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", test_server.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_kyc request")
        .json::<serde_json::Value>()
        .await
        .expect("get_kyc response");

    let kyc = resp
        .get("result")
        .and_then(|v| v.get("kyc"))
        .expect("kyc in response");
    assert_eq!(
        kyc.get("verified_at").and_then(|v| v.as_u64()),
        Some(new_verified_at)
    );

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_kyc_transfer_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-kyc-transfer".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_kyc_transfer_on_chain());
        })
        .expect("spawn fullstack kyc transfer thread");
    handle.join().expect("join fullstack kyc transfer thread");
}

async fn run_fullstack_kyc_transfer_on_chain() {
    let test_server = start_rpc_server().await;
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let source_member = KeyPair::new();
    let dest_member = KeyPair::new();
    let user = KeyPair::new();
    let user_pubkey = user.get_public_key().compress();

    let source_committee_id = SecurityCommittee::compute_id(KycRegion::Global, "kyc-src", 1);
    let dest_committee_id = SecurityCommittee::compute_id(KycRegion::Global, "kyc-dst", 1);
    let source_committee = SecurityCommittee::new(
        source_committee_id.clone(),
        KycRegion::Global,
        "kyc-src".to_string(),
        vec![CommitteeMember::new(
            source_member.get_public_key().compress(),
            Some("source-member".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );
    let dest_committee = SecurityCommittee::new(
        dest_committee_id.clone(),
        KycRegion::Global,
        "kyc-dst".to_string(),
        vec![CommitteeMember::new(
            dest_member.get_public_key().compress(),
            Some("dest-member".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .import_committee(&source_committee_id, &source_committee)
            .await
            .expect("import source committee");
        storage
            .import_committee(&dest_committee_id, &dest_committee)
            .await
            .expect("import dest committee");
    }
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &source_member.get_public_key().compress(),
        TEST_FUNDING_BALANCE,
    )
    .await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let data_hash = Hash::new([5u8; 32]);
    let set_message = CommitteeApproval::build_set_kyc_message(
        blockchain.get_network(),
        &source_committee_id,
        &user_pubkey,
        31,
        &data_hash,
        now,
        now,
    );
    let set_approval = CommitteeApproval::new(
        source_member.get_public_key().compress(),
        source_member.sign(&set_message),
        now,
    );
    let set_payload = SetKycPayload::new(
        user_pubkey.clone(),
        31,
        now,
        data_hash,
        source_committee_id.clone(),
        vec![set_approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        source_member.get_public_key().compress(),
        TransactionType::SetKyc(set_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&source_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add set kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let transferred_at = now + 1;
    let new_data_hash = Hash::new([6u8; 32]);
    let source_message = CommitteeApproval::build_transfer_kyc_source_message(
        blockchain.get_network(),
        &source_committee_id,
        &dest_committee_id,
        &user_pubkey,
        31,
        &new_data_hash,
        transferred_at,
        transferred_at,
    );
    let source_approval = CommitteeApproval::new(
        source_member.get_public_key().compress(),
        source_member.sign(&source_message),
        transferred_at,
    );

    let dest_message = CommitteeApproval::build_transfer_kyc_dest_message(
        blockchain.get_network(),
        &source_committee_id,
        &dest_committee_id,
        &user_pubkey,
        31,
        &new_data_hash,
        transferred_at,
        transferred_at,
    );
    let dest_approval = CommitteeApproval::new(
        dest_member.get_public_key().compress(),
        dest_member.sign(&dest_message),
        transferred_at,
    );

    let transfer_payload = TransferKycPayload::new(
        user_pubkey.clone(),
        source_committee_id.clone(),
        vec![source_approval],
        dest_committee_id.clone(),
        vec![dest_approval],
        new_data_hash,
        transferred_at,
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        source_member.get_public_key().compress(),
        TransactionType::TransferKyc(transfer_payload),
        FEE_PER_KB,
        FeeType::TOS,
        1,
        reference,
    );
    let tx = unsigned.finalize(&source_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add transfer kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let verifying_committee = blockchain
        .get_storage()
        .read()
        .await
        .get_verifying_committee(&user_pubkey)
        .await
        .expect("get verifying committee");
    assert_eq!(verifying_committee, Some(dest_committee_id));

    test_server.server.stop().await;
}

#[test]
fn test_fullstack_kyc_appeal_on_chain() {
    let handle = std::thread::Builder::new()
        .name("tck-fullstack-kyc-appeal".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_fullstack_kyc_appeal_on_chain());
        })
        .expect("spawn fullstack kyc appeal thread");
    handle.join().expect("join fullstack kyc appeal thread");
}

async fn run_fullstack_kyc_appeal_on_chain() {
    let test_server = start_rpc_server().await;
    let blockchain = test_server.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = test_server.miner_pubkey.clone();

    let committee_member = KeyPair::new();
    let member_pubkey = committee_member.get_public_key().compress();
    let user = KeyPair::new();
    let user_pubkey = user.get_public_key().compress();

    let parent_committee_id = SecurityCommittee::compute_id(KycRegion::Global, "kyc-parent", 1);
    let parent_committee = SecurityCommittee::new(
        parent_committee_id.clone(),
        KycRegion::Global,
        "kyc-parent".to_string(),
        vec![CommitteeMember::new(
            member_pubkey.clone(),
            Some("parent-member".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        None,
        1,
    );
    let original_committee_id =
        SecurityCommittee::compute_id(KycRegion::NorthAmerica, "kyc-original", 1);
    let original_committee = SecurityCommittee::new(
        original_committee_id.clone(),
        KycRegion::NorthAmerica,
        "kyc-original".to_string(),
        vec![CommitteeMember::new(
            member_pubkey.clone(),
            Some("original-member".to_string()),
            MemberRole::Member,
            1,
        )],
        1,
        32767,
        Some(parent_committee_id.clone()),
        1,
    );

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .import_committee(&parent_committee_id, &parent_committee)
            .await
            .expect("import parent committee");
        storage
            .import_committee(&original_committee_id, &original_committee)
            .await
            .expect("import original committee");
    }
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &member_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain,
        &test_server.miner_keypair,
        &user_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let data_hash = Hash::new([7u8; 32]);
    let set_message = CommitteeApproval::build_set_kyc_message(
        blockchain.get_network(),
        &original_committee_id,
        &user_pubkey,
        31,
        &data_hash,
        now,
        now,
    );
    let set_approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&set_message),
        now,
    );
    let set_payload = SetKycPayload::new(
        user_pubkey.clone(),
        31,
        now,
        data_hash,
        original_committee_id.clone(),
        vec![set_approval],
    );

    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::SetKyc(set_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add set kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let reason_hash = Hash::new([8u8; 32]);
    let revoke_message = CommitteeApproval::build_revoke_kyc_message(
        blockchain.get_network(),
        &original_committee_id,
        &user_pubkey,
        &reason_hash,
        now + 1,
    );
    let revoke_approval = CommitteeApproval::new(
        member_pubkey.clone(),
        committee_member.sign(&revoke_message),
        now + 1,
    );
    let revoke_payload = RevokeKycPayload::new(
        user_pubkey.clone(),
        reason_hash,
        original_committee_id.clone(),
        vec![revoke_approval],
    );
    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        member_pubkey.clone(),
        TransactionType::RevokeKyc(revoke_payload),
        FEE_PER_KB,
        FeeType::TOS,
        1,
        reference,
    );
    let tx = unsigned.finalize(&committee_member);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add revoke kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let appeal_payload = AppealKycPayload::new(
        user_pubkey.clone(),
        original_committee_id.clone(),
        parent_committee_id.clone(),
        Hash::new([9u8; 32]),
        Hash::new([10u8; 32]),
        now + 2,
    );
    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        user_pubkey.clone(),
        TransactionType::AppealKyc(appeal_payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(&user);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add appeal kyc tx");
    mine_block_in_chain(&blockchain, &miner_pubkey).await;

    let appeal = blockchain
        .get_storage()
        .read()
        .await
        .get_appeal(&user_pubkey)
        .await
        .expect("get appeal");
    assert!(appeal.is_some());
    let appeal = appeal.unwrap();
    assert_eq!(appeal.original_committee_id, original_committee_id);
    assert_eq!(appeal.parent_committee_id, parent_committee_id);

    test_server.server.stop().await;
}
