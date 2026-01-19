#![allow(clippy::disallowed_methods)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::await_holding_lock)]

use std::sync::{Arc, Mutex, Once};
use std::{convert::Infallible, net::SocketAddr, time::Duration};

use hyper::service::service_fn;
use hyper::Body;
use hyper::{Request, Response, StatusCode};
use once_cell::sync::Lazy;
use rcgen::{generate_simple_self_signed, CertifiedKey};
use sha3::{Digest, Sha3_256};
use tempdir::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::TlsAcceptor;
use tos_common::account::VersionedBalance;
use tos_common::crypto::PublicKey;
use tos_common::serializer::Serializer;
use tos_common::{
    arbitration::{canonical_hash_without_signature, ArbitrationOpen, JurorVote, VoteChoice},
    arbitration::{ArbiterAccount, ArbiterStatus, ExpertiseDomain},
    asset::{AssetData, VersionedAssetData},
    config::{COIN_DECIMALS, COIN_VALUE, MIN_ARBITER_STAKE, TOS_ASSET},
    crypto::{Address, AddressType, Hash, KeyPair},
    escrow::{ArbitrationConfig, ArbitrationMode, DisputeInfo, EscrowAccount, EscrowState},
    kyc::{CommitteeMember, KycRegion, MemberRole, SecurityCommittee},
    network::Network,
    time::get_current_time_in_seconds,
    versioned_type::Versioned,
};
use tos_daemon::{
    a2a::arbitration::evidence::fetch_evidence,
    a2a::arbitration::persistence::load_coordinator_case,
    a2a::arbitration::{ArbitrationError, MAX_CLOCK_DRIFT_SECS},
    a2a::{self, arbitration::coordinator::CoordinatorService},
    core::{
        blockchain::Blockchain,
        config::{Config, RocksDBConfig},
        storage::{
            AccountProvider, ArbiterProvider, AssetProvider, BalanceProvider, CommitteeProvider,
            EscrowProvider, RocksStorage,
        },
    },
};

static TEST_BASE_DIR: Lazy<TempDir> =
    Lazy::new(|| TempDir::new("a2a_arbitration_base").expect("base dir"));
static COORDINATOR_KEYPAIR: Lazy<KeyPair> = Lazy::new(KeyPair::new);
static INIT: Once = Once::new();
static TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
const MIN_JUROR_STAKE: u64 = COIN_VALUE * 10;

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    match TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

async fn build_blockchain(temp_dir: &TempDir) -> Arc<Blockchain<RocksStorage>> {
    eprintln!("[debug] build_blockchain: start");
    let mut config: Config = serde_json::from_value(serde_json::json!({
        "rpc": { "getwork": {}, "prometheus": {} },
        "p2p": { "proxy": {} },
        "rocksdb": {},
        "vrf": {}
    }))
    .expect("build daemon config");
    eprintln!("[debug] build_blockchain: config built");
    config.rpc.disable = true;
    config.rpc.getwork.disable = true;
    config.p2p.disable = true;
    config.skip_pow_verification = true;
    config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
    config.rocksdb = RocksDBConfig::default();

    eprintln!("[debug] build_blockchain: creating storage");
    let storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &config.rocksdb,
    );
    eprintln!("[debug] build_blockchain: storage created, creating blockchain");
    let blockchain = Blockchain::new(config, Network::Devnet, storage)
        .await
        .expect("create blockchain");
    eprintln!("[debug] build_blockchain: blockchain created");

    {
        eprintln!("[debug] build_blockchain: registering TOS asset");
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
        eprintln!("[debug] build_blockchain: TOS asset registered");
    }

    eprintln!("[debug] build_blockchain: done");
    blockchain
}

fn private_key_hex(keypair: &KeyPair) -> String {
    let mut bytes = Vec::new();
    let mut writer = tos_common::serializer::Writer::new(&mut bytes);
    keypair.get_private_key().write(&mut writer);
    hex::encode(bytes)
}

fn address_for(network: Network, pubkey: PublicKey) -> String {
    Address::new(network.is_mainnet(), AddressType::Normal, pubkey).to_string()
}

fn hash_bytes(bytes: &[u8]) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    Hash::new(digest.into())
}

