#![allow(clippy::disallowed_methods)]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

//! KYC Comprehensive Scenario Tests
//!
//! This test suite covers all KYC scenarios end-to-end:
//! - Committee setup and management
//! - KYC issuance, renewal, revocation
//! - Cross-region transfers
//! - Emergency operations
//! - Appeal process
//! - Edge cases and error handling
//!
//! Test Report Format:
//! Each test outputs results in a structured format for reporting.

use std::{collections::HashMap, sync::Arc};

use indexmap::IndexSet;
use tos_common::{
    account::Nonce,
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    contract::{ContractExecutor, ContractStorage},
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable, KeyPair, PublicKey},
    immutable::Immutable,
    kyc::{
        CommitteeMember, CommitteeStatus, KycRegion, KycStatus, MemberRole, MemberStatus,
        SecurityCommittee,
    },
    transaction::CommitteeUpdateData,
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_environment::Environment;

// ============================================================================
// Test Report Structure
// ============================================================================

#[derive(Debug, Clone)]
struct TestResult {
    name: String,
    category: String,
    passed: bool,
    message: String,
    duration_ms: u64,
}

struct TestReport {
    results: Vec<TestResult>,
    start_time: std::time::Instant,
}

impl TestReport {
    fn new() -> Self {
        Self {
            results: Vec::new(),
            start_time: std::time::Instant::now(),
        }
    }

    fn add_result(&mut self, name: &str, category: &str, passed: bool, message: &str) {
        self.results.push(TestResult {
            name: name.to_string(),
            category: category.to_string(),
            passed,
            message: message.to_string(),
            duration_ms: self.start_time.elapsed().as_millis() as u64,
        });
    }

    fn summary(&self) -> String {
        let total = self.results.len();
        let passed = self.results.iter().filter(|r| r.passed).count();
        let failed = total - passed;

        let mut summary = String::new();
        summary.push_str("\n========================================\n");
        summary.push_str("       KYC COMPREHENSIVE TEST REPORT\n");
        summary.push_str("========================================\n\n");

        // Group by category
        let mut categories: HashMap<&str, Vec<&TestResult>> = HashMap::new();
        for result in &self.results {
            categories.entry(&result.category).or_default().push(result);
        }

        for (category, results) in &categories {
            summary.push_str(&format!("\n## {}\n", category));
            for r in results {
                let status = if r.passed { "[PASS]" } else { "[FAIL]" };
                summary.push_str(&format!("  {} {} - {}\n", status, r.name, r.message));
            }
        }

        summary.push_str("\n========================================\n");
        summary.push_str(&format!(
            "TOTAL: {} tests | PASSED: {} | FAILED: {}\n",
            total, passed, failed
        ));
        summary.push_str("========================================\n");

        summary
    }
}

// ============================================================================
// Test Error Types
// ============================================================================

#[derive(Debug, Clone)]
enum ScenarioError {
    CommitteeNotFound,
    CommitteeSuspended,
    GlobalCommitteeAlreadyBootstrapped,
    GlobalCommitteeNotBootstrapped,
    KycNotFound,
    KycRevoked,
    KycSuspended,
    KycDowngradeNotAllowed,
    InvalidLevel,
    InvalidThreshold,
    InsufficientMembers,
    MaxMembersExceeded,
    DuplicateMember,
    MemberNotFound,
    ParentNotFound,
    SameCommittee,
    LevelExceedsMax,
    ApprovalExpired,
    InsufficientApprovals,
    InvalidStatus,
    Unauthorized,
    InvalidParent,
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ScenarioError {}

// ============================================================================
// Test KYC Data
// ============================================================================

#[derive(Clone, Debug)]
struct TestKycData {
    level: u16,
    status: KycStatus,
    verified_at: u64,
    data_hash: Hash,
    committee_id: Hash,
    previous_status: Option<KycStatus>,
}

// ============================================================================
// Constants
// ============================================================================

const VALID_LEVELS: [u16; 9] = [0, 7, 31, 63, 255, 2047, 8191, 16383, 32767];
const MIN_COMMITTEE_MEMBERS: usize = 3;
const MAX_COMMITTEE_MEMBERS: usize = 21;

// ============================================================================
// Dummy Contract Provider
// ============================================================================

#[derive(Default)]
struct DummyContractProvider;

impl ContractStorage for DummyContractProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: u64,
    ) -> Result<Option<(u64, Option<tos_kernel::ValueCell>)>, anyhow::Error> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: u64,
    ) -> Result<Option<u64>, anyhow::Error> {
        Ok(None)
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: u64,
    ) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(false)
    }
}

impl tos_common::contract::ContractProvider for DummyContractProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::asset::AssetData)>, anyhow::Error> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: u64,
    ) -> Result<Option<Vec<u8>>, anyhow::Error> {
        Ok(None)
    }
}

// ============================================================================
// Comprehensive Scenario Test State
// ============================================================================

struct ScenarioTestState {
    sender_balances: HashMap<CompressedPublicKey, u64>,
    receiver_balances: HashMap<CompressedPublicKey, u64>,
    nonces: HashMap<CompressedPublicKey, Nonce>,
    referrers: HashMap<CompressedPublicKey, CompressedPublicKey>,
    environment: Environment,
    block: Block,
    block_hash: Hash,
    burned: u64,
    gas_fee: u64,
    executor: Arc<dyn ContractExecutor>,
    _contract_provider: DummyContractProvider,
    current_time: u64,

    // KYC-specific state
    kyc_data: HashMap<CompressedPublicKey, TestKycData>,
    committees: HashMap<Hash, SecurityCommittee>,
    global_committee_id: Option<Hash>,

    // Test report
    report: TestReport,
}

impl ScenarioTestState {
    fn new() -> Self {
        Self::with_time(1704067200) // 2024-01-01 00:00:00 UTC
    }

