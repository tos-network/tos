use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, ContractId},
        ContractAssetExtProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight,
    contract_asset::{
        AdminDelay, AgentAuthorization, Allowance, BalanceCheckpoint, Checkpoint,
        ContractAssetData, Delegation, DelegationCheckpoint, Escrow, FreezeState, PauseState,
        RoleConfig, SupplyCheckpoint, TimelockOperation, TokenKey, TokenLock, TokenValue,
    },
    crypto::Hash,
    serializer::{Reader, Serializer},
    versioned_type::Versioned,
};

const TAG_ASSET: u8 = 0x00;
const TAG_BALANCE: u8 = 0x01;
const TAG_SUPPLY: u8 = 0x02;
const TAG_ALLOWANCE: u8 = 0x03;
const TAG_LOCK: u8 = 0x04;
const TAG_LOCK_COUNT: u8 = 0x05;
const TAG_LOCKED_BALANCE: u8 = 0x06;
const TAG_NEXT_LOCK_ID: u8 = 0x07;
const TAG_LOCK_IDS: u8 = 0x08;
const TAG_FREEZE_STATE: u8 = 0x09;
const TAG_PAUSE_STATE: u8 = 0x0A;
const TAG_PERMIT_NONCE: u8 = 0x0B;
const TAG_DELEGATION: u8 = 0x0C;
const TAG_VOTE_POWER: u8 = 0x0D;
const TAG_CHECKPOINT_COUNT: u8 = 0x0E;
const TAG_BALANCE_CHECKPOINT_COUNT: u8 = 0x0F;
const TAG_DELEGATION_CHECKPOINT_COUNT: u8 = 0x10;
const TAG_CHECKPOINT: u8 = 0x11;
const TAG_BALANCE_CHECKPOINT: u8 = 0x12;
const TAG_DELEGATION_CHECKPOINT: u8 = 0x13;
const TAG_SUPPLY_CHECKPOINT: u8 = 0x14;
const TAG_SUPPLY_CHECKPOINT_COUNT: u8 = 0x15;
const TAG_ROLE_CONFIG: u8 = 0x16;
const TAG_ROLE_MEMBER: u8 = 0x17;
const TAG_ROLE_MEMBERS: u8 = 0x18;
const TAG_ESCROW: u8 = 0x19;
const TAG_ESCROW_COUNTER: u8 = 0x1A;
const TAG_USER_ESCROWS: u8 = 0x1B;
const TAG_AGENT_AUTH: u8 = 0x1C;
const TAG_OWNER_AGENTS: u8 = 0x1D;
const TAG_DELEGATORS: u8 = 0x1E;
const TAG_PENDING_ADMIN: u8 = 0x1F;
const TAG_ADMIN_DELAY: u8 = 0x20;
const TAG_TIMELOCK_MIN_DELAY: u8 = 0x21;
const TAG_TIMELOCK_OPERATION: u8 = 0x22;
const TAG_METADATA_URI: u8 = 0x23;

fn asset_from_key(key: &TokenKey) -> &Hash {
    match key {
        TokenKey::Asset(asset)
        | TokenKey::Supply(asset)
        | TokenKey::PauseState(asset)
        | TokenKey::EscrowCounter(asset)
        | TokenKey::PendingAdmin(asset)
        | TokenKey::SupplyCheckpointCount(asset)
        | TokenKey::AdminDelay(asset)
        | TokenKey::TimelockMinDelay(asset)
        | TokenKey::MetadataUri(asset)
        | TokenKey::Balance { asset, .. }
        | TokenKey::Allowance { asset, .. }
        | TokenKey::Lock { asset, .. }
        | TokenKey::LockCount { asset, .. }
        | TokenKey::LockedBalance { asset, .. }
        | TokenKey::NextLockId { asset, .. }
        | TokenKey::LockIds { asset, .. }
        | TokenKey::FreezeState { asset, .. }
        | TokenKey::PermitNonce { asset, .. }
        | TokenKey::Delegation { asset, .. }
        | TokenKey::VotePower { asset, .. }
        | TokenKey::CheckpointCount { asset, .. }
        | TokenKey::BalanceCheckpointCount { asset, .. }
        | TokenKey::DelegationCheckpointCount { asset, .. }
        | TokenKey::Checkpoint { asset, .. }
        | TokenKey::BalanceCheckpoint { asset, .. }
        | TokenKey::DelegationCheckpoint { asset, .. }
        | TokenKey::SupplyCheckpoint { asset, .. }
        | TokenKey::RoleConfig { asset, .. }
        | TokenKey::RoleMember { asset, .. }
        | TokenKey::RoleMembers { asset, .. }
        | TokenKey::Escrow { asset, .. }
        | TokenKey::UserEscrows { asset, .. }
        | TokenKey::AgentAuth { asset, .. }
        | TokenKey::OwnerAgents { asset, .. }
        | TokenKey::Delegators { asset, .. }
        | TokenKey::TimelockOperation { asset, .. } => asset,
    }
}