fn build_open(
    fixture: &ArbitrationFixture,
    network: Network,
    request_id: Hash,
    opener: &KeyPair,
) -> ArbitrationOpen {
    let now = get_current_time_in_seconds() as u64;
    let coordinator_pub = COORDINATOR_KEYPAIR.get_public_key().compress();
    let juror_pub = fixture.juror.get_public_key().compress();
    let mut open = ArbitrationOpen {
        message_type: "ArbitrationOpen".to_string(),
        version: 1,
        chain_id: network.chain_id(),
        escrow_id: fixture.escrow_id.clone(),
        escrow_hash: Hash::zero(),
        dispute_id: fixture.dispute_id.clone(),
        round: 0,
        dispute_open_height: 0,
        committee_id: fixture.committee_id.clone(),
        committee_policy_hash: Hash::zero(),
        payer: address_for(network, coordinator_pub.clone()),
        payee: address_for(network, juror_pub.clone()),
        evidence_uri: format!("a2a://artifact/{}", fixture.evidence_hash.to_hex()),
        evidence_hash: fixture.evidence_hash.clone(),
        evidence_manifest_uri: "".to_string(),
        evidence_manifest_hash: Hash::zero(),
        client_nonce: "nonce-1".to_string(),
        issued_at: now,
        expires_at: now + 600,
        coordinator_pubkey: coordinator_pub.clone(),
        coordinator_account: address_for(network, coordinator_pub),
        request_id,
        opener_pubkey: opener.get_public_key().compress(),
        signature: opener.sign(b"placeholder"),
    };
    let open_hash = canonical_hash_without_signature(&open, "signature").expect("hash open");
    open.signature = opener.sign(open_hash.as_bytes());
    open
}

fn build_open_for(
    committee_id: Hash,
    escrow_id: Hash,
    dispute_id: Hash,
    evidence_hash: Hash,
    request_id: Hash,
    network: Network,
    opener: &KeyPair,
    payee_pubkey: &PublicKey,
) -> ArbitrationOpen {
    let now = get_current_time_in_seconds() as u64;
    let coordinator_pub = COORDINATOR_KEYPAIR.get_public_key().compress();
    let mut open = ArbitrationOpen {
        message_type: "ArbitrationOpen".to_string(),
        version: 1,
        chain_id: network.chain_id(),
        escrow_id,
        escrow_hash: Hash::zero(),
        dispute_id,
        round: 0,
        dispute_open_height: 0,
        committee_id,
        committee_policy_hash: Hash::zero(),
        payer: address_for(network, coordinator_pub.clone()),
        payee: address_for(network, payee_pubkey.clone()),
        evidence_uri: format!("a2a://artifact/{}", evidence_hash.to_hex()),
        evidence_hash,
        evidence_manifest_uri: "".to_string(),
        evidence_manifest_hash: Hash::zero(),
        client_nonce: "nonce-1".to_string(),
        issued_at: now,
        expires_at: now + 600,
        coordinator_pubkey: coordinator_pub.clone(),
        coordinator_account: address_for(network, coordinator_pub),
        request_id,
        opener_pubkey: opener.get_public_key().compress(),
        signature: opener.sign(b"placeholder"),
    };
    let open_hash = canonical_hash_without_signature(&open, "signature").expect("hash open");
    open.signature = opener.sign(open_hash.as_bytes());
    open
}

fn build_vote(
    fixture: &ArbitrationFixture,
    request: &tos_common::arbitration::VoteRequest,
) -> JurorVote {
    let voted_at = get_current_time_in_seconds() as u64;
    let mut vote = JurorVote {
        message_type: "JurorVote".to_string(),
        version: request.version,
        request_id: request.request_id.clone(),
        chain_id: request.chain_id,
        escrow_id: request.escrow_id.clone(),
        escrow_hash: request.escrow_hash.clone(),
        dispute_id: request.dispute_id.clone(),
        round: request.round,
        dispute_open_height: request.dispute_open_height,
        committee_id: request.committee_id.clone(),
        selection_block: request.selection_block,
        selection_commitment_id: request.selection_commitment_id.clone(),
        arbitration_open_hash: request.arbitration_open_hash.clone(),
        vote_request_hash: canonical_hash_without_signature(request, "signature")
            .expect("vote hash"),
        evidence_hash: request.evidence_hash.clone(),
        evidence_manifest_hash: request.evidence_manifest_hash.clone(),
        selected_jurors_hash: request.selected_jurors_hash.clone(),
        committee_policy_hash: request.committee_policy_hash.clone(),
        juror_pubkey: fixture.juror.get_public_key().compress(),
        juror_account: fixture.juror_account.clone(),
        vote: VoteChoice::Pay,
        voted_at,
        signature: fixture.juror.sign(b"placeholder"),
    };
    let vote_sig_hash = canonical_hash_without_signature(&vote, "signature").expect("hash vote");
    vote.signature = fixture.juror.sign(vote_sig_hash.as_bytes());
    vote
}

fn init_env() {
    INIT.call_once(|| {
        a2a::set_base_dir(TEST_BASE_DIR.path().to_str().unwrap());
        std::env::set_var(
            "TOS_ARBITRATION_COORDINATOR_PRIVATE_KEY",
            private_key_hex(&COORDINATOR_KEYPAIR),
        );
    });
}

fn build_arbiter_account(
    public_key: PublicKey,
    status: ArbiterStatus,
    stake_amount: u64,
) -> ArbiterAccount {
    ArbiterAccount {
        public_key,
        name: "arbiter".to_string(),
        status,
        expertise: vec![ExpertiseDomain::General],
        stake_amount,
        fee_basis_points: 0,
        min_escrow_value: 0,
        max_escrow_value: u64::MAX,
        reputation_score: 0,
        total_cases: 0,
        cases_overturned: 0,
        registered_at: 0,
        last_active_at: 0,
        pending_withdrawal: 0,
        deactivated_at: None,
        active_cases: 0,
        total_slashed: 0,
        slash_count: 0,
    }
}