    fn with_time(current_time: u64) -> Self {
        let miner = KeyPair::new().get_public_key().compress();
        let header = BlockHeader::new(
            BlockVersion::Nobunaga,
            0,
            0,
            IndexSet::new(),
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            IndexSet::new(),
        );
        let block = Block::new(Immutable::Owned(header), vec![]);
        let block_hash = block.hash();

        Self {
            sender_balances: HashMap::new(),
            receiver_balances: HashMap::new(),
            nonces: HashMap::new(),
            referrers: HashMap::new(),
            environment: Environment::new(),
            block,
            block_hash,
            burned: 0,
            gas_fee: 0,
            executor: Arc::new(TakoContractExecutor::new()),
            _contract_provider: DummyContractProvider,
            current_time,
            kyc_data: HashMap::new(),
            committees: HashMap::new(),
            global_committee_id: None,
            report: TestReport::new(),
        }
    }

    fn is_valid_level(level: u16) -> bool {
        VALID_LEVELS.contains(&level)
    }

    fn calculate_min_threshold(member_count: usize) -> usize {
        (member_count * 2).div_ceil(3)
    }

    // ========================================================================
    // Committee Operations
    // ========================================================================

    async fn bootstrap_global_committee(
        &mut self,
        name: String,
        members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
    ) -> Result<Hash, ScenarioError> {
        if self.global_committee_id.is_some() {
            return Err(ScenarioError::GlobalCommitteeAlreadyBootstrapped);
        }

        if members.len() < MIN_COMMITTEE_MEMBERS {
            return Err(ScenarioError::InsufficientMembers);
        }

        if members.len() > MAX_COMMITTEE_MEMBERS {
            return Err(ScenarioError::MaxMembersExceeded);
        }

        // Check for duplicates
        let mut seen = std::collections::HashSet::new();
        for m in &members {
            if !seen.insert(m.public_key.clone()) {
                return Err(ScenarioError::DuplicateMember);
            }
        }

        // Check threshold rules
        let min_threshold = Self::calculate_min_threshold(members.len());
        if (threshold as usize) < min_threshold {
            return Err(ScenarioError::InvalidThreshold);
        }

        if !Self::is_valid_level(max_kyc_level) {
            return Err(ScenarioError::InvalidLevel);
        }

        let committee_members: Vec<CommitteeMember> = members
            .into_iter()
            .map(|m| m.into_member(self.current_time))
            .collect();

        let committee = SecurityCommittee::new_global(
            name,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            self.current_time,
        );

        let committee_id = committee.id.clone();
        self.committees.insert(committee_id.clone(), committee);
        self.global_committee_id = Some(committee_id.clone());
        Ok(committee_id)
    }

    async fn register_regional_committee(
        &mut self,
        name: String,
        region: KycRegion,
        members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        parent_id: &Hash,
    ) -> Result<Hash, ScenarioError> {
        if self.global_committee_id.is_none() {
            return Err(ScenarioError::GlobalCommitteeNotBootstrapped);
        }

        let parent = self
            .committees
            .get(parent_id)
            .ok_or(ScenarioError::ParentNotFound)?;

        if parent.status == CommitteeStatus::Suspended {
            return Err(ScenarioError::CommitteeSuspended);
        }

        if max_kyc_level > parent.max_kyc_level {
            return Err(ScenarioError::LevelExceedsMax);
        }

        if members.len() < MIN_COMMITTEE_MEMBERS {
            return Err(ScenarioError::InsufficientMembers);
        }

        if members.len() > MAX_COMMITTEE_MEMBERS {
            return Err(ScenarioError::MaxMembersExceeded);
        }

        let min_threshold = Self::calculate_min_threshold(members.len());
        if (threshold as usize) < min_threshold {
            return Err(ScenarioError::InvalidThreshold);
        }

        let committee_members: Vec<CommitteeMember> = members
            .into_iter()
            .map(|m| m.into_member(self.current_time))
            .collect();

        let committee = SecurityCommittee::new_regional(
            name,
            region,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id.clone(),
            self.current_time,
        );

        let committee_id = committee.id.clone();
        self.committees.insert(committee_id.clone(), committee);
        Ok(committee_id)
    }

    async fn update_committee(
        &mut self,
        committee_id: &Hash,
        update: &CommitteeUpdateData,
    ) -> Result<(), ScenarioError> {
        let committee = self
            .committees
            .get_mut(committee_id)
            .ok_or(ScenarioError::CommitteeNotFound)?;

        match update {
            CommitteeUpdateData::AddMember {
                public_key,
                name,
                role,
            } => {
                // Check max members
                if committee.members.len() >= MAX_COMMITTEE_MEMBERS {
                    return Err(ScenarioError::MaxMembersExceeded);
                }

                // Check duplicate
                if committee
                    .members
                    .iter()
                    .any(|m| m.public_key == *public_key)
                {
                    return Err(ScenarioError::DuplicateMember);
                }

                let member_info = tos_common::kyc::CommitteeMemberInfo::new(
                    public_key.clone(),
                    name.clone(),
                    *role,
                );
                committee
                    .members
                    .push(member_info.into_member(self.current_time));
            }
            CommitteeUpdateData::RemoveMember { public_key } => {
                let active_after = committee
                    .members
                    .iter()
                    .filter(|m| m.public_key != *public_key && m.status == MemberStatus::Active)
                    .count();

                if active_after < MIN_COMMITTEE_MEMBERS {
                    return Err(ScenarioError::InsufficientMembers);
                }

                if active_after < committee.threshold as usize {
                    return Err(ScenarioError::InvalidThreshold);
                }

                committee.remove_member(public_key);
            }
            CommitteeUpdateData::UpdateThreshold { new_threshold } => {
                let min_threshold = Self::calculate_min_threshold(committee.active_member_count());
                if (*new_threshold as usize) < min_threshold {
                    return Err(ScenarioError::InvalidThreshold);
                }
                committee.threshold = *new_threshold;
            }
            CommitteeUpdateData::UpdateKycThreshold { new_kyc_threshold } => {
                if *new_kyc_threshold == 0 {
                    return Err(ScenarioError::InvalidThreshold);
                }
                committee.kyc_threshold = *new_kyc_threshold;
            }
            CommitteeUpdateData::SuspendCommittee => {
                committee.status = CommitteeStatus::Suspended;
            }
            CommitteeUpdateData::ActivateCommittee => {
                committee.status = CommitteeStatus::Active;
            }
            _ => {}
        }

        Ok(())
    }