fn encode_subkey(key: &TokenKey) -> Result<Vec<u8>, BlockchainError> {
    let mut out = Vec::with_capacity(1 + 32 + 32);
    match key {
        TokenKey::Asset(_) => out.push(TAG_ASSET),
        TokenKey::Supply(_) => out.push(TAG_SUPPLY),
        TokenKey::PauseState(_) => out.push(TAG_PAUSE_STATE),
        TokenKey::EscrowCounter(_) => out.push(TAG_ESCROW_COUNTER),
        TokenKey::PendingAdmin(_) => out.push(TAG_PENDING_ADMIN),
        TokenKey::SupplyCheckpointCount(_) => out.push(TAG_SUPPLY_CHECKPOINT_COUNT),
        TokenKey::AdminDelay(_) => out.push(TAG_ADMIN_DELAY),
        TokenKey::TimelockMinDelay(_) => out.push(TAG_TIMELOCK_MIN_DELAY),
        TokenKey::MetadataUri(_) => out.push(TAG_METADATA_URI),
        TokenKey::Balance { account, .. } => {
            out.push(TAG_BALANCE);
            out.extend_from_slice(account);
        }
        TokenKey::Allowance { owner, spender, .. } => {
            out.push(TAG_ALLOWANCE);
            out.extend_from_slice(owner);
            out.extend_from_slice(spender);
        }
        TokenKey::Lock {
            account, lock_id, ..
        } => {
            out.push(TAG_LOCK);
            out.extend_from_slice(account);
            out.extend_from_slice(&lock_id.to_be_bytes());
        }
        TokenKey::LockCount { account, .. } => {
            out.push(TAG_LOCK_COUNT);
            out.extend_from_slice(account);
        }
        TokenKey::LockedBalance { account, .. } => {
            out.push(TAG_LOCKED_BALANCE);
            out.extend_from_slice(account);
        }
        TokenKey::NextLockId { account, .. } => {
            out.push(TAG_NEXT_LOCK_ID);
            out.extend_from_slice(account);
        }
        TokenKey::LockIds { account, .. } => {
            out.push(TAG_LOCK_IDS);
            out.extend_from_slice(account);
        }
        TokenKey::FreezeState { account, .. } => {
            out.push(TAG_FREEZE_STATE);
            out.extend_from_slice(account);
        }
        TokenKey::PermitNonce { account, .. } => {
            out.push(TAG_PERMIT_NONCE);
            out.extend_from_slice(account);
        }
        TokenKey::Delegation { account, .. } => {
            out.push(TAG_DELEGATION);
            out.extend_from_slice(account);
        }
        TokenKey::VotePower { account, .. } => {
            out.push(TAG_VOTE_POWER);
            out.extend_from_slice(account);
        }
        TokenKey::CheckpointCount { account, .. } => {
            out.push(TAG_CHECKPOINT_COUNT);
            out.extend_from_slice(account);
        }
        TokenKey::BalanceCheckpointCount { account, .. } => {
            out.push(TAG_BALANCE_CHECKPOINT_COUNT);
            out.extend_from_slice(account);
        }
        TokenKey::DelegationCheckpointCount { account, .. } => {
            out.push(TAG_DELEGATION_CHECKPOINT_COUNT);
            out.extend_from_slice(account);
        }
        TokenKey::Checkpoint { account, index, .. } => {
            out.push(TAG_CHECKPOINT);
            out.extend_from_slice(account);
            out.extend_from_slice(&index.to_be_bytes());
        }
        TokenKey::BalanceCheckpoint { account, index, .. } => {
            out.push(TAG_BALANCE_CHECKPOINT);
            out.extend_from_slice(account);
            out.extend_from_slice(&index.to_be_bytes());
        }
        TokenKey::DelegationCheckpoint { account, index, .. } => {
            out.push(TAG_DELEGATION_CHECKPOINT);
            out.extend_from_slice(account);
            out.extend_from_slice(&index.to_be_bytes());
        }
        TokenKey::SupplyCheckpoint { index, .. } => {
            out.push(TAG_SUPPLY_CHECKPOINT);
            out.extend_from_slice(&index.to_be_bytes());
        }
        TokenKey::RoleConfig { role, .. } => {
            out.push(TAG_ROLE_CONFIG);
            out.extend_from_slice(role);
        }
        TokenKey::RoleMember { role, account, .. } => {
            out.push(TAG_ROLE_MEMBER);
            out.extend_from_slice(role);
            out.extend_from_slice(account);
        }
        TokenKey::RoleMembers { role, .. } => {
            out.push(TAG_ROLE_MEMBERS);
            out.extend_from_slice(role);
        }
        TokenKey::Escrow { escrow_id, .. } => {
            out.push(TAG_ESCROW);
            out.extend_from_slice(&escrow_id.to_be_bytes());
        }
        TokenKey::UserEscrows { user, .. } => {
            out.push(TAG_USER_ESCROWS);
            out.extend_from_slice(user);
        }
        TokenKey::AgentAuth { owner, agent, .. } => {
            out.push(TAG_AGENT_AUTH);
            out.extend_from_slice(owner);
            out.extend_from_slice(agent);
        }
        TokenKey::OwnerAgents { owner, .. } => {
            out.push(TAG_OWNER_AGENTS);
            out.extend_from_slice(owner);
        }
        TokenKey::Delegators { delegatee, .. } => {
            out.push(TAG_DELEGATORS);
            out.extend_from_slice(delegatee);
        }
        TokenKey::TimelockOperation { operation_id, .. } => {
            out.push(TAG_TIMELOCK_OPERATION);
            out.extend_from_slice(operation_id);
        }
    }

    Ok(out)
}

