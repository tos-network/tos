//! Comprehensive Test Suite for Agent Account Protocol
//!
//! Test Categories (per design document section 11):
//! - AA-REG: Register success + already-agent failure
//! - AA-SK: Session key add/revoke success + invalid expiry/limits rejection
//! - AA-AUTH: Admin-only payloads require owner signature
//! - AA-FROZEN: Frozen account rejects controller/session tx
//! - AA-SCOPE: Session key scope enforcement (targets/assets/max_value_per_window)
//! - AA-SERIAL: Serialization roundtrip tests for all payloads
//! - AA-BOUNDS: Boundary condition tests (limits, max values)

use std::{borrow::Cow, collections::HashMap};

use async_trait::async_trait;
use tos_common::network::Network;
use tos_common::{
    account::{AgentAccountMeta, Nonce, SessionKey},
    block::BlockVersion,
    config::TOS_ASSET,
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey, KeyPair},
        Hash,
    },
    serializer::{Reader, Serializer},
    transaction::{
        verify::{agent_account::verify_agent_account_payload, BlockchainVerificationState},
        AgentAccountPayload, MultiSigPayload, Reference,
    },
};
use tos_kernel::{Environment, Module};

// ============================================================================
// Mock State Implementation for Testing
// ============================================================================

#[derive(Debug)]
struct MockStateError(String);

impl std::fmt::Display for MockStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MockStateError: {}", self.0)
    }
}

impl std::error::Error for MockStateError {}

struct MockVerificationState {
    topoheight: u64,
    agent_accounts: HashMap<CompressedPublicKey, AgentAccountMeta>,
    session_keys: HashMap<(CompressedPublicKey, u64), SessionKey>,
    registered_accounts: std::collections::HashSet<CompressedPublicKey>,
    multisig: HashMap<CompressedPublicKey, MultiSigPayload>,
    env: Environment,
}

impl MockVerificationState {
    fn new(topoheight: u64) -> Self {
        Self {
            topoheight,
            agent_accounts: HashMap::new(),
            session_keys: HashMap::new(),
            registered_accounts: std::collections::HashSet::new(),
            multisig: HashMap::new(),
            env: Environment::new(),
        }
    }

    fn register_account(&mut self, account: &CompressedPublicKey) {
        self.registered_accounts.insert(account.clone());
    }

    fn set_agent_meta(&mut self, account: &CompressedPublicKey, meta: AgentAccountMeta) {
        self.agent_accounts.insert(account.clone(), meta);
    }

    fn add_session_key(&mut self, account: &CompressedPublicKey, key: SessionKey) {
        self.session_keys.insert((account.clone(), key.key_id), key);
    }
}