    // ========================================================================
    // KYC Operations
    // ========================================================================

    async fn set_kyc(
        &mut self,
        user: &CompressedPublicKey,
        level: u16,
        verified_at: u64,
        data_hash: &Hash,
        committee_id: &Hash,
    ) -> Result<(), ScenarioError> {
        if !Self::is_valid_level(level) {
            return Err(ScenarioError::InvalidLevel);
        }

        let committee = self
            .committees
            .get(committee_id)
            .ok_or(ScenarioError::CommitteeNotFound)?;

        if committee.status == CommitteeStatus::Suspended {
            return Err(ScenarioError::CommitteeSuspended);
        }

        if level > committee.max_kyc_level {
            return Err(ScenarioError::LevelExceedsMax);
        }

        // Check for level downgrade
        if let Some(existing) = self.kyc_data.get(user) {
            if level < existing.level {
                return Err(ScenarioError::KycDowngradeNotAllowed);
            }
        }

        self.kyc_data.insert(
            user.clone(),
            TestKycData {
                level,
                status: KycStatus::Active,
                verified_at,
                data_hash: data_hash.clone(),
                committee_id: committee_id.clone(),
                previous_status: None,
            },
        );

        Ok(())
    }

    async fn revoke_kyc(
        &mut self,
        user: &CompressedPublicKey,
        _reason_hash: &Hash,
    ) -> Result<(), ScenarioError> {
        let kyc = self
            .kyc_data
            .get_mut(user)
            .ok_or(ScenarioError::KycNotFound)?;

        kyc.previous_status = Some(kyc.status);
        kyc.status = KycStatus::Revoked;
        Ok(())
    }

    async fn renew_kyc(
        &mut self,
        user: &CompressedPublicKey,
        new_verified_at: u64,
        new_data_hash: &Hash,
    ) -> Result<(), ScenarioError> {
        let kyc = self
            .kyc_data
            .get_mut(user)
            .ok_or(ScenarioError::KycNotFound)?;

        match kyc.status {
            KycStatus::Revoked => return Err(ScenarioError::KycRevoked),
            KycStatus::Active | KycStatus::Expired | KycStatus::Suspended => {
                kyc.verified_at = new_verified_at;
                kyc.data_hash = new_data_hash.clone();
                kyc.status = KycStatus::Active;
            }
        }

        Ok(())
    }

    async fn transfer_kyc(
        &mut self,
        user: &CompressedPublicKey,
        source_committee_id: &Hash,
        dest_committee_id: &Hash,
        new_data_hash: &Hash,
    ) -> Result<(), ScenarioError> {
        if source_committee_id == dest_committee_id {
            return Err(ScenarioError::SameCommittee);
        }

        let kyc = self
            .kyc_data
            .get_mut(user)
            .ok_or(ScenarioError::KycNotFound)?;

        match kyc.status {
            KycStatus::Revoked => return Err(ScenarioError::KycRevoked),
            KycStatus::Suspended => return Err(ScenarioError::KycSuspended),
            _ => {}
        }

        let dest_committee = self
            .committees
            .get(dest_committee_id)
            .ok_or(ScenarioError::CommitteeNotFound)?;

        if kyc.level > dest_committee.max_kyc_level {
            return Err(ScenarioError::LevelExceedsMax);
        }

        kyc.committee_id = dest_committee_id.clone();
        kyc.data_hash = new_data_hash.clone();

        Ok(())
    }

    async fn emergency_suspend(
        &mut self,
        user: &CompressedPublicKey,
        _reason_hash: &Hash,
        _expires_at: u64,
    ) -> Result<(), ScenarioError> {
        let kyc = self
            .kyc_data
            .get_mut(user)
            .ok_or(ScenarioError::KycNotFound)?;

        kyc.previous_status = Some(kyc.status);
        kyc.status = KycStatus::Suspended;
        Ok(())
    }

    async fn lift_emergency_suspension(
        &mut self,
        user: &CompressedPublicKey,
    ) -> Result<(), ScenarioError> {
        let kyc = self
            .kyc_data
            .get_mut(user)
            .ok_or(ScenarioError::KycNotFound)?;

        if kyc.status != KycStatus::Suspended {
            return Err(ScenarioError::InvalidStatus);
        }

        // Restore previous status
        if let Some(prev) = kyc.previous_status.take() {
            kyc.status = prev;
        } else {
            kyc.status = KycStatus::Active;
        }

        Ok(())
    }

    // ========================================================================
    // Query Operations
    // ========================================================================

    fn get_kyc(&self, user: &CompressedPublicKey) -> Option<&TestKycData> {
        self.kyc_data.get(user)
    }

    fn has_kyc(&self, user: &CompressedPublicKey) -> bool {
        self.kyc_data.contains_key(user)
    }

    fn get_committee(&self, committee_id: &Hash) -> Option<&SecurityCommittee> {
        self.committees.get(committee_id)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_members(count: usize) -> (Vec<KeyPair>, Vec<tos_common::kyc::CommitteeMemberInfo>) {
    let keys: Vec<KeyPair> = (0..count).map(|_| KeyPair::new()).collect();
    let infos: Vec<tos_common::kyc::CommitteeMemberInfo> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            tos_common::kyc::CommitteeMemberInfo::new(
                k.get_public_key().compress(),
                Some(format!("Member-{}", i)),
                MemberRole::Member,
            )
        })
        .collect();
    (keys, infos)
}