fn encode_value(key: &TokenKey, value: &TokenValue) -> Result<Vec<u8>, BlockchainError> {
    let bytes = match (key, value) {
        (TokenKey::Asset(_), TokenValue::Asset(data)) => data.to_bytes(),
        (TokenKey::Balance { .. }, TokenValue::Balance(balance)) => balance.to_bytes(),
        (TokenKey::Supply(_), TokenValue::Supply(supply)) => supply.to_bytes(),
        (TokenKey::Allowance { .. }, TokenValue::Allowance(allowance)) => allowance.to_bytes(),
        (TokenKey::Lock { .. }, TokenValue::Lock(lock)) => lock.to_bytes(),
        (TokenKey::LockCount { .. }, TokenValue::LockCount(count)) => count.to_bytes(),
        (TokenKey::LockedBalance { .. }, TokenValue::LockedBalance(value)) => value.to_bytes(),
        (TokenKey::NextLockId { .. }, TokenValue::NextLockId(value)) => value.to_bytes(),
        (TokenKey::LockIds { .. }, TokenValue::LockIds(ids)) => ids.to_bytes(),
        (TokenKey::FreezeState { .. }, TokenValue::FreezeState(state)) => state.to_bytes(),
        (TokenKey::PauseState(_), TokenValue::PauseState(state)) => state.to_bytes(),
        (TokenKey::PermitNonce { .. }, TokenValue::PermitNonce(nonce)) => nonce.to_bytes(),
        (TokenKey::Delegation { .. }, TokenValue::Delegation(delegation)) => delegation.to_bytes(),
        (TokenKey::VotePower { .. }, TokenValue::VotePower(votes)) => votes.to_bytes(),
        (TokenKey::CheckpointCount { .. }, TokenValue::CheckpointCount(count)) => count.to_bytes(),
        (TokenKey::BalanceCheckpointCount { .. }, TokenValue::BalanceCheckpointCount(count)) => {
            count.to_bytes()
        }
        (
            TokenKey::DelegationCheckpointCount { .. },
            TokenValue::DelegationCheckpointCount(count),
        ) => count.to_bytes(),
        (TokenKey::Checkpoint { .. }, TokenValue::Checkpoint(checkpoint)) => checkpoint.to_bytes(),
        (TokenKey::BalanceCheckpoint { .. }, TokenValue::BalanceCheckpoint(checkpoint)) => {
            checkpoint.to_bytes()
        }
        (TokenKey::DelegationCheckpoint { .. }, TokenValue::DelegationCheckpoint(checkpoint)) => {
            checkpoint.to_bytes()
        }
        (TokenKey::SupplyCheckpoint { .. }, TokenValue::SupplyCheckpoint(checkpoint)) => {
            checkpoint.to_bytes()
        }
        (TokenKey::SupplyCheckpointCount(_), TokenValue::SupplyCheckpointCount(count)) => {
            count.to_bytes()
        }
        (TokenKey::RoleConfig { .. }, TokenValue::RoleConfig(config)) => config.to_bytes(),
        (TokenKey::RoleMember { .. }, TokenValue::RoleMemberGrantedAt(granted_at)) => {
            granted_at.to_bytes()
        }
        (TokenKey::RoleMembers { .. }, TokenValue::RoleMembers(members)) => members.to_bytes(),
        (TokenKey::Escrow { .. }, TokenValue::Escrow(escrow)) => escrow.to_bytes(),
        (TokenKey::EscrowCounter(_), TokenValue::EscrowCounter(counter)) => counter.to_bytes(),
        (TokenKey::UserEscrows { .. }, TokenValue::UserEscrows(escrows)) => escrows.to_bytes(),
        (TokenKey::AgentAuth { .. }, TokenValue::AgentAuth(auth)) => auth.to_bytes(),
        (TokenKey::OwnerAgents { .. }, TokenValue::OwnerAgents(agents)) => agents.to_bytes(),
        (TokenKey::Delegators { .. }, TokenValue::Delegators(delegators)) => delegators.to_bytes(),
        (TokenKey::PendingAdmin(_), TokenValue::PendingAdmin(admin)) => admin.to_bytes(),
        (TokenKey::AdminDelay(_), TokenValue::AdminDelay(delay)) => delay.to_bytes(),
        (TokenKey::TimelockMinDelay(_), TokenValue::TimelockMinDelay(delay)) => delay.to_bytes(),
        (TokenKey::TimelockOperation { .. }, TokenValue::TimelockOperation(operation)) => {
            operation.to_bytes()
        }
        (TokenKey::TimelockOperation { .. }, TokenValue::TimelockOperationOpt(Some(operation))) => {
            operation.to_bytes()
        }
        (TokenKey::MetadataUri(_), TokenValue::MetadataUri(uri)) => uri.to_bytes(),
        (_, TokenValue::Deleted) => Vec::new(),
        _ => return Err(BlockchainError::UnsupportedOperation),
    };

    Ok(bytes)
}