async fn seed_committee(
    blockchain: &Arc<Blockchain<RocksStorage>>,
    members: Vec<PublicKey>,
    threshold: u8,
    version: u32,
) -> Hash {
    let committee_id = SecurityCommittee::compute_id(KycRegion::Global, "option-c", version);
    let committee_members = members
        .into_iter()
        .map(|pubkey| {
            CommitteeMember::new(pubkey, Some("member".to_string()), MemberRole::Member, 0)
        })
        .collect::<Vec<_>>();
    let committee = SecurityCommittee::new(
        committee_id.clone(),
        KycRegion::Global,
        "option-c".to_string(),
        committee_members,
        threshold,
        1,
        None,
        0,
    );
    let mut storage = blockchain.get_storage().write().await;
    storage
        .bootstrap_global_committee(committee, 0, &Hash::zero())
        .await
        .expect("bootstrap committee");
    committee_id
}
struct EnvGuard {
    ca_bundle: Option<String>,
    allow_local: Option<String>,
    timeout: Option<String>,
    max_bytes: Option<String>,
    max_redirects: Option<String>,
}

impl EnvGuard {
    fn capture() -> Self {
        Self {
            ca_bundle: std::env::var("TOS_ARBITRATION_CA_BUNDLE").ok(),
            allow_local: std::env::var("TOS_ARBITRATION_TEST_ALLOW_LOCAL").ok(),
            timeout: std::env::var("TOS_ARBITRATION_EVIDENCE_TIMEOUT_SECS").ok(),
            max_bytes: std::env::var("TOS_ARBITRATION_EVIDENCE_MAX_BYTES").ok(),
            max_redirects: std::env::var("TOS_ARBITRATION_EVIDENCE_MAX_REDIRECTS").ok(),
        }
    }

    fn restore(self) {
        set_env_opt("TOS_ARBITRATION_CA_BUNDLE", self.ca_bundle);
        set_env_opt("TOS_ARBITRATION_TEST_ALLOW_LOCAL", self.allow_local);
        set_env_opt("TOS_ARBITRATION_EVIDENCE_TIMEOUT_SECS", self.timeout);
        set_env_opt("TOS_ARBITRATION_EVIDENCE_MAX_BYTES", self.max_bytes);
        set_env_opt("TOS_ARBITRATION_EVIDENCE_MAX_REDIRECTS", self.max_redirects);
    }
}

fn set_env_opt(key: &str, value: Option<String>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

struct HttpsTestServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    _temp_dir: TempDir,
    env_guard: Option<EnvGuard>,
}

impl HttpsTestServer {
    fn url(&self, path: &str) -> String {
        format!("https://localhost:{}{}", self.addr.port(), path)
    }

    fn shutdown(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
    }
}

async fn start_https_server<F>(handler: F) -> HttpsTestServer
where
    F: Fn(Request<Body>) -> Response<Body> + Send + Sync + 'static,
{
    let env_guard = EnvGuard::capture();
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert");
    let cert_pem = cert.pem();
    let key_der = key_pair.serialize_der();
    let cert_der = cert.der().clone();

    let temp_dir = TempDir::new("a2a_arbitration_https").expect("temp dir");
    let cert_path = temp_dir.path().join("ca.pem");
    std::fs::write(&cert_path, cert_pem).expect("write cert");

    std::env::set_var("TOS_ARBITRATION_CA_BUNDLE", &cert_path);
    std::env::set_var("TOS_ARBITRATION_TEST_ALLOW_LOCAL", "1");

    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key)
        .expect("tls config");
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (shutdown, mut rx) = oneshot::channel();
    let handler = Arc::new(handler);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                accept = listener.accept() => {
                    let (stream, _) = match accept {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let acceptor = acceptor.clone();
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        let tls = match acceptor.accept(stream).await {
                            Ok(tls) => tls,
                            Err(_) => return,
                        };
                        let service = service_fn(move |req| {
                            let response = handler(req);
                            async move { Ok::<_, Infallible>(response) }
                        });
                        let _ = hyper::server::conn::Http::new()
                            .serve_connection(tls, service)
                            .await;
                    });
                }
            }
        }
    });

    HttpsTestServer {
        addr,
        shutdown: Some(shutdown),
        _temp_dir: temp_dir,
        env_guard: Some(env_guard),
    }
}

impl Drop for HttpsTestServer {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(guard) = self.env_guard.take() {
            guard.restore();
        }
    }
}

struct ArbitrationFixture {
    committee_id: Hash,
    escrow_id: Hash,
    dispute_id: Hash,
    juror: KeyPair,
    juror_account: String,
    evidence_hash: Hash,
}