#[async_trait]
impl<'a> BlockchainVerificationState<'a, MockStateError> for MockVerificationState {
    async fn pre_verify_tx<'b>(
        &'b mut self,
        _tx: &tos_common::transaction::Transaction,
    ) -> Result<(), MockStateError> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, MockStateError> {
        unimplemented!("not needed for agent account tests")
    }

    async fn get_sender_balance<'b>(
        &'b mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
        _reference: &Reference,
    ) -> Result<&'b mut u64, MockStateError> {
        unimplemented!("not needed for agent account tests")
    }

    async fn add_sender_output(
        &mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
        _output: u64,
    ) -> Result<(), MockStateError> {
        Ok(())
    }

    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, MockStateError> {
        unimplemented!("not needed for agent account tests")
    }

    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        _account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut Ciphertext, MockStateError> {
        unimplemented!("not needed for agent account tests")
    }

    async fn add_sender_uno_output(
        &mut self,
        _account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _output: Ciphertext,
    ) -> Result<(), MockStateError> {
        Ok(())
    }

    async fn get_account_nonce(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Nonce, MockStateError> {
        Ok(0)
    }

    async fn account_exists(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<bool, MockStateError> {
        Ok(self.registered_accounts.contains(account))
    }

    async fn update_account_nonce(
        &mut self,
        _account: &'a CompressedPublicKey,
        _new_nonce: Nonce,
    ) -> Result<(), MockStateError> {
        Ok(())
    }

    async fn compare_and_swap_nonce(
        &mut self,
        _account: &'a CompressedPublicKey,
        _expected: Nonce,
        _new_value: Nonce,
    ) -> Result<bool, MockStateError> {
        Ok(true)
    }

    async fn get_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<AgentAccountMeta>, MockStateError> {
        Ok(self.agent_accounts.get(account).cloned())
    }

    async fn set_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
        meta: &AgentAccountMeta,
    ) -> Result<(), MockStateError> {
        self.agent_accounts.insert(account.clone(), meta.clone());
        Ok(())
    }

    async fn delete_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<(), MockStateError> {
        self.agent_accounts.remove(account);
        Ok(())
    }

    async fn get_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        key_id: u64,
    ) -> Result<Option<SessionKey>, MockStateError> {
        Ok(self.session_keys.get(&(account.clone(), key_id)).cloned())
    }

    async fn set_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        session_key: &SessionKey,
    ) -> Result<(), MockStateError> {
        self.session_keys
            .insert((account.clone(), session_key.key_id), session_key.clone());
        Ok(())
    }

    async fn delete_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        key_id: u64,
    ) -> Result<(), MockStateError> {
        self.session_keys.remove(&(account.clone(), key_id));
        Ok(())
    }

    async fn get_session_keys_for_account(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Vec<SessionKey>, MockStateError> {
        Ok(self
            .session_keys
            .iter()
            .filter_map(|((acc, _), key)| {
                if acc == account {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect())
    }

    fn get_block_version(&self) -> BlockVersion {
        BlockVersion::Nobunaga
    }

    fn get_verification_timestamp(&self) -> u64 {
        0
    }

    fn get_verification_topoheight(&self) -> u64 {
        self.topoheight
    }

    async fn get_recyclable_tos(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<u64, MockStateError> {
        Ok(0)
    }

    async fn set_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), MockStateError> {
        self.multisig.insert(account.clone(), config.clone());
        Ok(())
    }

    async fn get_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<&MultiSigPayload>, MockStateError> {
        Ok(self.multisig.get(account))
    }

    async fn get_environment(&mut self) -> Result<&Environment, MockStateError> {
        Ok(&self.env)
    }

    fn get_network(&self) -> Network {
        Network::Devnet
    }

    async fn set_contract_module(
        &mut self,
        _hash: &Hash,
        _module: &'a Module,
    ) -> Result<(), MockStateError> {
        Ok(())
    }

    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, MockStateError> {
        let _ = hash;
        Ok(false)
    }

    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), MockStateError> {
        let _ = hash;
        Err(MockStateError("Contract module not found".to_string()))
    }

    async fn is_name_registered(&self, _name_hash: &Hash) -> Result<bool, MockStateError> {
        Ok(false)
    }

    async fn account_has_name(
        &self,
        _account: &'a CompressedPublicKey,
    ) -> Result<bool, MockStateError> {
        Ok(false)
    }

    async fn get_account_name_hash(
        &self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, MockStateError> {
        Ok(None)
    }

    async fn is_message_id_used(&self, _message_id: &Hash) -> Result<bool, MockStateError> {
        Ok(false)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_keypair() -> KeyPair {
    KeyPair::new()
}

fn create_public_key() -> CompressedPublicKey {
    create_keypair().get_public_key().compress()
}

fn create_zero_key() -> CompressedPublicKey {
    let bytes = [0u8; 32];
    let mut reader = Reader::new(&bytes);
    CompressedPublicKey::read(&mut reader).expect("zero key")
}

fn create_policy_hash() -> Hash {
    Hash::new([1u8; 32])
}

fn create_zero_hash() -> Hash {
    Hash::zero()
}

fn create_session_key(
    key_id: u64,
    expiry_topoheight: u64,
    max_value_per_window: u64,
) -> SessionKey {
    SessionKey {
        key_id,
        public_key: create_public_key(),
        expiry_topoheight,
        max_value_per_window,
        allowed_targets: vec![],
        allowed_assets: vec![],
    }
}

// ============================================================================
// AA-REG: Register Tests
// ============================================================================

#[tokio::test]
async fn aa_reg_01_register_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: None,
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "Register should succeed");

    let meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(meta.owner, owner);
    assert_eq!(meta.controller, controller);
    assert_eq!(meta.policy_hash, policy_hash);
    assert_eq!(meta.status, 0);
    assert!(meta.energy_pool.is_none());
    assert!(meta.session_key_root.is_none());
}

#[tokio::test]
async fn aa_reg_02_register_with_energy_pool_owner() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: Some(owner.clone()),
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "Register with energy_pool=owner should succeed"
    );

    let meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(meta.energy_pool, Some(owner.clone()));
}

#[tokio::test]
async fn aa_reg_03_register_with_energy_pool_controller() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: Some(controller.clone()),
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "Register with energy_pool=controller should succeed"
    );
}