fn decode_value(key: &TokenKey, bytes: &[u8]) -> Result<TokenValue, BlockchainError> {
    if bytes.is_empty() {
        return Ok(TokenValue::Deleted);
    }
    let mut reader = Reader::new(bytes);
    let value = match key {
        TokenKey::Asset(_) => TokenValue::Asset(ContractAssetData::read(&mut reader)?),
        TokenKey::Balance { .. } => TokenValue::Balance(u64::read(&mut reader)?),
        TokenKey::Supply(_) => TokenValue::Supply(u64::read(&mut reader)?),
        TokenKey::Allowance { .. } => TokenValue::Allowance(Allowance::read(&mut reader)?),
        TokenKey::Lock { .. } => TokenValue::Lock(TokenLock::read(&mut reader)?),
        TokenKey::LockCount { .. } => TokenValue::LockCount(u32::read(&mut reader)?),
        TokenKey::LockedBalance { .. } => TokenValue::LockedBalance(u64::read(&mut reader)?),
        TokenKey::NextLockId { .. } => TokenValue::NextLockId(u64::read(&mut reader)?),
        TokenKey::LockIds { .. } => TokenValue::LockIds(Vec::<u64>::read(&mut reader)?),
        TokenKey::FreezeState { .. } => TokenValue::FreezeState(FreezeState::read(&mut reader)?),
        TokenKey::PauseState(_) => TokenValue::PauseState(PauseState::read(&mut reader)?),
        TokenKey::PermitNonce { .. } => TokenValue::PermitNonce(u64::read(&mut reader)?),
        TokenKey::Delegation { .. } => TokenValue::Delegation(Delegation::read(&mut reader)?),
        TokenKey::VotePower { .. } => TokenValue::VotePower(u64::read(&mut reader)?),
        TokenKey::CheckpointCount { .. } => TokenValue::CheckpointCount(u32::read(&mut reader)?),
        TokenKey::BalanceCheckpointCount { .. } => {
            TokenValue::BalanceCheckpointCount(u32::read(&mut reader)?)
        }
        TokenKey::DelegationCheckpointCount { .. } => {
            TokenValue::DelegationCheckpointCount(u32::read(&mut reader)?)
        }
        TokenKey::Checkpoint { .. } => TokenValue::Checkpoint(Checkpoint::read(&mut reader)?),
        TokenKey::BalanceCheckpoint { .. } => {
            TokenValue::BalanceCheckpoint(BalanceCheckpoint::read(&mut reader)?)
        }
        TokenKey::DelegationCheckpoint { .. } => {
            TokenValue::DelegationCheckpoint(DelegationCheckpoint::read(&mut reader)?)
        }
        TokenKey::SupplyCheckpoint { .. } => {
            TokenValue::SupplyCheckpoint(SupplyCheckpoint::read(&mut reader)?)
        }
        TokenKey::SupplyCheckpointCount(_) => {
            TokenValue::SupplyCheckpointCount(u32::read(&mut reader)?)
        }
        TokenKey::RoleConfig { .. } => TokenValue::RoleConfig(RoleConfig::read(&mut reader)?),
        TokenKey::RoleMember { .. } => TokenValue::RoleMemberGrantedAt(u64::read(&mut reader)?),
        TokenKey::RoleMembers { .. } => {
            TokenValue::RoleMembers(Vec::<[u8; 32]>::read(&mut reader)?)
        }
        TokenKey::Escrow { .. } => TokenValue::Escrow(Escrow::read(&mut reader)?),
        TokenKey::EscrowCounter(_) => TokenValue::EscrowCounter(u64::read(&mut reader)?),
        TokenKey::UserEscrows { .. } => TokenValue::UserEscrows(Vec::<u64>::read(&mut reader)?),
        TokenKey::AgentAuth { .. } => TokenValue::AgentAuth(AgentAuthorization::read(&mut reader)?),
        TokenKey::OwnerAgents { .. } => {
            TokenValue::OwnerAgents(Vec::<[u8; 32]>::read(&mut reader)?)
        }
        TokenKey::Delegators { .. } => TokenValue::Delegators(Vec::<[u8; 32]>::read(&mut reader)?),
        TokenKey::PendingAdmin(_) => {
            TokenValue::PendingAdmin(Option::<[u8; 32]>::read(&mut reader)?)
        }
        TokenKey::AdminDelay(_) => TokenValue::AdminDelay(AdminDelay::read(&mut reader)?),
        TokenKey::TimelockMinDelay(_) => TokenValue::TimelockMinDelay(u64::read(&mut reader)?),
        TokenKey::TimelockOperation { .. } => {
            TokenValue::TimelockOperation(TimelockOperation::read(&mut reader)?)
        }
        TokenKey::MetadataUri(_) => TokenValue::MetadataUri(Option::<String>::read(&mut reader)?),
    };

    Ok(value)
}