async fn setup_fixture(
    blockchain: &Arc<Blockchain<RocksStorage>>,
    evidence_tag: u8,
) -> ArbitrationFixture {
    let network = *blockchain.get_network();
    let coordinator_pub = COORDINATOR_KEYPAIR.get_public_key().compress();

    let juror = KeyPair::new();
    let juror_pub = juror.get_public_key().compress();
    let juror_account = address_for(network, juror_pub.clone());

    let committee_id = SecurityCommittee::compute_id(KycRegion::Global, "test-committee", 1);
    let committee_member = CommitteeMember::new(
        juror_pub.clone(),
        Some("juror".to_string()),
        MemberRole::Member,
        0,
    );
    let committee = SecurityCommittee::new(
        committee_id.clone(),
        KycRegion::Global,
        "test-committee".to_string(),
        vec![committee_member],
        1,
        1,
        None,
        0,
    );

    let escrow_id = Hash::new([7u8; 32]);
    let dispute_id = Hash::new([8u8; 32]);

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .bootstrap_global_committee(committee, 0, &Hash::zero())
            .await
            .expect("bootstrap committee");

        let arbiter = ArbiterAccount {
            public_key: coordinator_pub.clone(),
            name: "coordinator".to_string(),
            status: ArbiterStatus::Active,
            expertise: vec![ExpertiseDomain::General],
            stake_amount: MIN_ARBITER_STAKE + 1,
            fee_basis_points: 0,
            min_escrow_value: 0,
            max_escrow_value: u64::MAX,
            reputation_score: 0,
            total_cases: 0,
            cases_overturned: 0,
            registered_at: 0,
            last_active_at: 0,
            pending_withdrawal: 0,
            deactivated_at: None,
            active_cases: 0,
            total_slashed: 0,
            slash_count: 0,
        };
        storage.set_arbiter(&arbiter).await.expect("set arbiter");

        let juror_arbiter = ArbiterAccount {
            public_key: juror_pub.clone(),
            name: "juror".to_string(),
            status: ArbiterStatus::Active,
            expertise: vec![ExpertiseDomain::General],
            stake_amount: MIN_JUROR_STAKE + 1,
            fee_basis_points: 0,
            min_escrow_value: 0,
            max_escrow_value: u64::MAX,
            reputation_score: 0,
            total_cases: 0,
            cases_overturned: 0,
            registered_at: 0,
            last_active_at: 0,
            pending_withdrawal: 0,
            deactivated_at: None,
            active_cases: 0,
            total_slashed: 0,
            slash_count: 0,
        };
        storage
            .set_arbiter(&juror_arbiter)
            .await
            .expect("set juror arbiter");

        let escrow = EscrowAccount {
            id: escrow_id.clone(),
            task_id: "task-1".to_string(),
            payer: coordinator_pub.clone(),
            payee: juror_pub.clone(),
            amount: 100,
            total_amount: 100,
            released_amount: 0,
            refunded_amount: 0,
            pending_release_amount: None,
            challenge_deposit: 0,
            asset: TOS_ASSET,
            state: EscrowState::Challenged,
            dispute_id: Some(dispute_id.clone()),
            dispute_round: None,
            challenge_window: 10,
            challenge_deposit_bps: 500,
            optimistic_release: false,
            release_requested_at: None,
            created_at: 1,
            updated_at: 1,
            timeout_at: 100,
            timeout_blocks: 0,
            arbitration_config: Some(ArbitrationConfig {
                mode: ArbitrationMode::Single,
                arbiters: vec![coordinator_pub.clone()],
                threshold: Some(1),
                fee_amount: 0,
                allow_appeal: false,
            }),
            dispute: Some(DisputeInfo {
                initiator: coordinator_pub.clone(),
                reason: "dispute".to_string(),
                evidence_hash: None,
                disputed_at: 1,
                deadline: 100,
            }),
            appeal: None,
            resolutions: Vec::new(),
        };
        storage.set_escrow(&escrow).await.expect("set escrow");
        let stored = storage
            .get_escrow(&escrow_id)
            .await
            .expect("get escrow")
            .expect("escrow exists");
        assert_eq!(stored.id, escrow_id);

        storage
            .set_account_registration_topoheight(&coordinator_pub, 0)
            .await
            .expect("register account");

        let balance = VersionedBalance::new(1_000_000, None);
        storage
            .set_last_balance_to(&coordinator_pub, &TOS_ASSET, 0, &balance)
            .await
            .expect("set balance");
    }

    let evidence_bytes = vec![evidence_tag; 16];
    let evidence_hash = hash_bytes(&evidence_bytes);
    tos_daemon::a2a::arbitration::evidence::store_a2a_artifact(&evidence_hash, &evidence_bytes)
        .expect("store evidence");

    ArbitrationFixture {
        committee_id,
        escrow_id,
        dispute_id,
        juror,
        juror_account,
        evidence_hash,
    }
}