#[tokio::test]
async fn aa_reg_04_register_with_session_key_root() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();
    let session_key_root = Hash::new([2u8; 32]);

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: None,
        session_key_root: Some(session_key_root.clone()),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "Register with session_key_root should succeed"
    );

    let meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(meta.session_key_root, Some(session_key_root));
}

#[tokio::test]
async fn aa_reg_05_register_already_registered_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let existing_meta = AgentAccountMeta {
        owner: owner.clone(),
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, existing_meta);

    let new_controller = create_public_key();
    state.register_account(&new_controller);

    let payload = AgentAccountPayload::Register {
        controller: new_controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: None,
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Register should fail for already-agent account"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountAlreadyRegistered"),
        "Expected AgentAccountAlreadyRegistered error"
    );
}

#[tokio::test]
async fn aa_reg_06_register_zero_controller_fails() {
    let owner = create_public_key();
    let controller = create_zero_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);

    let payload = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: None,
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_err(), "Register should fail with zero controller");

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountInvalidController"),
        "Expected AgentAccountInvalidController error"
    );
}

#[tokio::test]
async fn aa_reg_07_register_controller_equals_owner_fails() {
    let owner = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);

    let payload = AgentAccountPayload::Register {
        controller: owner.clone(),
        policy_hash,
        energy_pool: None,
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Register should fail when controller == owner"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountInvalidController"),
        "Expected AgentAccountInvalidController error"
    );
}

#[tokio::test]
async fn aa_reg_08_register_zero_policy_hash_fails() {
    let owner = create_public_key();
    let controller = create_public_key();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller,
        policy_hash: create_zero_hash(),
        energy_pool: None,
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Register should fail with zero policy_hash"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountInvalidParameter"),
        "Expected AgentAccountInvalidParameter error"
    );
}

#[tokio::test]
async fn aa_reg_09_register_energy_pool_not_owner_or_controller_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let third_party = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);
    state.register_account(&third_party);

    let payload = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: Some(third_party),
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Register should fail when energy_pool is not owner or controller"
    );
}

#[tokio::test]
async fn aa_reg_10_register_energy_pool_not_registered_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let unregistered = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash,
        energy_pool: Some(unregistered),
        session_key_root: None,
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Register should fail when energy_pool account doesn't exist"
    );
}

#[tokio::test]
async fn aa_reg_11_register_zero_session_key_root_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let payload = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: None,
        session_key_root: Some(create_zero_hash()),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Register should fail with zero session_key_root"
    );
}

// ============================================================================
// AA-SK: Session Key Tests
// ============================================================================

#[tokio::test]
async fn aa_sk_01_add_session_key_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller: controller.clone(),
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 200, 1000);
    let payload = AgentAccountPayload::AddSessionKey {
        key: session_key.clone(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "AddSessionKey should succeed");

    let stored_key = state.session_keys.get(&(owner.clone(), 1)).unwrap();
    assert_eq!(stored_key.key_id, 1);
    assert_eq!(stored_key.expiry_topoheight, 200);
    assert_eq!(stored_key.max_value_per_window, 1000);
}

#[tokio::test]
async fn aa_sk_02_add_session_key_with_targets_and_assets() {
    let owner = create_public_key();
    let controller = create_public_key();
    let target1 = create_public_key();
    let target2 = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_public_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets: vec![target1.clone(), target2.clone()],
        allowed_assets: vec![TOS_ASSET],
    };

    let payload = AgentAccountPayload::AddSessionKey {
        key: session_key.clone(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "AddSessionKey with targets and assets should succeed"
    );
}