impl RocksStorage {
    fn build_contract_asset_ext_key(
        contract_id: ContractId,
        asset: &Hash,
        subkey: &[u8],
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(40 + subkey.len());
        buf.extend_from_slice(&contract_id.to_be_bytes());
        buf.extend_from_slice(asset.as_bytes());
        buf.extend_from_slice(subkey);
        buf
    }

    fn build_versioned_contract_asset_ext_key(
        contract_id: ContractId,
        asset: &Hash,
        topoheight: TopoHeight,
        subkey: &[u8],
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(48 + subkey.len());
        buf.extend_from_slice(&topoheight.to_be_bytes());
        buf.extend_from_slice(&contract_id.to_be_bytes());
        buf.extend_from_slice(asset.as_bytes());
        buf.extend_from_slice(subkey);
        buf
    }
}

#[async_trait]
impl ContractAssetExtProvider for RocksStorage {
    async fn get_contract_asset_ext(
        &self,
        contract: &Hash,
        key: &TokenKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, TokenValue)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset ext {} at maximum topoheight {}",
                contract,
                topoheight
            );
        }
        let Some(contract_id) = self.get_optional_contract_id(contract)? else {
            return Ok(None);
        };
        let asset = asset_from_key(key);
        let subkey = encode_subkey(key)?;
        let pointer_key = Self::build_contract_asset_ext_key(contract_id, asset, &subkey);
        let mut prev_topo =
            self.load_optional_from_disk(Column::ContractsAssetExt, &pointer_key)?;

        while let Some(topo) = prev_topo {
            let versioned_key =
                Self::build_versioned_contract_asset_ext_key(contract_id, asset, topo, &subkey);
            let versioned: Versioned<Vec<u8>> =
                self.load_from_disk(Column::VersionedContractsAssetExt, &versioned_key)?;
            if topo <= topoheight {
                let value = decode_value(key, versioned.get())?;
                return Ok(Some((topo, value)));
            }
            prev_topo = versioned.get_previous_topoheight();
        }

        Ok(None)
    }

    async fn set_last_contract_asset_ext_to(
        &mut self,
        contract: &Hash,
        key: &TokenKey,
        topoheight: TopoHeight,
        value: &TokenValue,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset ext {} to topoheight {}",
                contract,
                topoheight
            );
        }
        let contract_id = self.get_contract_id(contract)?;
        let asset = asset_from_key(key);
        let subkey = encode_subkey(key)?;
        let pointer_key = Self::build_contract_asset_ext_key(contract_id, asset, &subkey);
        let previous = self.load_optional_from_disk(Column::ContractsAssetExt, &pointer_key)?;
        let encoded = encode_value(key, value)?;
        let versioned = Versioned::new(encoded, previous);
        let versioned_key =
            Self::build_versioned_contract_asset_ext_key(contract_id, asset, topoheight, &subkey);

        self.insert_into_disk(
            Column::ContractsAssetExt,
            &pointer_key,
            &topoheight.to_be_bytes(),
        )?;
        self.insert_into_disk(
            Column::VersionedContractsAssetExt,
            &versioned_key,
            &versioned,
        )
    }

    async fn delete_contract_asset_ext(
        &mut self,
        contract: &Hash,
        key: &TokenKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract asset ext {} at topoheight {}",
                contract,
                topoheight
            );
        }
        let contract_id = self.get_contract_id(contract)?;
        let asset = asset_from_key(key);
        let subkey = encode_subkey(key)?;
        let pointer_key = Self::build_contract_asset_ext_key(contract_id, asset, &subkey);
        let previous = self.load_optional_from_disk(Column::ContractsAssetExt, &pointer_key)?;
        let versioned = Versioned::new(Vec::<u8>::new(), previous);
        let versioned_key =
            Self::build_versioned_contract_asset_ext_key(contract_id, asset, topoheight, &subkey);

        self.insert_into_disk(
            Column::ContractsAssetExt,
            &pointer_key,
            &topoheight.to_be_bytes(),
        )?;
        self.insert_into_disk(
            Column::VersionedContractsAssetExt,
            &versioned_key,
            &versioned,
        )
    }
}