#[tokio::test]
async fn arbitration_happy_path_submits_verdict() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_flow_test").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();

    let fixture = setup_fixture(&blockchain, 1).await;
    let coordinator_pub = COORDINATOR_KEYPAIR.get_public_key().compress();
    let juror_pub = fixture.juror.get_public_key().compress();

    let request_id = Hash::new([9u8; 32]);
    let opener = KeyPair::new();
    let open = build_open(&fixture, network, request_id, &opener);

    let coordinator_service = CoordinatorService::new();
    let vote_request = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect("vote request");

    // Verify coordinator signature on VoteRequest
    let vote_request_hash =
        canonical_hash_without_signature(&vote_request, "signature").expect("hash vote request");
    let coordinator_pub = coordinator_pub
        .decompress()
        .expect("decompress coordinator");
    assert!(vote_request
        .signature
        .verify(vote_request_hash.as_bytes(), &coordinator_pub));

    let vote_hash =
        canonical_hash_without_signature(&vote_request, "signature").expect("vote hash");
    let open_hash = vote_request.arbitration_open_hash.clone();
    let voted_at = get_current_time_in_seconds() as u64;
    let mut vote = JurorVote {
        message_type: "JurorVote".to_string(),
        version: vote_request.version,
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
        arbitration_open_hash: open_hash.clone(),
        vote_request_hash: vote_hash.clone(),
        evidence_hash: vote_request.evidence_hash.clone(),
        evidence_manifest_hash: vote_request.evidence_manifest_hash.clone(),
        selected_jurors_hash: vote_request.selected_jurors_hash.clone(),
        committee_policy_hash: vote_request.committee_policy_hash.clone(),
        juror_pubkey: juror_pub.clone(),
        juror_account: fixture.juror_account.clone(),
        vote: VoteChoice::Pay,
        voted_at,
        signature: fixture.juror.sign(b"placeholder"),
    };
    let vote_sig_hash = canonical_hash_without_signature(&vote, "signature").expect("hash vote");
    vote.signature = fixture.juror.sign(vote_sig_hash.as_bytes());

    let verdict = coordinator_service
        .handle_juror_vote(&blockchain, vote)
        .await
        .expect("verdict")
        .expect("verdict exists");

    let verdict_hash =
        canonical_hash_without_signature(&verdict, "coordinatorSignature").expect("hash verdict");
    let verdict_pub = verdict
        .coordinator_pubkey
        .decompress()
        .expect("decompress verdict");
    assert!(verdict
        .coordinator_signature
        .verify(verdict_hash.as_bytes(), &verdict_pub));

    assert_eq!(blockchain.get_mempool_size().await, 1);
}

#[tokio::test]
async fn arbitration_open_rejects_invalid_signature() {
    let _guard = lock_test();
    init_env();
    eprintln!("[debug] arbitration_open_rejects_invalid_signature: start");
    eprintln!("[debug] creating temp dir");
    let temp_dir = TempDir::new("a2a_arbitration_open_bad_sig").expect("temp dir");
    eprintln!("[debug] temp dir created");
    let blockchain = build_blockchain(&temp_dir).await;
    eprintln!("[debug] built blockchain");
    let network = *blockchain.get_network();
    let fixture = setup_fixture(&blockchain, 2).await;
    eprintln!("[debug] setup_fixture done");

    let opener = KeyPair::new();
    let mut open = build_open(&fixture, network, Hash::new([10u8; 32]), &opener);
    open.signature = COORDINATOR_KEYPAIR.sign(b"wrong");
    eprintln!("[debug] open built and invalid signature set");

    let coordinator_service = CoordinatorService::new();
    eprintln!("[debug] coordinator service created, calling handle_arbitration_open");
    let err = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect_err("invalid signature");
    eprintln!("[debug] handle_arbitration_open returned error");
    assert!(matches!(err, ArbitrationError::InvalidSignature));
}

#[tokio::test]
async fn arbitration_open_rejects_clock_drift() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_open_drift").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();
    let fixture = setup_fixture(&blockchain, 3).await;

    let opener = KeyPair::new();
    let mut open = build_open(&fixture, network, Hash::new([11u8; 32]), &opener);
    open.issued_at = open.issued_at.saturating_sub(MAX_CLOCK_DRIFT_SECS + 5);
    open.expires_at = open.issued_at + 600;
    let open_hash = canonical_hash_without_signature(&open, "signature").expect("hash open");
    open.signature = opener.sign(open_hash.as_bytes());

    let coordinator_service = CoordinatorService::new();
    let err = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect_err("expired");
    assert!(matches!(err, ArbitrationError::Expired));
}

#[tokio::test]
async fn arbitration_open_replay_detected() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_open_replay").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();
    let fixture = setup_fixture(&blockchain, 4).await;

    let opener = KeyPair::new();
    let open = build_open(&fixture, network, Hash::new([12u8; 32]), &opener);

    let coordinator_service = CoordinatorService::new();
    let _ = coordinator_service
        .handle_arbitration_open(&blockchain, open.clone())
        .await
        .expect("first open");
    let err = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect_err("replay");
    assert!(matches!(err, ArbitrationError::Replay));
}