#[tokio::test]
async fn aa_sk_03_add_session_key_expired_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 50, 1000);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with expired expiry_topoheight"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountSessionKeyExpired"),
        "Expected AgentAccountSessionKeyExpired error"
    );
}

#[tokio::test]
async fn aa_sk_04_add_session_key_zero_max_value_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 200, 0);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with max_value_per_window=0"
    );
}

#[tokio::test]
async fn aa_sk_05_add_session_key_duplicate_key_id_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let existing_key = create_session_key(1, 200, 1000);
    state.add_session_key(&owner, existing_key);

    let new_key = create_session_key(1, 300, 2000);
    let payload = AgentAccountPayload::AddSessionKey { key: new_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with duplicate key_id"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountSessionKeyExists"),
        "Expected AgentAccountSessionKeyExists error"
    );
}

#[tokio::test]
async fn aa_sk_06_add_session_key_duplicate_public_key_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();
    let session_pub_key = create_public_key();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let existing_key = SessionKey {
        key_id: 1,
        public_key: session_pub_key.clone(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets: vec![],
        allowed_assets: vec![],
    };
    state.add_session_key(&owner, existing_key);

    let new_key = SessionKey {
        key_id: 2,
        public_key: session_pub_key,
        expiry_topoheight: 300,
        max_value_per_window: 2000,
        allowed_targets: vec![],
        allowed_assets: vec![],
    };
    let payload = AgentAccountPayload::AddSessionKey { key: new_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with duplicate public_key"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountSessionKeyExists"),
        "Expected AgentAccountSessionKeyExists error"
    );
}

#[tokio::test]
async fn aa_sk_07_add_session_key_zero_public_key_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_zero_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets: vec![],
        allowed_assets: vec![],
    };
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with zero public_key"
    );
}

#[tokio::test]
async fn aa_sk_08_add_session_key_with_session_key_root_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: Some(Hash::new([3u8; 32])),
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 200, 1000);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail when session_key_root is set"
    );
}

#[tokio::test]
async fn aa_sk_09_revoke_session_key_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 200, 1000);
    state.add_session_key(&owner, session_key);

    let payload = AgentAccountPayload::RevokeSessionKey { key_id: 1 };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "RevokeSessionKey should succeed");

    assert!(
        state.session_keys.get(&(owner.clone(), 1)).is_none(),
        "Session key should be deleted"
    );
}

#[tokio::test]
async fn aa_sk_10_revoke_session_key_not_found_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::RevokeSessionKey { key_id: 999 };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "RevokeSessionKey should fail for non-existent key"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountSessionKeyNotFound"),
        "Expected AgentAccountSessionKeyNotFound error"
    );
}

// ============================================================================
// AA-ADMIN: Admin Payload Tests
// ============================================================================

#[tokio::test]
async fn aa_admin_01_update_policy_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let new_policy_hash = Hash::new([5u8; 32]);
    let payload = AgentAccountPayload::UpdatePolicy {
        policy_hash: new_policy_hash.clone(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "UpdatePolicy should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(updated_meta.policy_hash, new_policy_hash);
}

#[tokio::test]
async fn aa_admin_02_update_policy_zero_hash_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::UpdatePolicy {
        policy_hash: create_zero_hash(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "UpdatePolicy should fail with zero policy_hash"
    );
}

#[tokio::test]
async fn aa_admin_03_rotate_controller_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let new_controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);
    state.register_account(&new_controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::RotateController {
        new_controller: new_controller.clone(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "RotateController should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(updated_meta.controller, new_controller);
}

#[tokio::test]
async fn aa_admin_04_rotate_controller_to_owner_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::RotateController {
        new_controller: owner.clone(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "RotateController should fail when new_controller == owner"
    );

    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("AgentAccountInvalidController"),
        "Expected AgentAccountInvalidController error"
    );
}

#[tokio::test]
async fn aa_admin_05_rotate_controller_same_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller: controller.clone(),
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::RotateController {
        new_controller: controller.clone(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "RotateController should fail when new_controller == current controller"
    );
}

#[tokio::test]
async fn aa_admin_06_set_status_active() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 1,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::SetStatus { status: 0 };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "SetStatus to active should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(updated_meta.status, 0);
}