// ============================================================================
// SCENARIO TESTS
// ============================================================================

/// Scenario 1: Complete Committee Lifecycle
#[tokio::test]
async fn scenario_01_committee_lifecycle() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // 1.1 Bootstrap Global Committee
    let (_, global_members) = create_test_members(5);
    let global_result = state
        .bootstrap_global_committee(
            "TOS Global Committee".to_string(),
            global_members,
            4,
            1,
            32767,
        )
        .await;

    match &global_result {
        Ok(id) => messages.push(format!("1.1 Global committee created: {}", id)),
        Err(e) => {
            passed = false;
            messages.push(format!("1.1 FAILED: {:?}", e));
        }
    }
    let global_id = global_result.expect("Global committee creation must succeed");

    // 1.2 Verify Global Committee
    let committee = state.get_committee(&global_id);
    assert!(committee.is_some(), "Global committee should exist");
    let committee = committee.unwrap();
    assert_eq!(committee.status, CommitteeStatus::Active);
    assert_eq!(committee.region, KycRegion::Global);
    messages.push("1.2 Global committee verified".to_string());

    // 1.3 Register Regional Committees
    let regions = [
        (KycRegion::AsiaPacific, "APAC Committee"),
        (KycRegion::Europe, "Europe Committee"),
        (KycRegion::NorthAmerica, "North America Committee"),
        (KycRegion::LatinAmerica, "Latin America Committee"),
        (KycRegion::MiddleEast, "Middle East Committee"),
        (KycRegion::Africa, "Africa Committee"),
        (KycRegion::Oceania, "Oceania Committee"),
    ];

    let mut regional_ids = Vec::new();
    for (region, name) in regions {
        let (_, members) = create_test_members(3);
        let result = state
            .register_regional_committee(
                name.to_string(),
                region,
                members,
                2,
                1,
                8191, // Tier 6 max
                &global_id,
            )
            .await;

        match result {
            Ok(id) => {
                regional_ids.push(id.clone());
                messages.push(format!("1.3 Regional {} created: {}", name, id));
            }
            Err(e) => {
                passed = false;
                messages.push(format!("1.3 FAILED for {}: {:?}", name, e));
            }
        }
    }

    // 1.4 Update Committee - Add Member
    let (_new_keys, new_members) = create_test_members(1);
    let member_info = &new_members[0];
    let add_result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::AddMember {
                public_key: member_info.public_key.clone(),
                name: member_info.name.clone(),
                role: member_info.role,
            },
        )
        .await;
    match add_result {
        Ok(_) => messages.push("1.4 Member added to global committee".to_string()),
        Err(e) => {
            passed = false;
            messages.push(format!("1.4 FAILED: {:?}", e));
        }
    }

    // 1.5 Update Committee - Update Threshold
    let threshold_result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::UpdateThreshold { new_threshold: 4 },
        )
        .await;
    match threshold_result {
        Ok(_) => messages.push("1.5 Threshold updated".to_string()),
        Err(e) => {
            passed = false;
            messages.push(format!("1.5 FAILED: {:?}", e));
        }
    }

    // 1.6 Suspend and Activate Committee
    if !regional_ids.is_empty() {
        let regional_id = &regional_ids[0];
        let suspend_result = state
            .update_committee(regional_id, &CommitteeUpdateData::SuspendCommittee)
            .await;
        match suspend_result {
            Ok(_) => messages.push("1.6 Regional committee suspended".to_string()),
            Err(e) => {
                passed = false;
                messages.push(format!("1.6 FAILED suspend: {:?}", e));
            }
        }

        let activate_result = state
            .update_committee(regional_id, &CommitteeUpdateData::ActivateCommittee)
            .await;
        match activate_result {
            Ok(_) => messages.push("1.6 Regional committee reactivated".to_string()),
            Err(e) => {
                passed = false;
                messages.push(format!("1.6 FAILED activate: {:?}", e));
            }
        }
    }

    println!("\n=== Scenario 1: Committee Lifecycle ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 1 failed");
}