#[tokio::test]
async fn juror_vote_replay_returns_none() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_vote_replay").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();
    let fixture = setup_fixture(&blockchain, 5).await;

    let opener = KeyPair::new();
    let open = build_open(&fixture, network, Hash::new([13u8; 32]), &opener);

    let coordinator_service = CoordinatorService::new();
    let vote_request = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect("vote request");

    let vote_hash =
        canonical_hash_without_signature(&vote_request, "signature").expect("vote hash");
    let voted_at = get_current_time_in_seconds() as u64;
    let mut vote = JurorVote {
        message_type: "JurorVote".to_string(),
        version: vote_request.version,
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
        vote_request_hash: vote_hash.clone(),
        evidence_hash: vote_request.evidence_hash.clone(),
        evidence_manifest_hash: vote_request.evidence_manifest_hash.clone(),
        selected_jurors_hash: vote_request.selected_jurors_hash.clone(),
        committee_policy_hash: vote_request.committee_policy_hash.clone(),
        juror_pubkey: fixture.juror.get_public_key().compress(),
        juror_account: fixture.juror_account.clone(),
        vote: VoteChoice::Pay,
        voted_at,
        signature: fixture.juror.sign(b"placeholder"),
    };
    let vote_sig_hash = canonical_hash_without_signature(&vote, "signature").expect("hash vote");
    vote.signature = fixture.juror.sign(vote_sig_hash.as_bytes());

    let _ = coordinator_service
        .handle_juror_vote(&blockchain, vote.clone())
        .await
        .expect("first vote");
    let second = coordinator_service
        .handle_juror_vote(&blockchain, vote)
        .await
        .expect("second vote");
    assert!(second.is_none());
}

#[tokio::test]
async fn arbitration_open_filters_non_arbiters() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_non_arbiters").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();

    let keypairs = (0..5).map(|_| KeyPair::new()).collect::<Vec<_>>();
    let members = keypairs
        .iter()
        .map(|kp| kp.get_public_key().compress())
        .collect::<Vec<_>>();
    let committee_id = seed_committee(&blockchain, members.clone(), 3, 100).await;

    {
        let mut storage = blockchain.get_storage().write().await;
        for kp in keypairs.iter().take(3) {
            let arbiter = build_arbiter_account(
                kp.get_public_key().compress(),
                ArbiterStatus::Active,
                MIN_JUROR_STAKE + 1,
            );
            storage.set_arbiter(&arbiter).await.expect("set arbiter");
        }
    }

    let opener = KeyPair::new();
    let open = build_open_for(
        committee_id,
        Hash::new([2u8; 32]),
        Hash::new([3u8; 32]),
        Hash::new([4u8; 32]),
        Hash::new([1u8; 32]),
        network,
        &opener,
        &keypairs[0].get_public_key().compress(),
    );

    let coordinator_service = CoordinatorService::new();
    let vote_request = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect("vote request");

    let mut expected = keypairs
        .iter()
        .take(3)
        .map(|kp| address_for(network, kp.get_public_key().compress()))
        .collect::<Vec<_>>();
    expected.sort();
    let mut selected = vote_request.selected_jurors.clone();
    selected.sort();
    assert_eq!(selected, expected);
}

#[tokio::test]
async fn arbitration_open_filters_low_stake_arbiters() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_low_stake").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();

    let keypairs = (0..3).map(|_| KeyPair::new()).collect::<Vec<_>>();
    let members = keypairs
        .iter()
        .map(|kp| kp.get_public_key().compress())
        .collect::<Vec<_>>();
    let committee_id = seed_committee(&blockchain, members, 2, 101).await;

    {
        let mut storage = blockchain.get_storage().write().await;
        let high = build_arbiter_account(
            keypairs[0].get_public_key().compress(),
            ArbiterStatus::Active,
            MIN_JUROR_STAKE + 1,
        );
        let low = build_arbiter_account(
            keypairs[1].get_public_key().compress(),
            ArbiterStatus::Active,
            MIN_JUROR_STAKE.saturating_sub(1),
        );
        let high2 = build_arbiter_account(
            keypairs[2].get_public_key().compress(),
            ArbiterStatus::Active,
            MIN_JUROR_STAKE + 100,
        );
        storage.set_arbiter(&high).await.expect("set arbiter");
        storage.set_arbiter(&low).await.expect("set arbiter");
        storage.set_arbiter(&high2).await.expect("set arbiter");
    }

    let opener = KeyPair::new();
    let open = build_open_for(
        committee_id,
        Hash::new([5u8; 32]),
        Hash::new([6u8; 32]),
        Hash::new([7u8; 32]),
        Hash::new([2u8; 32]),
        network,
        &opener,
        &keypairs[0].get_public_key().compress(),
    );

    let coordinator_service = CoordinatorService::new();
    let vote_request = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect("vote request");

    let mut expected = vec![
        address_for(network, keypairs[0].get_public_key().compress()),
        address_for(network, keypairs[2].get_public_key().compress()),
    ];
    expected.sort();
    let mut selected = vote_request.selected_jurors.clone();
    selected.sort();
    assert_eq!(selected, expected);
}