#[tokio::test]
async fn aa_admin_07_set_status_frozen() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::SetStatus { status: 1 };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "SetStatus to frozen should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(updated_meta.status, 1);
}

#[tokio::test]
async fn aa_admin_08_set_status_invalid_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::SetStatus { status: 2 };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "SetStatus should fail with invalid status value"
    );
}

#[tokio::test]
async fn aa_admin_09_set_energy_pool_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller: controller.clone(),
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::SetEnergyPool {
        energy_pool: Some(controller.clone()),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "SetEnergyPool should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(updated_meta.energy_pool, Some(controller));
}

#[tokio::test]
async fn aa_admin_10_set_energy_pool_clear_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller: controller.clone(),
        policy_hash,
        status: 0,
        energy_pool: Some(controller.clone()),
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let payload = AgentAccountPayload::SetEnergyPool { energy_pool: None };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "SetEnergyPool to None should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert!(updated_meta.energy_pool.is_none());
}

#[tokio::test]
async fn aa_admin_11_set_session_key_root_success() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let new_root = Hash::new([7u8; 32]);
    let payload = AgentAccountPayload::SetSessionKeyRoot {
        session_key_root: Some(new_root.clone()),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_ok(), "SetSessionKeyRoot should succeed");

    let updated_meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(updated_meta.session_key_root, Some(new_root));
}

#[tokio::test]
async fn aa_admin_12_set_session_key_root_with_active_keys_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let active_key = create_session_key(1, 200, 1000);
    state.add_session_key(&owner, active_key);

    let new_root = Hash::new([7u8; 32]);
    let payload = AgentAccountPayload::SetSessionKeyRoot {
        session_key_root: Some(new_root),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "SetSessionKeyRoot should fail when active session keys exist"
    );
}

#[tokio::test]
async fn aa_admin_13_admin_payload_on_non_agent_fails() {
    let owner = create_public_key();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);

    let payload = AgentAccountPayload::UpdatePolicy {
        policy_hash: create_policy_hash(),
    };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "Admin payload should fail on non-agent account"
    );
}

// ============================================================================
// AA-BOUNDS: Boundary Tests
// ============================================================================

#[tokio::test]
async fn aa_bounds_01_max_allowed_targets() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let allowed_targets: Vec<CompressedPublicKey> = (0..64).map(|_| create_public_key()).collect();

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_public_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets,
        allowed_assets: vec![],
    };

    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "AddSessionKey with 64 targets (max) should succeed"
    );
}

#[tokio::test]
async fn aa_bounds_02_exceed_max_allowed_targets() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let allowed_targets: Vec<CompressedPublicKey> = (0..65).map(|_| create_public_key()).collect();

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_public_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets,
        allowed_assets: vec![],
    };

    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with >64 targets"
    );
}

#[tokio::test]
async fn aa_bounds_03_max_allowed_assets() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let allowed_assets: Vec<Hash> = (0..64).map(|i| Hash::new([i as u8; 32])).collect();

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_public_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets: vec![],
        allowed_assets,
    };

    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "AddSessionKey with 64 assets (max) should succeed"
    );
}

#[tokio::test]
async fn aa_bounds_04_exceed_max_allowed_assets() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let allowed_assets: Vec<Hash> = (0..65).map(|i| Hash::new([i as u8; 32])).collect();

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_public_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets: vec![],
        allowed_assets,
    };

    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(result.is_err(), "AddSessionKey should fail with >64 assets");
}

#[tokio::test]
async fn aa_bounds_05_zero_target_in_list_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = SessionKey {
        key_id: 1,
        public_key: create_public_key(),
        expiry_topoheight: 200,
        max_value_per_window: 1000,
        allowed_targets: vec![create_public_key(), create_zero_key()],
        allowed_assets: vec![],
    };

    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail with zero key in allowed_targets"
    );
}

#[tokio::test]
async fn aa_bounds_06_expiry_at_current_height_fails() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 100, 1000);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_err(),
        "AddSessionKey should fail when expiry_topoheight == current_topoheight"
    );
}

#[tokio::test]
async fn aa_bounds_07_expiry_one_above_current_succeeds() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 101, 1000);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "AddSessionKey should succeed when expiry_topoheight = current + 1"
    );
}