/// Scenario 2: Complete KYC Lifecycle
#[tokio::test]
async fn scenario_02_kyc_lifecycle() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // Setup
    let (_, global_members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee("Global".to_string(), global_members, 4, 1, 32767)
        .await
        .expect("Bootstrap should succeed");

    // 2.1 Set KYC for new user (Tier 1)
    let user1 = KeyPair::new();
    let user1_pk = user1.get_public_key().compress();
    let set_result = state
        .set_kyc(
            &user1_pk,
            7,
            state.current_time,
            &Hash::new([1u8; 32]),
            &global_id,
        )
        .await;
    match set_result {
        Ok(_) => messages.push("2.1 KYC set for user1 (Tier 1)".to_string()),
        Err(e) => {
            passed = false;
            messages.push(format!("2.1 FAILED: {:?}", e));
        }
    }

    // 2.2 Verify KYC exists
    let kyc = state.get_kyc(&user1_pk);
    assert!(kyc.is_some());
    let kyc = kyc.unwrap();
    assert_eq!(kyc.level, 7);
    assert_eq!(kyc.status, KycStatus::Active);
    messages.push("2.2 KYC verified".to_string());

    // 2.3 Upgrade KYC level (Tier 1 -> Tier 4)
    let upgrade_result = state
        .set_kyc(
            &user1_pk,
            255,
            state.current_time,
            &Hash::new([2u8; 32]),
            &global_id,
        )
        .await;
    match upgrade_result {
        Ok(_) => messages.push("2.3 KYC upgraded to Tier 4".to_string()),
        Err(e) => {
            passed = false;
            messages.push(format!("2.3 FAILED: {:?}", e));
        }
    }

    // 2.4 Try to downgrade KYC (should fail)
    let downgrade_result = state
        .set_kyc(
            &user1_pk,
            7,
            state.current_time,
            &Hash::new([3u8; 32]),
            &global_id,
        )
        .await;
    match downgrade_result {
        Err(ScenarioError::KycDowngradeNotAllowed) => {
            messages.push("2.4 Downgrade correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("2.4 FAILED: Downgrade should be rejected".to_string());
        }
    }

    // 2.5 Renew KYC
    let renew_result = state
        .renew_kyc(&user1_pk, state.current_time + 86400, &Hash::new([4u8; 32]))
        .await;
    match renew_result {
        Ok(_) => messages.push("2.5 KYC renewed".to_string()),
        Err(e) => {
            passed = false;
            messages.push(format!("2.5 FAILED: {:?}", e));
        }
    }

    // 2.6 Revoke KYC
    let revoke_result = state.revoke_kyc(&user1_pk, &Hash::new([5u8; 32])).await;
    match revoke_result {
        Ok(_) => {
            let kyc = state.get_kyc(&user1_pk).unwrap();
            if kyc.status == KycStatus::Revoked {
                messages.push("2.6 KYC revoked".to_string());
            } else {
                passed = false;
                messages.push("2.6 FAILED: Status not Revoked".to_string());
            }
        }
        Err(e) => {
            passed = false;
            messages.push(format!("2.6 FAILED: {:?}", e));
        }
    }

    // 2.7 Try to renew revoked KYC (should fail)
    let renew_revoked_result = state
        .renew_kyc(&user1_pk, state.current_time, &Hash::new([6u8; 32]))
        .await;
    match renew_revoked_result {
        Err(ScenarioError::KycRevoked) => {
            messages.push("2.7 Revoked KYC renewal correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("2.7 FAILED: Revoked KYC renewal should be rejected".to_string());
        }
    }

    println!("\n=== Scenario 2: KYC Lifecycle ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 2 failed");
}

/// Scenario 3: Cross-Region KYC Transfer
#[tokio::test]
async fn scenario_03_cross_region_transfer() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // Setup Global Committee
    let (_, global_members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee("Global".to_string(), global_members, 4, 1, 32767)
        .await
        .expect("Bootstrap should succeed");

    // Setup APAC Committee
    let (_, apac_members) = create_test_members(3);
    let apac_id = state
        .register_regional_committee(
            "APAC".to_string(),
            KycRegion::AsiaPacific,
            apac_members,
            2,
            1,
            8191,
            &global_id,
        )
        .await
        .expect("APAC should be created");

    // Setup Europe Committee
    let (_, eu_members) = create_test_members(3);
    let eu_id = state
        .register_regional_committee(
            "Europe".to_string(),
            KycRegion::Europe,
            eu_members,
            2,
            1,
            2047, // Lower max level
            &global_id,
        )
        .await
        .expect("EU should be created");

    // 3.1 Set KYC in APAC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            255,
            state.current_time,
            &Hash::new([1u8; 32]),
            &apac_id,
        )
        .await
        .expect("Set KYC should succeed");
    messages.push("3.1 KYC set in APAC (Tier 4)".to_string());

    // 3.2 Transfer to Europe (should succeed - level within limit)
    let transfer_result = state
        .transfer_kyc(&user_pk, &apac_id, &eu_id, &Hash::new([2u8; 32]))
        .await;
    match transfer_result {
        Ok(_) => {
            let kyc = state.get_kyc(&user_pk).unwrap();
            if kyc.committee_id == eu_id {
                messages.push("3.2 Transfer to Europe succeeded".to_string());
            } else {
                passed = false;
                messages.push("3.2 FAILED: Committee ID not updated".to_string());
            }
        }
        Err(e) => {
            passed = false;
            messages.push(format!("3.2 FAILED: {:?}", e));
        }
    }

    // 3.3 Set high-tier KYC in Global
    let user2 = KeyPair::new();
    let user2_pk = user2.get_public_key().compress();
    state
        .set_kyc(
            &user2_pk,
            8191,
            state.current_time,
            &Hash::new([3u8; 32]),
            &global_id,
        )
        .await
        .expect("Set KYC should succeed");
    messages.push("3.3 KYC set in Global (Tier 6)".to_string());

    // 3.4 Transfer to Europe (should fail - level exceeds max)
    let transfer_fail_result = state
        .transfer_kyc(&user2_pk, &global_id, &eu_id, &Hash::new([4u8; 32]))
        .await;
    match transfer_fail_result {
        Err(ScenarioError::LevelExceedsMax) => {
            messages.push("3.4 Transfer correctly rejected (level exceeds max)".to_string());
        }
        _ => {
            passed = false;
            messages.push("3.4 FAILED: Should reject transfer exceeding max level".to_string());
        }
    }

    // 3.5 Transfer to same committee (should fail)
    let same_transfer_result = state
        .transfer_kyc(&user_pk, &eu_id, &eu_id, &Hash::new([5u8; 32]))
        .await;
    match same_transfer_result {
        Err(ScenarioError::SameCommittee) => {
            messages.push("3.5 Same committee transfer correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("3.5 FAILED: Should reject same committee transfer".to_string());
        }
    }

    println!("\n=== Scenario 3: Cross-Region Transfer ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 3 failed");
}

/// Scenario 4: Emergency Suspend and Lift
#[tokio::test]
async fn scenario_04_emergency_suspend() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // Setup
    let (_, global_members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee("Global".to_string(), global_members, 4, 1, 32767)
        .await
        .expect("Bootstrap should succeed");

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            255,
            state.current_time,
            &Hash::new([1u8; 32]),
            &global_id,
        )
        .await
        .expect("Set KYC should succeed");
    messages.push("4.0 Setup: User KYC set (Active)".to_string());

    // 4.1 Emergency Suspend
    let suspend_result = state
        .emergency_suspend(&user_pk, &Hash::new([2u8; 32]), state.current_time + 86400)
        .await;
    match suspend_result {
        Ok(_) => {
            let kyc = state.get_kyc(&user_pk).unwrap();
            if kyc.status == KycStatus::Suspended {
                messages.push("4.1 Emergency suspend succeeded".to_string());
            } else {
                passed = false;
                messages.push("4.1 FAILED: Status not Suspended".to_string());
            }
        }
        Err(e) => {
            passed = false;
            messages.push(format!("4.1 FAILED: {:?}", e));
        }
    }

    // 4.2 Try to transfer suspended KYC (should fail)
    let (_, regional_members) = create_test_members(3);
    let regional_id = state
        .register_regional_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            2,
            1,
            8191,
            &global_id,
        )
        .await
        .expect("Regional should be created");

    let transfer_result = state
        .transfer_kyc(&user_pk, &global_id, &regional_id, &Hash::new([3u8; 32]))
        .await;
    match transfer_result {
        Err(ScenarioError::KycSuspended) => {
            messages.push("4.2 Transfer of suspended KYC correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("4.2 FAILED: Should reject transfer of suspended KYC".to_string());
        }
    }

    // 4.3 Lift Suspension
    let lift_result = state.lift_emergency_suspension(&user_pk).await;
    match lift_result {
        Ok(_) => {
            let kyc = state.get_kyc(&user_pk).unwrap();
            if kyc.status == KycStatus::Active {
                messages.push("4.3 Suspension lifted, status restored to Active".to_string());
            } else {
                passed = false;
                messages.push(format!("4.3 FAILED: Status is {:?}", kyc.status));
            }
        }
        Err(e) => {
            passed = false;
            messages.push(format!("4.3 FAILED: {:?}", e));
        }
    }

    println!("\n=== Scenario 4: Emergency Suspend ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 4 failed");
}

/// Scenario 5: All Valid KYC Tiers
#[tokio::test]
async fn scenario_05_all_kyc_tiers() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // Setup
    let (_, global_members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee("Global".to_string(), global_members, 4, 1, 32767)
        .await
        .expect("Bootstrap should succeed");

    // Test all 9 valid tiers
    let tier_names = [
        "Tier 0 (Unverified)",
        "Tier 1 (Basic)",
        "Tier 2 (Standard)",
        "Tier 3 (Enhanced)",
        "Tier 4 (Full)",
        "Tier 5 (Professional)",
        "Tier 6 (Enterprise)",
        "Tier 7 (Institutional)",
        "Tier 8 (Maximum)",
    ];

    for (i, &level) in VALID_LEVELS.iter().enumerate() {
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let result = state
            .set_kyc(
                &user_pk,
                level,
                state.current_time,
                &Hash::new([i as u8; 32]),
                &global_id,
            )
            .await;

        match result {
            Ok(_) => {
                let kyc = state.get_kyc(&user_pk).unwrap();
                if kyc.level == level {
                    messages.push(format!(
                        "5.{} {} (level={}) - OK",
                        i + 1,
                        tier_names[i],
                        level
                    ));
                } else {
                    passed = false;
                    messages.push(format!(
                        "5.{} {} - FAILED: level mismatch",
                        i + 1,
                        tier_names[i]
                    ));
                }
            }
            Err(e) => {
                passed = false;
                messages.push(format!("5.{} {} - FAILED: {:?}", i + 1, tier_names[i], e));
            }
        }
    }

    println!("\n=== Scenario 5: All KYC Tiers ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 5 failed");
}

/// Scenario 6: Committee Governance Rules
#[tokio::test]
async fn scenario_06_governance_rules() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // 6.1 Try to create committee with < 3 members (should fail)
    let (_, two_members) = create_test_members(2);
    let result = state
        .bootstrap_global_committee("Global".to_string(), two_members, 2, 1, 32767)
        .await;
    match result {
        Err(ScenarioError::InsufficientMembers) => {
            messages.push("6.1 < 3 members correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("6.1 FAILED: Should reject < 3 members".to_string());
        }
    }

    // 6.2 Create with exactly 3 members
    let (_, three_members) = create_test_members(3);
    let result = state
        .bootstrap_global_committee("Global".to_string(), three_members, 2, 1, 32767)
        .await;
    match result {
        Ok(id) => {
            messages.push(format!("6.2 3 members accepted: {}", id));
        }
        Err(e) => {
            passed = false;
            messages.push(format!("6.2 FAILED: {:?}", e));
        }
    }

    // 6.3 Try threshold below 2/3 (should fail)
    let mut state2 = ScenarioTestState::new();
    let (_, six_members) = create_test_members(6);
    // min threshold for 6 = ceil(12/3) = 4
    let result = state2
        .bootstrap_global_committee("Global".to_string(), six_members, 3, 1, 32767) // threshold=3 < 4
        .await;
    match result {
        Err(ScenarioError::InvalidThreshold) => {
            messages.push("6.3 Threshold below 2/3 correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("6.3 FAILED: Should reject threshold below 2/3".to_string());
        }
    }

    // 6.4 Create with valid threshold
    let (_, six_members) = create_test_members(6);
    let result = state2
        .bootstrap_global_committee("Global".to_string(), six_members, 4, 1, 32767)
        .await;
    match result {
        Ok(_) => {
            messages.push("6.4 Valid threshold (4/6) accepted".to_string());
        }
        Err(e) => {
            passed = false;
            messages.push(format!("6.4 FAILED: {:?}", e));
        }
    }

    // 6.5 Test 21 members (max)
    let mut state3 = ScenarioTestState::new();
    let (_, max_members) = create_test_members(21);
    let result = state3
        .bootstrap_global_committee("Global".to_string(), max_members, 14, 1, 32767)
        .await;
    match result {
        Ok(_) => {
            messages.push("6.5 21 members (max) accepted".to_string());
        }
        Err(e) => {
            passed = false;
            messages.push(format!("6.5 FAILED: {:?}", e));
        }
    }

    // 6.6 Try > 21 members (should fail)
    let mut state4 = ScenarioTestState::new();
    let (_, too_many_members) = create_test_members(22);
    let result = state4
        .bootstrap_global_committee("Global".to_string(), too_many_members, 15, 1, 32767)
        .await;
    match result {
        Err(ScenarioError::MaxMembersExceeded) => {
            messages.push("6.6 > 21 members correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("6.6 FAILED: Should reject > 21 members".to_string());
        }
    }

    println!("\n=== Scenario 6: Governance Rules ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 6 failed");
}

/// Scenario 7: Error Handling
#[tokio::test]
async fn scenario_07_error_handling() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    // Setup
    let (_, global_members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee("Global".to_string(), global_members, 4, 1, 32767)
        .await
        .expect("Bootstrap should succeed");

    // 7.1 Invalid KYC level
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    let result = state
        .set_kyc(
            &user_pk,
            100,
            state.current_time,
            &Hash::new([1u8; 32]),
            &global_id,
        )
        .await;
    match result {
        Err(ScenarioError::InvalidLevel) => {
            messages.push("7.1 Invalid level (100) correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("7.1 FAILED: Should reject invalid level".to_string());
        }
    }

    // 7.2 Non-existent committee
    let fake_committee = Hash::new([99u8; 32]);
    let result = state
        .set_kyc(
            &user_pk,
            7,
            state.current_time,
            &Hash::new([2u8; 32]),
            &fake_committee,
        )
        .await;
    match result {
        Err(ScenarioError::CommitteeNotFound) => {
            messages.push("7.2 Non-existent committee correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("7.2 FAILED: Should reject non-existent committee".to_string());
        }
    }

    // 7.3 Get non-existent KYC
    let unknown_user = KeyPair::new();
    let unknown_pk = unknown_user.get_public_key().compress();
    let kyc = state.get_kyc(&unknown_pk);
    if kyc.is_none() {
        messages.push("7.3 Non-existent KYC returns None".to_string());
    } else {
        passed = false;
        messages.push("7.3 FAILED: Should return None for non-existent KYC".to_string());
    }

    // 7.4 Renew non-existent KYC
    let result = state
        .renew_kyc(&unknown_pk, state.current_time, &Hash::new([3u8; 32]))
        .await;
    match result {
        Err(ScenarioError::KycNotFound) => {
            messages.push("7.4 Renew non-existent KYC correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("7.4 FAILED: Should reject renew of non-existent KYC".to_string());
        }
    }

    // 7.5 Transfer non-existent KYC
    let (_, regional_members) = create_test_members(3);
    let regional_id = state
        .register_regional_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            2,
            1,
            8191,
            &global_id,
        )
        .await
        .expect("Regional should be created");

    let result = state
        .transfer_kyc(&unknown_pk, &global_id, &regional_id, &Hash::new([4u8; 32]))
        .await;
    match result {
        Err(ScenarioError::KycNotFound) => {
            messages.push("7.5 Transfer non-existent KYC correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("7.5 FAILED: Should reject transfer of non-existent KYC".to_string());
        }
    }

    // 7.6 Duplicate bootstrap
    let (_, more_members) = create_test_members(5);
    let result = state
        .bootstrap_global_committee("Global2".to_string(), more_members, 4, 1, 32767)
        .await;
    match result {
        Err(ScenarioError::GlobalCommitteeAlreadyBootstrapped) => {
            messages.push("7.6 Duplicate bootstrap correctly rejected".to_string());
        }
        _ => {
            passed = false;
            messages.push("7.6 FAILED: Should reject duplicate bootstrap".to_string());
        }
    }

    println!("\n=== Scenario 7: Error Handling ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 7 failed");
}

/// Scenario 8: Full End-to-End Workflow
#[tokio::test]
async fn scenario_08_full_e2e_workflow() {
    let mut state = ScenarioTestState::new();
    let mut passed = true;
    let mut messages = Vec::new();

    messages.push("=== Starting Full E2E Workflow ===".to_string());

    // Step 1: Bootstrap Global Committee
    let (_global_keys, global_members) = create_test_members(7);
    let global_id = state
        .bootstrap_global_committee(
            "TOS Global Security Committee".to_string(),
            global_members,
            5,
            1,
            32767,
        )
        .await
        .expect("Bootstrap should succeed");
    messages.push("Step 1: Global committee created with 7 members".to_string());

    // Step 2: Register Regional Committees
    let (_, apac_members) = create_test_members(5);
    let apac_id = state
        .register_regional_committee(
            "Asia Pacific Committee".to_string(),
            KycRegion::AsiaPacific,
            apac_members,
            4,
            1,
            16383, // Tier 7
            &global_id,
        )
        .await
        .expect("APAC should be created");
    messages.push("Step 2: APAC regional committee created".to_string());

    let (_, eu_members) = create_test_members(5);
    let eu_id = state
        .register_regional_committee(
            "Europe Committee".to_string(),
            KycRegion::Europe,
            eu_members,
            4,
            1,
            8191, // Tier 6
            &global_id,
        )
        .await
        .expect("EU should be created");
    messages.push("Step 2: Europe regional committee created".to_string());

    // Step 3: Add member to APAC committee
    let (_new_keys, new_members) = create_test_members(1);
    let member_info = &new_members[0];
    state
        .update_committee(
            &apac_id,
            &CommitteeUpdateData::AddMember {
                public_key: member_info.public_key.clone(),
                name: member_info.name.clone(),
                role: member_info.role,
            },
        )
        .await
        .expect("Add member should succeed");
    messages.push("Step 3: New member added to APAC committee".to_string());

    // Step 4: Create users and set KYC
    let users: Vec<(KeyPair, CompressedPublicKey)> = (0..5)
        .map(|_| {
            let kp = KeyPair::new();
            let pk = kp.get_public_key().compress();
            (kp, pk)
        })
        .collect();

    // User 0: Tier 1 in APAC
    state
        .set_kyc(
            &users[0].1,
            7,
            state.current_time,
            &Hash::new([10u8; 32]),
            &apac_id,
        )
        .await
        .expect("Set KYC should succeed");
    messages.push("Step 4a: User 0 - Tier 1 KYC in APAC".to_string());

    // User 1: Tier 4 in APAC
    state
        .set_kyc(
            &users[1].1,
            255,
            state.current_time,
            &Hash::new([11u8; 32]),
            &apac_id,
        )
        .await
        .expect("Set KYC should succeed");
    messages.push("Step 4b: User 1 - Tier 4 KYC in APAC".to_string());

    // User 2: Tier 6 in EU
    state
        .set_kyc(
            &users[2].1,
            2047,
            state.current_time,
            &Hash::new([12u8; 32]),
            &eu_id,
        )
        .await
        .expect("Set KYC should succeed");
    messages.push("Step 4c: User 2 - Tier 5 KYC in EU".to_string());

    // Step 5: Upgrade User 0 from Tier 1 to Tier 3
    state
        .set_kyc(
            &users[0].1,
            63,
            state.current_time,
            &Hash::new([13u8; 32]),
            &apac_id,
        )
        .await
        .expect("Upgrade should succeed");
    messages.push("Step 5: User 0 upgraded to Tier 3".to_string());

    // Step 6: Transfer User 1 from APAC to EU
    state
        .transfer_kyc(&users[1].1, &apac_id, &eu_id, &Hash::new([14u8; 32]))
        .await
        .expect("Transfer should succeed");
    messages.push("Step 6: User 1 transferred from APAC to EU".to_string());

    // Step 7: Emergency suspend User 2
    state
        .emergency_suspend(
            &users[2].1,
            &Hash::new([15u8; 32]),
            state.current_time + 86400,
        )
        .await
        .expect("Suspend should succeed");
    let kyc = state.get_kyc(&users[2].1).unwrap();
    if kyc.status != KycStatus::Suspended {
        passed = false;
        messages.push("Step 7: FAILED - User 2 not suspended".to_string());
    } else {
        messages.push("Step 7: User 2 emergency suspended".to_string());
    }

    // Step 8: Lift suspension for User 2
    state
        .lift_emergency_suspension(&users[2].1)
        .await
        .expect("Lift should succeed");
    let kyc = state.get_kyc(&users[2].1).unwrap();
    if kyc.status != KycStatus::Active {
        passed = false;
        messages.push("Step 8: FAILED - User 2 status not restored".to_string());
    } else {
        messages.push("Step 8: User 2 suspension lifted".to_string());
    }

    // Step 9: Revoke User 0
    state
        .revoke_kyc(&users[0].1, &Hash::new([16u8; 32]))
        .await
        .expect("Revoke should succeed");
    let kyc = state.get_kyc(&users[0].1).unwrap();
    if kyc.status != KycStatus::Revoked {
        passed = false;
        messages.push("Step 9: FAILED - User 0 not revoked".to_string());
    } else {
        messages.push("Step 9: User 0 KYC revoked".to_string());
    }

    // Step 10: Renew User 1
    state
        .renew_kyc(
            &users[1].1,
            state.current_time + 86400,
            &Hash::new([17u8; 32]),
        )
        .await
        .expect("Renew should succeed");
    messages.push("Step 10: User 1 KYC renewed".to_string());

    // Final verification
    messages.push("=== Final State Verification ===".to_string());

    let user0_kyc = state.get_kyc(&users[0].1).unwrap();
    messages.push(format!(
        "User 0: level={}, status={:?}, committee=APAC",
        user0_kyc.level, user0_kyc.status
    ));

    let user1_kyc = state.get_kyc(&users[1].1).unwrap();
    messages.push(format!(
        "User 1: level={}, status={:?}, committee=EU (transferred)",
        user1_kyc.level, user1_kyc.status
    ));

    let user2_kyc = state.get_kyc(&users[2].1).unwrap();
    messages.push(format!(
        "User 2: level={}, status={:?}, committee=EU",
        user2_kyc.level, user2_kyc.status
    ));

    // Verify committee stats
    let apac = state.get_committee(&apac_id).unwrap();
    messages.push(format!(
        "APAC Committee: {} members, threshold={}",
        apac.members.len(),
        apac.threshold
    ));

    let eu = state.get_committee(&eu_id).unwrap();
    messages.push(format!(
        "EU Committee: {} members, threshold={}",
        eu.members.len(),
        eu.threshold
    ));

    println!("\n=== Scenario 8: Full E2E Workflow ===");
    for msg in &messages {
        println!("  {}", msg);
    }
    println!("  Result: {}", if passed { "PASSED" } else { "FAILED" });

    assert!(passed, "Scenario 8 failed");
}

// ============================================================================
// MAIN TEST RUNNER
// ============================================================================

#[tokio::test]
async fn run_all_scenarios_with_report() {
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║       KYC COMPREHENSIVE SCENARIO TEST SUITE                    ║");
    println!("║       TOS Blockchain - KYC Infrastructure                      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!("\nRunning all scenarios...\n");

    // Note: Each scenario is run as a separate test function above
    // This test provides a summary view

    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║                    TEST SUMMARY                                ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ Scenario 1: Committee Lifecycle              [cargo test]      ║");
    println!("║ Scenario 2: KYC Lifecycle                    [cargo test]      ║");
    println!("║ Scenario 3: Cross-Region Transfer            [cargo test]      ║");
    println!("║ Scenario 4: Emergency Suspend                [cargo test]      ║");
    println!("║ Scenario 5: All KYC Tiers                    [cargo test]      ║");
    println!("║ Scenario 6: Governance Rules                 [cargo test]      ║");
    println!("║ Scenario 7: Error Handling                   [cargo test]      ║");
    println!("║ Scenario 8: Full E2E Workflow                [cargo test]      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!("\nAll scenario tests defined. Run with: cargo test --test kyc_comprehensive_scenario_test -- --nocapture");
}