#[tokio::test]
async fn arbitration_open_filters_inactive_arbiters() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_inactive_arbiter").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();

    let keypairs = (0..3).map(|_| KeyPair::new()).collect::<Vec<_>>();
    let members = keypairs
        .iter()
        .map(|kp| kp.get_public_key().compress())
        .collect::<Vec<_>>();
    let committee_id = seed_committee(&blockchain, members, 2, 102).await;

    {
        let mut storage = blockchain.get_storage().write().await;
        let active = build_arbiter_account(
            keypairs[0].get_public_key().compress(),
            ArbiterStatus::Active,
            MIN_JUROR_STAKE + 1,
        );
        let suspended = build_arbiter_account(
            keypairs[1].get_public_key().compress(),
            ArbiterStatus::Suspended,
            MIN_JUROR_STAKE + 1,
        );
        let active2 = build_arbiter_account(
            keypairs[2].get_public_key().compress(),
            ArbiterStatus::Active,
            MIN_JUROR_STAKE + 10,
        );
        storage.set_arbiter(&active).await.expect("set arbiter");
        storage.set_arbiter(&suspended).await.expect("set arbiter");
        storage.set_arbiter(&active2).await.expect("set arbiter");
    }

    let opener = KeyPair::new();
    let open = build_open_for(
        committee_id,
        Hash::new([8u8; 32]),
        Hash::new([9u8; 32]),
        Hash::new([10u8; 32]),
        Hash::new([3u8; 32]),
        network,
        &opener,
        &keypairs[0].get_public_key().compress(),
    );

    let coordinator_service = CoordinatorService::new();
    let vote_request = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect("vote request");

    let mut expected = vec![
        address_for(network, keypairs[0].get_public_key().compress()),
        address_for(network, keypairs[2].get_public_key().compress()),
    ];
    expected.sort();
    let mut selected = vote_request.selected_jurors.clone();
    selected.sort();
    assert_eq!(selected, expected);
}

#[tokio::test]
async fn arbitration_open_errors_when_insufficient_jurors() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_insufficient").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();

    let keypairs = (0..3).map(|_| KeyPair::new()).collect::<Vec<_>>();
    let members = keypairs
        .iter()
        .map(|kp| kp.get_public_key().compress())
        .collect::<Vec<_>>();
    let committee_id = seed_committee(&blockchain, members, 3, 103).await;

    {
        let mut storage = blockchain.get_storage().write().await;
        let active = build_arbiter_account(
            keypairs[0].get_public_key().compress(),
            ArbiterStatus::Active,
            MIN_JUROR_STAKE + 1,
        );
        let suspended = build_arbiter_account(
            keypairs[1].get_public_key().compress(),
            ArbiterStatus::Suspended,
            MIN_JUROR_STAKE + 1,
        );
        storage.set_arbiter(&active).await.expect("set arbiter");
        storage.set_arbiter(&suspended).await.expect("set arbiter");
    }

    let opener = KeyPair::new();
    let open = build_open_for(
        committee_id,
        Hash::new([11u8; 32]),
        Hash::new([12u8; 32]),
        Hash::new([13u8; 32]),
        Hash::new([4u8; 32]),
        network,
        &opener,
        &keypairs[0].get_public_key().compress(),
    );

    let coordinator_service = CoordinatorService::new();
    let err = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect_err("insufficient jurors");
    assert!(matches!(
        err,
        ArbitrationError::InsufficientJurors {
            required: 3,
            available: 1
        }
    ));
}

#[tokio::test]
async fn evidence_rejects_non_https() {
    let _guard = lock_test();
    init_env();
    match fetch_evidence("http://example.com", &Hash::zero()).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(msg.contains("unsupported scheme")),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test]
async fn evidence_blocks_loopback_ip() {
    let _guard = lock_test();
    init_env();
    match fetch_evidence("https://127.0.0.1/resource", &Hash::zero()).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(msg.contains("blocked ip")),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test]
async fn evidence_detects_hash_mismatch() {
    let _guard = lock_test();
    init_env();
    let bytes = b"evidence bytes";
    let hash = hash_bytes(bytes);
    tos_daemon::a2a::arbitration::evidence::store_a2a_artifact(&hash, bytes)
        .expect("store evidence");
    let wrong = Hash::new([9u8; 32]);
    let uri = format!("a2a://artifact/{}", hash.to_hex());
    match fetch_evidence(&uri, &wrong).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(msg.contains("hash mismatch")),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test]