#[tokio::test]
async fn aa_bounds_08_max_u64_expiry_succeeds() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, u64::MAX, 1000);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "AddSessionKey should succeed with u64::MAX expiry"
    );
}

#[tokio::test]
async fn aa_bounds_09_max_u64_max_value_succeeds() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let meta = AgentAccountMeta {
        owner: owner.clone(),
        controller,
        policy_hash,
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };
    state.set_agent_meta(&owner, meta);

    let session_key = create_session_key(1, 200, u64::MAX);
    let payload = AgentAccountPayload::AddSessionKey { key: session_key };

    let result = verify_agent_account_payload(&payload, &owner, &mut state).await;
    assert!(
        result.is_ok(),
        "AddSessionKey should succeed with u64::MAX max_value_per_window"
    );
}

// ============================================================================
// AA-SERIAL: Serialization Tests
// ============================================================================

#[test]
fn aa_serial_01_register_roundtrip() {
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: None,
        session_key_root: None,
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::Register {
            controller: dec_controller,
            policy_hash: dec_policy_hash,
            energy_pool,
            session_key_root,
        } => {
            assert_eq!(dec_controller, controller);
            assert_eq!(dec_policy_hash, policy_hash);
            assert!(energy_pool.is_none());
            assert!(session_key_root.is_none());
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_02_register_with_all_fields_roundtrip() {
    let controller = create_public_key();
    let policy_hash = create_policy_hash();
    let energy_pool = create_public_key();
    let session_key_root = Hash::new([3u8; 32]);

    let payload = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: Some(energy_pool.clone()),
        session_key_root: Some(session_key_root.clone()),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::Register {
            controller: dec_controller,
            policy_hash: dec_policy_hash,
            energy_pool: dec_energy_pool,
            session_key_root: dec_session_key_root,
        } => {
            assert_eq!(dec_controller, controller);
            assert_eq!(dec_policy_hash, policy_hash);
            assert_eq!(dec_energy_pool, Some(energy_pool));
            assert_eq!(dec_session_key_root, Some(session_key_root));
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_03_update_policy_roundtrip() {
    let policy_hash = create_policy_hash();

    let payload = AgentAccountPayload::UpdatePolicy {
        policy_hash: policy_hash.clone(),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::UpdatePolicy {
            policy_hash: dec_policy_hash,
        } => {
            assert_eq!(dec_policy_hash, policy_hash);
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_04_rotate_controller_roundtrip() {
    let new_controller = create_public_key();

    let payload = AgentAccountPayload::RotateController {
        new_controller: new_controller.clone(),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::RotateController {
            new_controller: dec_new_controller,
        } => {
            assert_eq!(dec_new_controller, new_controller);
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_05_set_status_roundtrip() {
    for status in [0u8, 1u8] {
        let payload = AgentAccountPayload::SetStatus { status };

        let bytes = payload.to_bytes();
        let mut reader = Reader::new(&bytes);
        let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

        match decoded {
            AgentAccountPayload::SetStatus { status: dec_status } => {
                assert_eq!(dec_status, status);
            }
            _ => panic!("unexpected payload variant"),
        }
    }
}

#[test]
fn aa_serial_06_set_energy_pool_roundtrip() {
    let energy_pool = create_public_key();

    let payload = AgentAccountPayload::SetEnergyPool {
        energy_pool: Some(energy_pool.clone()),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::SetEnergyPool {
            energy_pool: dec_energy_pool,
        } => {
            assert_eq!(dec_energy_pool, Some(energy_pool));
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_07_set_energy_pool_none_roundtrip() {
    let payload = AgentAccountPayload::SetEnergyPool { energy_pool: None };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::SetEnergyPool {
            energy_pool: dec_energy_pool,
        } => {
            assert!(dec_energy_pool.is_none());
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_08_set_session_key_root_roundtrip() {
    let session_key_root = Hash::new([5u8; 32]);

    let payload = AgentAccountPayload::SetSessionKeyRoot {
        session_key_root: Some(session_key_root.clone()),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::SetSessionKeyRoot {
            session_key_root: dec_session_key_root,
        } => {
            assert_eq!(dec_session_key_root, Some(session_key_root));
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_09_add_session_key_roundtrip() {
    let session_key = SessionKey {
        key_id: 42,
        public_key: create_public_key(),
        expiry_topoheight: 100000,
        max_value_per_window: 50000,
        allowed_targets: vec![create_public_key(), create_public_key()],
        allowed_assets: vec![TOS_ASSET, Hash::new([9u8; 32])],
    };

    let payload = AgentAccountPayload::AddSessionKey {
        key: session_key.clone(),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::AddSessionKey { key: dec_key } => {
            assert_eq!(dec_key, session_key);
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_10_revoke_session_key_roundtrip() {
    let payload = AgentAccountPayload::RevokeSessionKey { key_id: 12345 };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::RevokeSessionKey { key_id: dec_key_id } => {
            assert_eq!(dec_key_id, 12345);
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn aa_serial_11_agent_account_meta_roundtrip() {
    let meta = AgentAccountMeta {
        owner: create_public_key(),
        controller: create_public_key(),
        policy_hash: create_policy_hash(),
        status: 1,
        energy_pool: Some(create_public_key()),
        session_key_root: Some(Hash::new([8u8; 32])),
    };

    let bytes = meta.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountMeta::read(&mut reader).expect("decode meta");

    assert_eq!(decoded.owner, meta.owner);
    assert_eq!(decoded.controller, meta.controller);
    assert_eq!(decoded.policy_hash, meta.policy_hash);
    assert_eq!(decoded.status, meta.status);
    assert_eq!(decoded.energy_pool, meta.energy_pool);
    assert_eq!(decoded.session_key_root, meta.session_key_root);
}

#[test]
fn aa_serial_12_session_key_roundtrip() {
    let session_key = SessionKey {
        key_id: u64::MAX,
        public_key: create_public_key(),
        expiry_topoheight: u64::MAX,
        max_value_per_window: u64::MAX,
        allowed_targets: (0..10).map(|_| create_public_key()).collect(),
        allowed_assets: (0..10).map(|i| Hash::new([i as u8; 32])).collect(),
    };

    let bytes = session_key.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = SessionKey::read(&mut reader).expect("decode session key");

    assert_eq!(decoded, session_key);
}

// ============================================================================
// AA-META: AgentAccountMeta Field Tests
// ============================================================================

#[test]
fn aa_meta_01_minimal_meta() {
    let meta = AgentAccountMeta {
        owner: create_public_key(),
        controller: create_public_key(),
        policy_hash: create_policy_hash(),
        status: 0,
        energy_pool: None,
        session_key_root: None,
    };

    let bytes = meta.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountMeta::read(&mut reader).expect("decode meta");

    assert_eq!(decoded, meta);
}

#[test]
fn aa_meta_02_full_meta() {
    let meta = AgentAccountMeta {
        owner: create_public_key(),
        controller: create_public_key(),
        policy_hash: create_policy_hash(),
        status: 1,
        energy_pool: Some(create_public_key()),
        session_key_root: Some(Hash::new([11u8; 32])),
    };

    let bytes = meta.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountMeta::read(&mut reader).expect("decode meta");

    assert_eq!(decoded, meta);
}

// ============================================================================
// Integration-Style Tests (Multi-Step Scenarios)
// ============================================================================

#[tokio::test]
async fn aa_integration_01_full_lifecycle() {
    let owner = create_public_key();
    let controller = create_public_key();
    let new_controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);
    state.register_account(&new_controller);

    let register = AgentAccountPayload::Register {
        controller: controller.clone(),
        policy_hash: policy_hash.clone(),
        energy_pool: None,
        session_key_root: None,
    };
    verify_agent_account_payload(&register, &owner, &mut state)
        .await
        .expect("Register should succeed");

    let session_key = create_session_key(1, 500, 10000);
    let add_key = AgentAccountPayload::AddSessionKey {
        key: session_key.clone(),
    };
    verify_agent_account_payload(&add_key, &owner, &mut state)
        .await
        .expect("AddSessionKey should succeed");

    let rotate = AgentAccountPayload::RotateController {
        new_controller: new_controller.clone(),
    };
    verify_agent_account_payload(&rotate, &owner, &mut state)
        .await
        .expect("RotateController should succeed");

    let meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(meta.controller, new_controller);

    let freeze = AgentAccountPayload::SetStatus { status: 1 };
    verify_agent_account_payload(&freeze, &owner, &mut state)
        .await
        .expect("SetStatus frozen should succeed");

    let meta = state.agent_accounts.get(&owner).unwrap();
    assert_eq!(meta.status, 1);

    let unfreeze = AgentAccountPayload::SetStatus { status: 0 };
    verify_agent_account_payload(&unfreeze, &owner, &mut state)
        .await
        .expect("SetStatus active should succeed");

    let revoke = AgentAccountPayload::RevokeSessionKey { key_id: 1 };
    verify_agent_account_payload(&revoke, &owner, &mut state)
        .await
        .expect("RevokeSessionKey should succeed");

    assert!(state.session_keys.get(&(owner.clone(), 1)).is_none());
}

#[tokio::test]
async fn aa_integration_02_multiple_session_keys() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let register = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: None,
        session_key_root: None,
    };
    verify_agent_account_payload(&register, &owner, &mut state)
        .await
        .expect("Register should succeed");

    for i in 1..=10u64 {
        let key = create_session_key(i, 200 + i * 10, 1000 * i);
        let payload = AgentAccountPayload::AddSessionKey { key };
        verify_agent_account_payload(&payload, &owner, &mut state)
            .await
            .expect(&format!("AddSessionKey {} should succeed", i));
    }

    let keys = state
        .get_session_keys_for_account(&owner)
        .await
        .expect("get keys");
    assert_eq!(keys.len(), 10);

    for i in [2u64, 5, 8] {
        let payload = AgentAccountPayload::RevokeSessionKey { key_id: i };
        verify_agent_account_payload(&payload, &owner, &mut state)
            .await
            .expect(&format!("RevokeSessionKey {} should succeed", i));
    }

    let keys = state
        .get_session_keys_for_account(&owner)
        .await
        .expect("get keys");
    assert_eq!(keys.len(), 7);
}

#[tokio::test]
async fn aa_limits_17_max_session_keys_enforced() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let register = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: None,
        session_key_root: None,
    };
    verify_agent_account_payload(&register, &owner, &mut state)
        .await
        .expect("Register should succeed");

    for i in 1..=1024u64 {
        let key = create_session_key(i, 200 + i, 1000);
        let payload = AgentAccountPayload::AddSessionKey { key };
        verify_agent_account_payload(&payload, &owner, &mut state)
            .await
            .expect(&format!("AddSessionKey {} should succeed", i));
    }

    let extra_key = create_session_key(2048, 5000, 1000);
    let payload = AgentAccountPayload::AddSessionKey { key: extra_key };
    let err = verify_agent_account_payload(&payload, &owner, &mut state)
        .await
        .expect_err("Expected max session key limit rejection");
    assert!(
        format!("{:?}", err).contains("AgentAccountInvalidParameter"),
        "Expected AgentAccountInvalidParameter error"
    );
}

#[tokio::test]
async fn aa_limits_18_duplicate_session_key_public_key_rejected() {
    let owner = create_public_key();
    let controller = create_public_key();
    let policy_hash = create_policy_hash();

    let mut state = MockVerificationState::new(100);
    state.register_account(&owner);
    state.register_account(&controller);

    let register = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: None,
        session_key_root: None,
    };
    verify_agent_account_payload(&register, &owner, &mut state)
        .await
        .expect("Register should succeed");

    let key_a = create_session_key(1, 200, 1000);
    let public_key = key_a.public_key.clone();
    let payload = AgentAccountPayload::AddSessionKey { key: key_a };
    verify_agent_account_payload(&payload, &owner, &mut state)
        .await
        .expect("AddSessionKey should succeed");

    let mut key_b = create_session_key(2, 300, 1000);
    key_b.public_key = public_key;
    let payload = AgentAccountPayload::AddSessionKey { key: key_b };
    let err = verify_agent_account_payload(&payload, &owner, &mut state)
        .await
        .expect_err("Expected duplicate public key rejection");
    assert!(
        format!("{:?}", err).contains("AgentAccountSessionKeyExists"),
        "Expected AgentAccountSessionKeyExists error"
    );
}