async fn evidence_rejects_oversize_payload() {
    let _guard = lock_test();
    init_env();
    let env_guard = EnvGuard::capture();
    std::env::set_var("TOS_ARBITRATION_EVIDENCE_MAX_BYTES", "1024");

    let mut server = start_https_server(|_req| {
        let body = vec![42u8; 2048];
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(Body::from(body))
            .expect("response")
    })
    .await;

    let url = server.url("/oversize");
    match fetch_evidence(&url, &Hash::zero()).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(msg.contains("evidence too large")),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
    server.shutdown();
    drop(server);
    env_guard.restore();
}

#[tokio::test]
async fn evidence_times_out() {
    let _guard = lock_test();
    init_env();
    let env_guard = EnvGuard::capture();
    std::env::set_var("TOS_ARBITRATION_EVIDENCE_TIMEOUT_SECS", "1");

    let mut server = start_https_server(|_req| {
        std::thread::sleep(Duration::from_secs(2));
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(Body::from(vec![1u8; 8]))
            .expect("response")
    })
    .await;

    let url = server.url("/timeout");
    match fetch_evidence(&url, &Hash::zero()).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(
            msg.contains("timeout") || msg.contains("timed out") || msg.contains("deadline")
        ),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
    server.shutdown();
    drop(server);
    env_guard.restore();
}

#[tokio::test]
async fn evidence_rejects_too_many_redirects() {
    let _guard = lock_test();
    init_env();
    let env_guard = EnvGuard::capture();
    std::env::set_var("TOS_ARBITRATION_EVIDENCE_MAX_REDIRECTS", "2");

    let mut server = start_https_server(|req| {
        let path = req.uri().path().trim_start_matches("/redirect/");
        let idx = path.parse::<u32>().unwrap_or(0);
        if idx >= 3 {
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![7u8; 8]))
                .expect("response");
        }
        let next = format!("/redirect/{}", idx + 1);
        Response::builder()
            .status(StatusCode::FOUND)
            .header("location", next)
            .body(Body::empty())
            .expect("response")
    })
    .await;

    let url = server.url("/redirect/0");
    match fetch_evidence(&url, &Hash::zero()).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(msg.contains("too many redirects")),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
    server.shutdown();
    drop(server);
    env_guard.restore();
}

#[tokio::test]
async fn evidence_accepts_https_redirect_within_limit() {
    let _guard = lock_test();
    init_env();
    let env_guard = EnvGuard::capture();
    std::env::set_var("TOS_ARBITRATION_EVIDENCE_MAX_REDIRECTS", "3");

    let payload = vec![3u8; 16];
    let expected = hash_bytes(&payload);

    let mut server = start_https_server(move |req| {
        let path = req.uri().path();
        if path == "/final" {
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/octet-stream")
                .body(Body::from(payload.clone()))
                .expect("response");
        }
        Response::builder()
            .status(StatusCode::FOUND)
            .header("location", "/final")
            .body(Body::empty())
            .expect("response")
    })
    .await;

    let url = server.url("/redirect");
    let result = fetch_evidence(&url, &expected).await;
    assert!(result.is_ok());
    server.shutdown();
    drop(server);
    env_guard.restore();
}

#[tokio::test]
async fn evidence_rejects_disallowed_content_type() {
    let _guard = lock_test();
    init_env();

    let mut server = start_https_server(|_req| {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .body(Body::from(b"not allowed".to_vec()))
            .expect("response")
    })
    .await;

    let url = server.url("/plain");
    match fetch_evidence(&url, &Hash::zero()).await {
        Err(ArbitrationError::Evidence(msg)) => assert!(msg.contains("content-type not allowed")),
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected error"),
    }
    server.shutdown();
}

#[tokio::test]
async fn evidence_accepts_json_content_type() {
    let _guard = lock_test();
    init_env();

    let payload = br#"{"ok":true}"#.to_vec();
    let expected = hash_bytes(&payload);
    let mut server = start_https_server(move |_req| {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(payload.clone()))
            .expect("response")
    })
    .await;

    let url = server.url("/json");
    let result = fetch_evidence(&url, &expected).await;
    assert!(result.is_ok());
    server.shutdown();
}

#[tokio::test]
async fn coordinator_persists_verdict_status() {
    let _guard = lock_test();
    init_env();
    let temp_dir = TempDir::new("a2a_arbitration_persist_verdict").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;
    let network = *blockchain.get_network();
    let fixture = setup_fixture(&blockchain, 6).await;

    let opener = KeyPair::new();
    let open = build_open(&fixture, network, Hash::new([14u8; 32]), &opener);
    let coordinator_service = CoordinatorService::new();
    let vote_request = coordinator_service
        .handle_arbitration_open(&blockchain, open)
        .await
        .expect("vote request");

    let vote = build_vote(&fixture, &vote_request);
    let _ = coordinator_service
        .handle_juror_vote(&blockchain, vote)
        .await
        .expect("verdict");

    let case = load_coordinator_case(&vote_request.request_id)
        .expect("load case")
        .expect("case exists");
    assert!(case.verdict.is_some());
    assert!(case.verdict_submitted);
}
