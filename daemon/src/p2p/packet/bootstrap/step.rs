use crate::{
    config::{CHAIN_SYNC_REQUEST_MAX_BLOCKS, PEER_MAX_PACKET_SIZE, PRUNE_SAFETY_LIMIT},
    p2p::packet::{
        bootstrap::BlockMetadata,
        chain::{BlockId, CommonPoint},
    },
};
use indexmap::{IndexMap, IndexSet};
use log::debug;
use std::borrow::Cow;
use tos_common::{
    account::{AccountSummary, AgentAccountMeta, Balance, EnergyResource, Nonce, UnoBalance},
    arbitration::ArbiterAccount,
    asset::AssetData,
    block::TopoHeight,
    contract::{ScheduledExecution, MAX_KEY_SIZE, MAX_VALUE_SIZE},
    contract_asset::ContractAssetData,
    crypto::{Hash, PublicKey},
    escrow::EscrowAccount,
    kyc::{KycData, SecurityCommittee},
    nft::{Nft, NftCollection},
    referral::ReferralRecord,
    serializer::{Reader, ReaderError, Serializer, Writer},
    static_assert,
    transaction::{CommitArbitrationOpenPayload, MultiSigPayload},
    versioned_type::State,
};
use tos_kernel::{Module, ValueCell};

// this file implements the protocol for the fast sync (bootstrapped chain)
// You will have to request through StepRequest::FetchAssets all the registered assets
// based on the size of the chain, you can have pagination or not.
// With the set of assets, you can retrieve all registered keys for it and then its balances
// Nonces need to be retrieve only one time because its common for all assets.
// The protocol is based on
// how many items we can answer per request

pub const MAX_ITEMS_PER_PAGE: usize = 1024; // 1k items per page

// Contract Stores can be a big packet, we must ensure that we are below the max packet size
// 8 overhead for the packet bootstrap id
static_assert!(
    8 + MAX_ITEMS_PER_PAGE * (MAX_KEY_SIZE + MAX_VALUE_SIZE) + 32 <= PEER_MAX_PACKET_SIZE as usize,
    "Contract Stores packet must be below max packet size"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub enum StepKind {
    ChainInfo,
    Assets,
    Keys,
    KeyBalances,
    Accounts,
    MultiSigs,
    Contracts,
    // TOS extensions
    Kyc,
    Committees,
    Nft,
    Escrow,
    Arbitration,
    Tns,
    Energy,
    Referral,
    UnoBalance,
    Agent,
    A2aNonce,
    ContractAsset,
    BlocksMetadata,
}

impl StepKind {
    pub fn next(&self) -> Option<Self> {
        Some(match self {
            Self::ChainInfo => Self::Assets,
            Self::Assets => Self::Keys,
            Self::Keys => Self::KeyBalances,
            Self::KeyBalances => Self::Accounts,
            Self::Accounts => Self::MultiSigs,
            Self::MultiSigs => Self::Contracts,
            Self::Contracts => Self::Kyc,
            Self::Kyc => Self::Committees,
            Self::Committees => Self::Nft,
            Self::Nft => Self::Escrow,
            Self::Escrow => Self::Arbitration,
            Self::Arbitration => Self::Tns,
            Self::Tns => Self::Energy,
            Self::Energy => Self::Referral,
            Self::Referral => Self::UnoBalance,
            Self::UnoBalance => Self::Agent,
            Self::Agent => Self::A2aNonce,
            Self::A2aNonce => Self::ContractAsset,
            Self::ContractAsset => Self::BlocksMetadata,
            Self::BlocksMetadata => return None,
        })
    }
}

#[derive(Debug)]
pub enum StepRequest<'a> {
    // Request chain info (top topoheight, top height, top hash)
    ChainInfo(IndexSet<BlockId>),
    // Min topoheight, Max topoheight, Pagination
    Assets(TopoHeight, TopoHeight, Option<u64>),
    // stable topoheight, assets (grouped by 1024)
    AssetsSupply(TopoHeight, Cow<'a, IndexSet<Hash>>),
    // Min topoheight, Max topoheight, pagination
    Keys(TopoHeight, TopoHeight, Option<u64>),
    // Request the assets for a public key
    // Can request up to 1024 keys per page
    // Key, min topoheight, max topoheight, pagination
    KeyBalances(Cow<'a, PublicKey>, TopoHeight, TopoHeight, Option<u64>),
    // Request the spendable balances of a public key
    // Can request up to 1024 keys per page
    // Key, Asset, min topoheight, max topoheight (exclusive range)
    SpendableBalances(Cow<'a, PublicKey>, Cow<'a, Hash>, TopoHeight, TopoHeight),
    // Request the nonces of a list of public key
    // min TopoHeight, max Topoheight, List of public keys
    Accounts(TopoHeight, TopoHeight, Cow<'a, IndexSet<PublicKey>>),
    // Min topoheight, Max topoheight, pagination
    Contracts(TopoHeight, TopoHeight, Option<u64>),
    // Request the contract module and its metadata
    // min TopoHeight, max Topoheight, Hash of the contract
    ContractModule(TopoHeight, TopoHeight, Cow<'a, Hash>),
    // Request the contract balances
    // Hash of the contract, topoheight, page
    ContractBalances(Cow<'a, Hash>, TopoHeight, Option<u64>),
    // Request the contract stores
    // Hash of the contract, topoheight, page
    ContractStores(Cow<'a, Hash>, TopoHeight, Option<u64>),
    // Min topoheight, Max topoheight, pagination
    ContractsExecutions(TopoHeight, TopoHeight, Option<u64>),

    // === TOS Extension Steps (IDs 13-28) ===

    // KYC data, pagination
    KycData(Option<u64>),
    // Committees, pagination
    Committees(Option<u64>),
    // Global committee (no params)
    GlobalCommittee,
    // NFT collections, topoheight, pagination
    NftCollections(TopoHeight, Option<u64>),
    // NFT tokens per collection, collection_id, topoheight, pagination
    NftTokens(Cow<'a, Hash>, TopoHeight, Option<u64>),
    // NFT ownership per collection, collection_id, topoheight, pagination
    NftOwnership(Cow<'a, Hash>, TopoHeight, Option<u64>),
    // Escrow accounts, pagination
    EscrowAccounts(Option<u64>),
    // Arbitration data, pagination
    ArbitrationData(Option<u64>),
    // Arbiter accounts, pagination
    ArbiterAccounts(Option<u64>),
    // TNS name records, pagination
    TnsNames(Option<u64>),
    // Energy data (batch request by keys), topoheight
    EnergyData(Cow<'a, Vec<PublicKey>>, TopoHeight),
    // Referral records, pagination
    ReferralRecords(Option<u64>),
    // UNO balances per key/asset, key, asset, topoheight, pagination
    UnoBalances(Cow<'a, PublicKey>, Cow<'a, Hash>, TopoHeight, Option<u64>),
    // Agent account data, pagination
    AgentData(Option<u64>),
    // A2A nonces, pagination
    A2aNonces(Option<u64>),
    // Contract asset data, pagination
    ContractAssets(Option<u64>),
    // UNO balance keys discovery (list all key+asset pairs), pagination
    UnoBalanceKeys(Option<u64>),

    // Request blocks metadata starting topoheight
    BlocksMetadata(TopoHeight),
}

impl<'a> StepRequest<'a> {
    pub fn kind(&self) -> StepKind {
        match self {
            Self::ChainInfo(_) => StepKind::ChainInfo,
            Self::Assets(_, _, _) => StepKind::Assets,
            Self::AssetsSupply(_, _) => StepKind::Assets,
            Self::Keys(_, _, _) => StepKind::Keys,
            Self::KeyBalances(_, _, _, _) => StepKind::KeyBalances,
            Self::SpendableBalances(_, _, _, _) => StepKind::KeyBalances,
            Self::Accounts(_, _, _) => StepKind::Accounts,
            Self::Contracts(_, _, _) => StepKind::Contracts,
            Self::ContractModule(_, _, _) => StepKind::Contracts,
            Self::ContractBalances(_, _, _) => StepKind::Contracts,
            Self::ContractStores(_, _, _) => StepKind::Contracts,
            Self::ContractsExecutions(_, _, _) => StepKind::Contracts,
            Self::KycData(_) => StepKind::Kyc,
            Self::Committees(_) => StepKind::Committees,
            Self::GlobalCommittee => StepKind::Committees,
            Self::NftCollections(_, _) => StepKind::Nft,
            Self::NftTokens(_, _, _) => StepKind::Nft,
            Self::NftOwnership(_, _, _) => StepKind::Nft,
            Self::EscrowAccounts(_) => StepKind::Escrow,
            Self::ArbitrationData(_) => StepKind::Arbitration,
            Self::ArbiterAccounts(_) => StepKind::Arbitration,
            Self::TnsNames(_) => StepKind::Tns,
            Self::EnergyData(_, _) => StepKind::Energy,
            Self::ReferralRecords(_) => StepKind::Referral,
            Self::UnoBalances(_, _, _, _) => StepKind::UnoBalance,
            Self::AgentData(_) => StepKind::Agent,
            Self::A2aNonces(_) => StepKind::A2aNonce,
            Self::ContractAssets(_) => StepKind::ContractAsset,
            Self::UnoBalanceKeys(_) => StepKind::UnoBalance,
            Self::BlocksMetadata(_) => StepKind::BlocksMetadata,
        }
    }

    pub fn get_requested_topoheight(&self) -> Option<u64> {
        Some(*match self {
            Self::Assets(_, topo, _) => topo,
            Self::AssetsSupply(topo, _) => topo,
            Self::Keys(_, topo, _) => topo,
            Self::KeyBalances(_, _, topo, _) => topo,
            Self::SpendableBalances(_, _, _, topo) => topo,
            Self::Accounts(_, topo, _) => topo,
            Self::Contracts(_, topo, _) => topo,
            Self::ContractModule(_, topo, _) => topo,
            Self::ContractBalances(_, topo, _) => topo,
            Self::ContractsExecutions(_, topo, _) => topo,
            Self::BlocksMetadata(topo) => topo,
            _ => return None,
        })
    }
}

impl Serializer for StepRequest<'_> {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(match reader.read_u8()? {
            0 => {
                let len = reader.read_u8()?;
                if len == 0 || len > CHAIN_SYNC_REQUEST_MAX_BLOCKS as u8 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid chain info request length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut blocks = IndexSet::with_capacity(len as usize);
                for _ in 0..len {
                    if !blocks.insert(BlockId::read(reader)?) {
                        debug!("Duplicated block id for chain info request");
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::ChainInfo(blocks)
            }
            1 => {
                let min_topoheight = reader.read_u64()?;
                let topoheight = reader.read_u64()?;
                if min_topoheight > topoheight {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid min topoheight in Step Request");
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Request");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Assets(min_topoheight, topoheight, page)
            }
            2 => {
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                if min > max {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid min topoheight in Step Request");
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Request");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Keys(min, max, page)
            }
            3 => {
                let key = Cow::read(reader)?;
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                if min > max {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid min topoheight in Step Request");
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Request");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::KeyBalances(key, min, max, page)
            }
            4 => {
                let key = Cow::read(reader)?;
                let asset = Cow::read(reader)?;
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                if min > max {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid min topoheight in Step Request");
                    }
                    return Err(ReaderError::InvalidValue);
                }

                Self::SpendableBalances(key, asset, min, max)
            }
            5 => {
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid accounts request length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut keys = IndexSet::with_capacity(len as usize);
                for _ in 0..len {
                    if !keys.insert(PublicKey::read(reader)?) {
                        debug!("Duplicated public key for accounts request");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                Self::Accounts(min, max, Cow::Owned(keys))
            }
            6 => {
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Request");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Contracts(min, max, page)
            }
            7 => {
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                let hash = Cow::read(reader)?;
                Self::ContractModule(min, max, hash)
            }
            8 => {
                let hash = Cow::read(reader)?;
                let topoheight = reader.read_u64()?;
                let page = Option::read(reader)?;
                Self::ContractBalances(hash, topoheight, page)
            }
            9 => {
                let hash = Cow::read(reader)?;
                let topoheight = reader.read_u64()?;
                let page = Option::read(reader)?;
                Self::ContractStores(hash, topoheight, page)
            }
            10 => Self::BlocksMetadata(reader.read_u64()?),
            11 => {
                let topoheight = reader.read_u64()?;
                let len = reader.read_u16()?;
                if len == 0 || len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid assets request length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut assets = IndexSet::with_capacity(len as usize);
                for _ in 0..len {
                    if !assets.insert(Hash::read(reader)?) {
                        debug!("Duplicated asset id for assets supply request");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                Self::AssetsSupply(topoheight, Cow::Owned(assets))
            }
            12 => {
                let min = reader.read_u64()?;
                let max = reader.read_u64()?;
                if min > max {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid min topoheight in Step Request");
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Request");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::ContractsExecutions(min, max, page)
            }
            13 => Self::KycData(Option::read(reader)?),
            14 => Self::Committees(Option::read(reader)?),
            15 => Self::GlobalCommittee,
            16 => {
                let topo = reader.read_u64()?;
                Self::NftCollections(topo, Option::read(reader)?)
            }
            17 => {
                let collection = Cow::read(reader)?;
                let topo = reader.read_u64()?;
                Self::NftTokens(collection, topo, Option::read(reader)?)
            }
            18 => {
                let collection = Cow::read(reader)?;
                let topo = reader.read_u64()?;
                Self::NftOwnership(collection, topo, Option::read(reader)?)
            }
            19 => Self::EscrowAccounts(Option::read(reader)?),
            20 => Self::ArbitrationData(Option::read(reader)?),
            21 => Self::ArbiterAccounts(Option::read(reader)?),
            22 => Self::TnsNames(Option::read(reader)?),
            23 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    return Err(ReaderError::InvalidValue);
                }
                let mut keys = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    keys.push(PublicKey::read(reader)?);
                }
                let topo = reader.read_u64()?;
                Self::EnergyData(Cow::Owned(keys), topo)
            }
            24 => Self::ReferralRecords(Option::read(reader)?),
            25 => {
                let key = Cow::read(reader)?;
                let asset = Cow::read(reader)?;
                let topo = reader.read_u64()?;
                Self::UnoBalances(key, asset, topo, Option::read(reader)?)
            }
            26 => Self::AgentData(Option::read(reader)?),
            27 => Self::A2aNonces(Option::read(reader)?),
            28 => Self::ContractAssets(Option::read(reader)?),
            29 => Self::UnoBalanceKeys(Option::read(reader)?),
            id => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Received invalid value for StepRequest: {}", id);
                }
                return Err(ReaderError::InvalidValue);
            }
        })
    }

    fn write(&self, writer: &mut Writer) {
        match self {
            Self::ChainInfo(blocks) => {
                writer.write_u8(0);
                writer.write_u8(blocks.len() as u8);
                for block_id in blocks {
                    block_id.write(writer);
                }
            }
            Self::Assets(min, max, page) => {
                writer.write_u8(1);
                writer.write_u64(min);
                writer.write_u64(max);
                page.write(writer);
            }
            Self::AssetsSupply(topoheight, assets) => {
                writer.write_u8(11);
                topoheight.write(writer);
                assets.write(writer);
            }
            Self::Keys(min, max, page) => {
                writer.write_u8(2);
                writer.write_u64(min);
                writer.write_u64(max);
                page.write(writer);
            }
            Self::KeyBalances(key, min, max, page) => {
                writer.write_u8(3);
                key.write(writer);
                writer.write_u64(min);
                writer.write_u64(max);
                page.write(writer);
            }
            Self::SpendableBalances(key, asset, min, max) => {
                writer.write_u8(4);
                key.write(writer);
                asset.write(writer);
                writer.write_u64(min);
                writer.write_u64(max);
            }
            Self::Accounts(min, max, keys) => {
                writer.write_u8(5);
                writer.write_u64(min);
                writer.write_u64(max);
                keys.write(writer);
            }
            Self::Contracts(min, max, pagination) => {
                writer.write_u8(6);
                writer.write_u64(min);
                writer.write_u64(max);
                pagination.write(writer);
            }
            Self::ContractModule(min, max, hash) => {
                writer.write_u8(7);
                writer.write_u64(min);
                writer.write_u64(max);
                hash.write(writer);
            }
            Self::ContractBalances(hash, topoheight, page) => {
                writer.write_u8(8);
                hash.write(writer);
                topoheight.write(writer);
                page.write(writer);
            }
            Self::ContractStores(hash, topoheight, page) => {
                writer.write_u8(9);
                hash.write(writer);
                topoheight.write(writer);
                page.write(writer);
            }
            Self::ContractsExecutions(min, max, page) => {
                writer.write_u8(12);
                min.write(writer);
                max.write(writer);
                page.write(writer);
            }
            Self::KycData(page) => {
                writer.write_u8(13);
                page.write(writer);
            }
            Self::Committees(page) => {
                writer.write_u8(14);
                page.write(writer);
            }
            Self::GlobalCommittee => {
                writer.write_u8(15);
            }
            Self::NftCollections(topo, page) => {
                writer.write_u8(16);
                writer.write_u64(topo);
                page.write(writer);
            }
            Self::NftTokens(collection, topo, page) => {
                writer.write_u8(17);
                collection.write(writer);
                writer.write_u64(topo);
                page.write(writer);
            }
            Self::NftOwnership(collection, topo, page) => {
                writer.write_u8(18);
                collection.write(writer);
                writer.write_u64(topo);
                page.write(writer);
            }
            Self::EscrowAccounts(page) => {
                writer.write_u8(19);
                page.write(writer);
            }
            Self::ArbitrationData(page) => {
                writer.write_u8(20);
                page.write(writer);
            }
            Self::ArbiterAccounts(page) => {
                writer.write_u8(21);
                page.write(writer);
            }
            Self::TnsNames(page) => {
                writer.write_u8(22);
                page.write(writer);
            }
            Self::EnergyData(keys, topo) => {
                writer.write_u8(23);
                writer.write_u16(keys.len() as u16);
                for key in keys.iter() {
                    key.write(writer);
                }
                writer.write_u64(topo);
            }
            Self::ReferralRecords(page) => {
                writer.write_u8(24);
                page.write(writer);
            }
            Self::UnoBalances(key, asset, topo, page) => {
                writer.write_u8(25);
                key.write(writer);
                asset.write(writer);
                writer.write_u64(topo);
                page.write(writer);
            }
            Self::AgentData(page) => {
                writer.write_u8(26);
                page.write(writer);
            }
            Self::A2aNonces(page) => {
                writer.write_u8(27);
                page.write(writer);
            }
            Self::ContractAssets(page) => {
                writer.write_u8(28);
                page.write(writer);
            }
            Self::UnoBalanceKeys(page) => {
                writer.write_u8(29);
                page.write(writer);
            }
            Self::BlocksMetadata(topoheight) => {
                writer.write_u8(10);
                writer.write_u64(topoheight);
            }
        };
    }

    fn size(&self) -> usize {
        let size = match self {
            Self::ChainInfo(blocks) => 1 + blocks.size(),
            Self::Assets(min, max, page) => min.size() + max.size() + page.size(),
            Self::AssetsSupply(topoheight, assets) => topoheight.size() + assets.size(),
            Self::Keys(min, max, page) => min.size() + max.size() + page.size(),
            Self::KeyBalances(key, min, max, page) => {
                key.size() + min.size() + max.size() + page.size()
            }
            Self::SpendableBalances(key, asset, min, max) => {
                key.size() + asset.size() + min.size() + max.size()
            }
            Self::Accounts(min, max, nonces) => min.size() + max.size() + nonces.size(),
            Self::Contracts(min, max, pagination) => min.size() + max.size() + pagination.size(),
            Self::ContractModule(min, max, hash) => min.size() + max.size() + hash.size(),
            Self::ContractBalances(hash, topoheight, page) => {
                hash.size() + topoheight.size() + page.size()
            }
            Self::ContractStores(hash, topoheight, page) => {
                hash.size() + topoheight.size() + page.size()
            }
            Self::ContractsExecutions(min, max, page) => min.size() + max.size() + page.size(),
            Self::KycData(page) => page.size(),
            Self::Committees(page) => page.size(),
            Self::GlobalCommittee => 0,
            Self::NftCollections(topo, page) => topo.size() + page.size(),
            Self::NftTokens(collection, topo, page) => {
                collection.size() + topo.size() + page.size()
            }
            Self::NftOwnership(collection, topo, page) => {
                collection.size() + topo.size() + page.size()
            }
            Self::EscrowAccounts(page) => page.size(),
            Self::ArbitrationData(page) => page.size(),
            Self::ArbiterAccounts(page) => page.size(),
            Self::TnsNames(page) => page.size(),
            Self::EnergyData(keys, topo) => {
                2 + keys.iter().map(|k| k.size()).sum::<usize>() + topo.size()
            }
            Self::ReferralRecords(page) => page.size(),
            Self::UnoBalances(key, asset, topo, page) => {
                key.size() + asset.size() + topo.size() + page.size()
            }
            Self::AgentData(page) => page.size(),
            Self::A2aNonces(page) => page.size(),
            Self::ContractAssets(page) => page.size(),
            Self::UnoBalanceKeys(page) => page.size(),
            Self::BlocksMetadata(topoheight) => topoheight.size(),
        };
        // 1 for the id
        size + 1
    }
}

#[derive(Debug)]
pub enum StepResponse {
    // common point, topoheight of stable hash, stable height, stable hash
    ChainInfo(Option<CommonPoint>, u64, u64, Hash),
    // Set of assets, pagination
    Assets(IndexMap<Hash, AssetData>, Option<u64>),
    // List of circulating supply (positional, matching request order)
    AssetsSupply(Vec<Option<u64>>),
    // Set of keys, pagination
    Keys(IndexSet<PublicKey>, Option<u64>),
    // All assets for requested key, pagination
    KeyBalances(IndexMap<Hash, Option<AccountSummary>>, Option<u64>),
    // This is for per key/account only
    // TopoHeight is for the next max exclusive topoheight (if none, no more data)
    SpendableBalances(Vec<Balance>, Option<TopoHeight>),
    // Nonces and multisig states for requested accounts
    // It is optional in case the peer send us some keys
    // that got deleted because he forked
    Accounts(Vec<(State<Nonce>, State<MultiSigPayload>)>),
    // Contracts hashes with pagination
    Contracts(IndexSet<Hash>, Option<u64>),
    // Contract module
    // This is one by one due to the potential max size
    ContractModule(State<Module>),
    // Contract assets
    // all assets detected, pagination
    ContractBalances(IndexMap<Hash, u64>, Option<u64>),
    // Contract assets
    // all assets detected, pagination
    ContractStores(IndexMap<ValueCell, ValueCell>, Option<u64>),
    // Contract executions
    ContractsExecutions(Vec<ScheduledExecution>, Option<u64>),

    // === TOS Extension Responses (IDs 13-28) ===

    // KYC data entries, pagination
    KycData(IndexMap<PublicKey, KycData>, Option<u64>),
    // Committee entries, pagination
    Committees(IndexMap<Hash, SecurityCommittee>, Option<u64>),
    // Global committee ID
    GlobalCommittee(Option<Hash>),
    // NFT collection entries, pagination
    NftCollections(IndexMap<Hash, NftCollection>, Option<u64>),
    // NFT tokens for a collection, collection_id, tokens, pagination
    NftTokens(Hash, IndexMap<u64, Nft>, Option<u64>),
    // NFT ownership for a collection, collection_id, token_id -> owner, pagination
    NftOwnership(Hash, IndexMap<u64, PublicKey>, Option<u64>),
    // Escrow account entries, pagination
    EscrowAccounts(IndexMap<Hash, EscrowAccount>, Option<u64>),
    // Arbitration request entries, pagination
    ArbitrationData(Vec<CommitArbitrationOpenPayload>, Option<u64>),
    // Arbiter account entries, pagination
    ArbiterAccounts(IndexMap<PublicKey, ArbiterAccount>, Option<u64>),
    // TNS name -> owner entries, pagination
    TnsNames(IndexMap<Hash, PublicKey>, Option<u64>),
    // Energy resource data (positional, matching request order)
    EnergyData(Vec<Option<EnergyResource>>),
    // Referral record entries, pagination
    ReferralRecords(IndexMap<PublicKey, ReferralRecord>, Option<u64>),
    // UNO balance entries, pagination
    UnoBalances(Vec<UnoBalance>, Option<u64>),
    // Agent account entries, pagination
    AgentData(IndexMap<PublicKey, AgentAccountMeta>, Option<u64>),
    // A2A nonce entries (nonce_bytes, timestamp), pagination
    A2aNonces(Vec<(Vec<u8>, u64)>, Option<u64>),
    // Contract asset data entries, pagination
    ContractAssets(IndexMap<Hash, ContractAssetData>, Option<u64>),
    // UNO balance keys discovery (key, asset pairs), pagination
    UnoBalanceKeys(Vec<(PublicKey, Hash)>, Option<u64>),

    // top blocks metadata
    BlocksMetadata(IndexSet<BlockMetadata>),
}

impl StepResponse {
    pub fn kind(&self) -> StepKind {
        match self {
            Self::ChainInfo(_, _, _, _) => StepKind::ChainInfo,
            Self::Assets(_, _) => StepKind::Assets,
            Self::AssetsSupply(_) => StepKind::Assets,
            Self::Keys(_, _) => StepKind::Keys,
            Self::KeyBalances(_, _) => StepKind::KeyBalances,
            Self::SpendableBalances(_, _) => StepKind::KeyBalances,
            Self::Accounts(_) => StepKind::Accounts,
            Self::Contracts(_, _) => StepKind::Contracts,
            Self::ContractModule(_) => StepKind::Contracts,
            Self::ContractBalances(_, _) => StepKind::Contracts,
            Self::ContractStores(_, _) => StepKind::Contracts,
            Self::ContractsExecutions(_, _) => StepKind::Contracts,
            Self::KycData(_, _) => StepKind::Kyc,
            Self::Committees(_, _) => StepKind::Committees,
            Self::GlobalCommittee(_) => StepKind::Committees,
            Self::NftCollections(_, _) => StepKind::Nft,
            Self::NftTokens(_, _, _) => StepKind::Nft,
            Self::NftOwnership(_, _, _) => StepKind::Nft,
            Self::EscrowAccounts(_, _) => StepKind::Escrow,
            Self::ArbitrationData(_, _) => StepKind::Arbitration,
            Self::ArbiterAccounts(_, _) => StepKind::Arbitration,
            Self::TnsNames(_, _) => StepKind::Tns,
            Self::EnergyData(_) => StepKind::Energy,
            Self::ReferralRecords(_, _) => StepKind::Referral,
            Self::UnoBalances(_, _) => StepKind::UnoBalance,
            Self::AgentData(_, _) => StepKind::Agent,
            Self::A2aNonces(_, _) => StepKind::A2aNonce,
            Self::ContractAssets(_, _) => StepKind::ContractAsset,
            Self::UnoBalanceKeys(_, _) => StepKind::UnoBalance,
            Self::BlocksMetadata(_) => StepKind::BlocksMetadata,
        }
    }
}

impl Serializer for StepResponse {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(match reader.read_u8()? {
            0 => {
                let common_point = Option::read(reader)?;
                let topoheight = reader.read_u64()?;
                let stable_height = reader.read_u64()?;
                let hash = reader.read_hash()?;

                Self::ChainInfo(common_point, topoheight, stable_height, hash)
            }
            1 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid assets response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut assets = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    let value = AssetData::read(reader)?;
                    if assets.insert(key, value).is_some() {
                        debug!("Duplicated asset key in Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Assets(assets, page)
            }
            11 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid assets supply response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut values = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    values.push(Option::read(reader)?);
                }

                Self::AssetsSupply(values)
            }
            2 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid keys response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }
                let mut keys = IndexSet::with_capacity(len as usize);
                for _ in 0..len {
                    if !keys.insert(PublicKey::read(reader)?) {
                        debug!("Duplicated public key in Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Keys(keys, page)
            }
            3 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid key balances response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }
                let mut keys = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    let value = Option::read(reader)?;
                    if keys.insert(key, value).is_some() {
                        debug!("Duplicated key in Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::KeyBalances(keys, page)
            }
            4 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid spendable balances response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut balances = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let balance = Balance::read(reader)?;
                    balances.push(balance);
                }

                Self::SpendableBalances(balances, Option::read(reader)?)
            }
            5 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid accounts response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }
                let mut accounts = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let nonce = State::<Nonce>::read(reader)?;
                    let multisig = State::<MultiSigPayload>::read(reader)?;
                    accounts.push((nonce, multisig));
                }

                Self::Accounts(accounts)
            }
            6 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid contracts response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut contracts = IndexSet::with_capacity(len as usize);
                for _ in 0..len {
                    if !contracts.insert(Hash::read(reader)?) {
                        debug!("Duplicated contract hash in Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Contracts(contracts, page)
            }
            7 => Self::ContractModule(State::read(reader)?),
            8 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid contracts assets response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut assets = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let asset = Hash::read(reader)?;
                    let value = reader.read_u64()?;
                    if assets.insert(asset, value).is_some() {
                        debug!("Duplicated contract asset in Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }

                Self::ContractBalances(assets, page)
            }
            9 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid contracts assets response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = ValueCell::read(reader)?;
                    let value = ValueCell::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Duplicated contract store in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }

                Self::ContractStores(entries, page)
            }
            12 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid contracts executions response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut executions = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    executions.push(ScheduledExecution::read(reader)?);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }

                Self::ContractsExecutions(executions, page)
            }
            13 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid kyc data response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = PublicKey::read(reader)?;
                    let value = KycData::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated public key in KycData Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::KycData(entries, page)
            }
            14 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid committees response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    let value = SecurityCommittee::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated hash in Committees Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::Committees(entries, page)
            }
            15 => Self::GlobalCommittee(Option::read(reader)?),
            16 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid nft collections response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    let value = NftCollection::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated hash in NftCollections Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::NftCollections(entries, page)
            }
            17 => {
                let collection_id = Hash::read(reader)?;
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid nft tokens response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let token_id = reader.read_u64()?;
                    let nft = Nft::read(reader)?;
                    if entries.insert(token_id, nft).is_some() {
                        debug!("Duplicated token id in NftTokens Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::NftTokens(collection_id, entries, page)
            }
            18 => {
                let collection_id = Hash::read(reader)?;
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid nft ownership response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let token_id = reader.read_u64()?;
                    let owner = PublicKey::read(reader)?;
                    if entries.insert(token_id, owner).is_some() {
                        debug!("Duplicated token id in NftOwnership Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::NftOwnership(collection_id, entries, page)
            }
            19 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid escrow accounts response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    let value = EscrowAccount::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated hash in EscrowAccounts Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::EscrowAccounts(entries, page)
            }
            20 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid arbitration data response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    entries.push(CommitArbitrationOpenPayload::read(reader)?);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::ArbitrationData(entries, page)
            }
            21 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid arbiter accounts response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = PublicKey::read(reader)?;
                    let value = ArbiterAccount::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated public key in ArbiterAccounts Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::ArbiterAccounts(entries, page)
            }
            22 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid tns names response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    let value = PublicKey::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated hash in TnsNames Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::TnsNames(entries, page)
            }
            23 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid energy data response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    entries.push(Option::read(reader)?);
                }

                Self::EnergyData(entries)
            }
            24 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid referral records response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = PublicKey::read(reader)?;
                    let value = ReferralRecord::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated public key in ReferralRecords Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::ReferralRecords(entries, page)
            }
            25 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid uno balances response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    entries.push(UnoBalance::read(reader)?);
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::UnoBalances(entries, page)
            }
            26 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid agent data response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = PublicKey::read(reader)?;
                    let value = AgentAccountMeta::read(reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated public key in AgentData Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::AgentData(entries, page)
            }
            27 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid a2a nonces response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let nonce_len = reader.read_u16()?;
                    let nonce_bytes = reader.read_bytes(nonce_len as usize)?;
                    let timestamp = reader.read_u64()?;
                    entries.push((nonce_bytes, timestamp));
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::A2aNonces(entries, page)
            }
            28 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid contract assets response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = IndexMap::with_capacity(len as usize);
                for _ in 0..len {
                    let key = Hash::read(reader)?;
                    // ContractAssetData::read() checks reader.size() == 0,
                    // so we must use a sub-reader with the exact data length
                    let data_len = reader.read_u32()? as usize;
                    let data_bytes = reader.read_bytes_ref(data_len)?;
                    let mut sub_reader = Reader::new(data_bytes);
                    let value = ContractAssetData::read(&mut sub_reader)?;
                    if entries.insert(key, value).is_some() {
                        debug!("Duplicated hash in ContractAssets Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::ContractAssets(entries, page)
            }
            29 => {
                let len = reader.read_u16()?;
                if len > MAX_ITEMS_PER_PAGE as u16 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid uno balance keys response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut entries = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let key = PublicKey::read(reader)?;
                    let asset = Hash::read(reader)?;
                    entries.push((key, asset));
                }

                let page = Option::read(reader)?;
                if let Some(page_number) = &page {
                    if *page_number == 0 {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Invalid page number (0) in Step Response");
                        }
                        return Err(ReaderError::InvalidValue);
                    }
                }
                Self::UnoBalanceKeys(entries, page)
            }
            10 => {
                let len = reader.read_u16()?;
                if len > PRUNE_SAFETY_LIMIT as u16 + 1 {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Invalid blocks metadata response length: {}", len);
                    }
                    return Err(ReaderError::InvalidValue);
                }

                let mut blocks = IndexSet::with_capacity(len as usize);
                for _ in 0..len {
                    let metadata = BlockMetadata::read(reader)?;
                    if !blocks.insert(metadata) {
                        debug!("Duplicated block metadata in Step Response");
                        return Err(ReaderError::InvalidValue);
                    }
                }

                Self::BlocksMetadata(blocks)
            }
            id => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Received invalid value for StepResponse: {}", id);
                }
                return Err(ReaderError::InvalidValue);
            }
        })
    }

    fn write(&self, writer: &mut Writer) {
        match self {
            Self::ChainInfo(common_point, topoheight, stable_height, hash) => {
                writer.write_u8(0);
                common_point.write(writer);
                writer.write_u64(topoheight);
                writer.write_u64(stable_height);
                writer.write_hash(hash);
            }
            Self::Assets(assets, page) => {
                writer.write_u8(1);
                assets.write(writer);
                page.write(writer);
            }
            Self::AssetsSupply(supply) => {
                writer.write_u8(11);
                supply.write(writer);
            }
            Self::Keys(keys, page) => {
                writer.write_u8(2);
                keys.write(writer);
                page.write(writer);
            }
            Self::KeyBalances(keys, page) => {
                writer.write_u8(3);
                keys.write(writer);
                page.write(writer);
            }
            Self::SpendableBalances(balances, page) => {
                writer.write_u8(4);
                balances.write(writer);
                page.write(writer);
            }
            Self::Accounts(nonces) => {
                writer.write_u8(5);
                nonces.write(writer);
            }
            Self::Contracts(contracts, page) => {
                writer.write_u8(6);
                contracts.write(writer);
                page.write(writer);
            }
            Self::ContractModule(metadata) => {
                writer.write_u8(7);
                metadata.write(writer);
            }
            Self::ContractBalances(assets, page) => {
                writer.write_u8(8);
                assets.write(writer);
                page.write(writer);
            }
            Self::ContractStores(entries, page) => {
                writer.write_u8(9);
                entries.write(writer);
                page.write(writer);
            }
            Self::ContractsExecutions(executions, page) => {
                writer.write_u8(12);
                executions.write(writer);
                page.write(writer);
            }
            Self::KycData(entries, page) => {
                writer.write_u8(13);
                entries.write(writer);
                page.write(writer);
            }
            Self::Committees(entries, page) => {
                writer.write_u8(14);
                entries.write(writer);
                page.write(writer);
            }
            Self::GlobalCommittee(id) => {
                writer.write_u8(15);
                id.write(writer);
            }
            Self::NftCollections(entries, page) => {
                writer.write_u8(16);
                entries.write(writer);
                page.write(writer);
            }
            Self::NftTokens(collection_id, tokens, page) => {
                writer.write_u8(17);
                collection_id.write(writer);
                writer.write_u16(tokens.len() as u16);
                for (token_id, nft) in tokens {
                    writer.write_u64(token_id);
                    nft.write(writer);
                }
                page.write(writer);
            }
            Self::NftOwnership(collection_id, ownership, page) => {
                writer.write_u8(18);
                collection_id.write(writer);
                writer.write_u16(ownership.len() as u16);
                for (token_id, owner) in ownership {
                    writer.write_u64(token_id);
                    owner.write(writer);
                }
                page.write(writer);
            }
            Self::EscrowAccounts(entries, page) => {
                writer.write_u8(19);
                entries.write(writer);
                page.write(writer);
            }
            Self::ArbitrationData(entries, page) => {
                writer.write_u8(20);
                entries.write(writer);
                page.write(writer);
            }
            Self::ArbiterAccounts(entries, page) => {
                writer.write_u8(21);
                entries.write(writer);
                page.write(writer);
            }
            Self::TnsNames(entries, page) => {
                writer.write_u8(22);
                entries.write(writer);
                page.write(writer);
            }
            Self::EnergyData(entries) => {
                writer.write_u8(23);
                entries.write(writer);
            }
            Self::ReferralRecords(entries, page) => {
                writer.write_u8(24);
                entries.write(writer);
                page.write(writer);
            }
            Self::UnoBalances(entries, page) => {
                writer.write_u8(25);
                entries.write(writer);
                page.write(writer);
            }
            Self::AgentData(entries, page) => {
                writer.write_u8(26);
                entries.write(writer);
                page.write(writer);
            }
            Self::A2aNonces(entries, page) => {
                writer.write_u8(27);
                writer.write_u16(entries.len() as u16);
                for (nonce_bytes, timestamp) in entries {
                    writer.write_u16(nonce_bytes.len() as u16);
                    writer.write_bytes(nonce_bytes);
                    writer.write_u64(timestamp);
                }
                page.write(writer);
            }
            Self::ContractAssets(entries, page) => {
                writer.write_u8(28);
                writer.write_u16(entries.len() as u16);
                for (key, value) in entries {
                    key.write(writer);
                    // Write length prefix so ContractAssetData can be read with sub-reader
                    writer.write_u32(&(value.size() as u32));
                    value.write(writer);
                }
                page.write(writer);
            }
            Self::UnoBalanceKeys(entries, page) => {
                writer.write_u8(29);
                writer.write_u16(entries.len() as u16);
                for (key, asset) in entries {
                    key.write(writer);
                    asset.write(writer);
                }
                page.write(writer);
            }
            Self::BlocksMetadata(blocks) => {
                writer.write_u8(10);
                blocks.write(writer);
            }
        };
    }

    fn size(&self) -> usize {
        let size = match self {
            Self::ChainInfo(common_point, topoheight, stable_height, hash) => {
                common_point.size() + topoheight.size() + stable_height.size() + hash.size()
            }
            Self::Assets(assets, page) => assets.size() + page.size(),
            Self::AssetsSupply(supply) => supply.size(),
            Self::Keys(keys, page) => keys.size() + page.size(),
            Self::KeyBalances(keys, page) => keys.size() + page.size(),
            Self::SpendableBalances(balances, page) => balances.size() + page.size(),
            Self::Accounts(nonces) => nonces.size(),
            Self::Contracts(contracts, page) => contracts.size() + page.size(),
            Self::ContractModule(metadata) => metadata.size(),
            Self::ContractBalances(assets, page) => assets.size() + page.size(),
            Self::ContractStores(entries, page) => entries.size() + page.size(),
            Self::ContractsExecutions(executions, page) => executions.size() + page.size(),
            Self::KycData(entries, page) => entries.size() + page.size(),
            Self::Committees(entries, page) => entries.size() + page.size(),
            Self::GlobalCommittee(id) => id.size(),
            Self::NftCollections(entries, page) => entries.size() + page.size(),
            Self::NftTokens(collection_id, tokens, page) => {
                collection_id.size()
                    + 2
                    + tokens
                        .iter()
                        .map(|(id, nft)| id.size() + nft.size())
                        .sum::<usize>()
                    + page.size()
            }
            Self::NftOwnership(collection_id, ownership, page) => {
                collection_id.size()
                    + 2
                    + ownership
                        .iter()
                        .map(|(id, owner)| id.size() + owner.size())
                        .sum::<usize>()
                    + page.size()
            }
            Self::EscrowAccounts(entries, page) => entries.size() + page.size(),
            Self::ArbitrationData(entries, page) => entries.size() + page.size(),
            Self::ArbiterAccounts(entries, page) => entries.size() + page.size(),
            Self::TnsNames(entries, page) => entries.size() + page.size(),
            Self::EnergyData(entries) => entries.size(),
            Self::ReferralRecords(entries, page) => entries.size() + page.size(),
            Self::UnoBalances(entries, page) => entries.size() + page.size(),
            Self::AgentData(entries, page) => entries.size() + page.size(),
            Self::A2aNonces(entries, page) => {
                2 + entries
                    .iter()
                    .map(|(nonce_bytes, _)| 2 + nonce_bytes.len() + 8)
                    .sum::<usize>()
                    + page.size()
            }
            Self::ContractAssets(entries, page) => {
                2 + entries
                    .iter()
                    .map(|(key, value)| key.size() + 4 + value.size())
                    .sum::<usize>()
                    + page.size()
            }
            Self::UnoBalanceKeys(entries, page) => {
                2 + entries
                    .iter()
                    .map(|(key, asset)| key.size() + asset.size())
                    .sum::<usize>()
                    + page.size()
            }
            Self::BlocksMetadata(blocks) => blocks.size(),
        };
        // 1 for the id
        size + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::contract::ScheduledExecutionKind;
    use tos_common::crypto::Hash;

    // Helper: create a deterministic test hash from a byte seed
    fn test_hash(seed: u8) -> Hash {
        Hash::new([seed; 32])
    }

    // Helper: serialize and deserialize a StepRequest, returning the deserialized result
    fn round_trip_request(request: &StepRequest) -> StepRequest<'static> {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            request.write(&mut writer);
        }
        let mut reader = Reader::new(&bytes);
        StepRequest::read(&mut reader).expect("Failed to deserialize StepRequest")
    }

    // Helper: serialize and deserialize a StepResponse, returning the deserialized result
    fn round_trip_response(response: &StepResponse) -> StepResponse {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            response.write(&mut writer);
        }
        let mut reader = Reader::new(&bytes);
        StepResponse::read(&mut reader).expect("Failed to deserialize StepResponse")
    }

    // Helper: verify that size() matches actual serialized bytes
    fn verify_size_request(request: &StepRequest) {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            request.write(&mut writer);
        }
        assert_eq!(
            request.size(),
            bytes.len(),
            "StepRequest::size() mismatch for {:?}",
            request.kind()
        );
    }

    // Helper: verify that size() matches actual serialized bytes
    fn verify_size_response(response: &StepResponse) {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            response.write(&mut writer);
        }
        assert_eq!(
            response.size(),
            bytes.len(),
            "StepResponse::size() mismatch for {:?}",
            response.kind()
        );
    }

    // Helper: create a test ScheduledExecution
    fn test_scheduled_execution(contract_seed: u8, topo: u64) -> ScheduledExecution {
        ScheduledExecution::new_offercall(
            test_hash(contract_seed),
            1,                                          // chunk_id
            vec![0x01, 0x02],                           // input_data
            100_000,                                    // max_gas
            1000,                                       // offer_amount
            test_hash(contract_seed.wrapping_add(100)), // scheduler_contract
            ScheduledExecutionKind::TopoHeight(topo),
            topo.saturating_sub(10), // registration_topoheight
        )
    }

    // ========================================================================
    // StepRequest::AssetsSupply tests
    // ========================================================================

    #[test]
    fn test_request_assets_supply_round_trip() {
        let mut assets = IndexSet::new();
        assets.insert(test_hash(1));
        assets.insert(test_hash(2));
        assets.insert(test_hash(3));

        let request = StepRequest::AssetsSupply(100, Cow::Owned(assets.clone()));
        let decoded = round_trip_request(&request);

        match decoded {
            StepRequest::AssetsSupply(topo, decoded_assets) => {
                assert_eq!(topo, 100);
                assert_eq!(decoded_assets.into_owned(), assets);
            }
            _ => panic!("Expected AssetsSupply variant"),
        }
    }

    #[test]
    fn test_request_assets_supply_single_asset() {
        let mut assets = IndexSet::new();
        assets.insert(test_hash(42));

        let request = StepRequest::AssetsSupply(0, Cow::Owned(assets.clone()));
        let decoded = round_trip_request(&request);

        match decoded {
            StepRequest::AssetsSupply(topo, decoded_assets) => {
                assert_eq!(topo, 0);
                assert_eq!(decoded_assets.into_owned().len(), 1);
            }
            _ => panic!("Expected AssetsSupply variant"),
        }
    }

    #[test]
    fn test_request_assets_supply_max_items() {
        let mut assets = IndexSet::new();
        for i in 0..MAX_ITEMS_PER_PAGE {
            // Use i split across multiple bytes to avoid u8 wrapping
            let mut bytes = [0u8; 32];
            bytes[0] = (i & 0xFF) as u8;
            bytes[1] = ((i >> 8) & 0xFF) as u8;
            assets.insert(Hash::new(bytes));
        }
        assert_eq!(assets.len(), MAX_ITEMS_PER_PAGE);

        let request = StepRequest::AssetsSupply(999, Cow::Owned(assets.clone()));
        let decoded = round_trip_request(&request);

        match decoded {
            StepRequest::AssetsSupply(topo, decoded_assets) => {
                assert_eq!(topo, 999);
                assert_eq!(decoded_assets.into_owned().len(), MAX_ITEMS_PER_PAGE);
            }
            _ => panic!("Expected AssetsSupply variant"),
        }
    }

    #[test]
    fn test_request_assets_supply_size_consistency() {
        let mut assets = IndexSet::new();
        assets.insert(test_hash(1));
        assets.insert(test_hash(2));

        let request = StepRequest::AssetsSupply(50, Cow::Owned(assets));
        verify_size_request(&request);
    }

    #[test]
    fn test_request_assets_supply_empty_rejected() {
        // Empty assets should be rejected during deserialization (len == 0 check)
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(11); // AssetsSupply ID
            writer.write_u64(&0u64); // topoheight
            writer.write_u16(0); // len = 0 (invalid)
        }
        let mut reader = Reader::new(&bytes);
        let result = StepRequest::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_assets_supply_too_many_rejected() {
        // More than MAX_ITEMS_PER_PAGE should be rejected
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(11); // AssetsSupply ID
            writer.write_u64(&0u64); // topoheight
            writer.write_u16((MAX_ITEMS_PER_PAGE + 1) as u16); // len > max
        }
        let mut reader = Reader::new(&bytes);
        let result = StepRequest::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_assets_supply_duplicate_rejected() {
        // Duplicate asset hashes should be rejected
        let hash = test_hash(1);
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(11); // AssetsSupply ID
            writer.write_u64(&0u64); // topoheight
            writer.write_u16(2); // len = 2
            hash.write(&mut writer); // first hash
            hash.write(&mut writer); // duplicate hash
        }
        let mut reader = Reader::new(&bytes);
        let result = StepRequest::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_assets_supply_kind() {
        let request = StepRequest::AssetsSupply(0, Cow::Owned(IndexSet::new()));
        assert_eq!(request.kind(), StepKind::Assets);
    }

    #[test]
    fn test_request_assets_supply_topoheight() {
        let request = StepRequest::AssetsSupply(12345, Cow::Owned(IndexSet::new()));
        assert_eq!(request.get_requested_topoheight(), Some(12345));
    }

    // ========================================================================
    // StepRequest::ContractsExecutions tests
    // ========================================================================

    #[test]
    fn test_request_contracts_executions_round_trip() {
        let request = StepRequest::ContractsExecutions(10, 100, None);
        let decoded = round_trip_request(&request);

        match decoded {
            StepRequest::ContractsExecutions(min, max, page) => {
                assert_eq!(min, 10);
                assert_eq!(max, 100);
                assert_eq!(page, None);
            }
            _ => panic!("Expected ContractsExecutions variant"),
        }
    }

    #[test]
    fn test_request_contracts_executions_with_page() {
        let request = StepRequest::ContractsExecutions(0, 500, Some(3));
        let decoded = round_trip_request(&request);

        match decoded {
            StepRequest::ContractsExecutions(min, max, page) => {
                assert_eq!(min, 0);
                assert_eq!(max, 500);
                assert_eq!(page, Some(3));
            }
            _ => panic!("Expected ContractsExecutions variant"),
        }
    }

    #[test]
    fn test_request_contracts_executions_invalid_range() {
        // min > max should be rejected
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(12); // ContractsExecutions ID
            writer.write_u64(&100u64); // min = 100
            writer.write_u64(&50u64); // max = 50 (invalid: min > max)
            None::<u64>.write(&mut writer); // no page
        }
        let mut reader = Reader::new(&bytes);
        let result = StepRequest::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_contracts_executions_page_zero_rejected() {
        // page = 0 should be rejected (pages are 1-indexed for "next page")
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(12); // ContractsExecutions ID
            writer.write_u64(&0u64); // min
            writer.write_u64(&100u64); // max
            Some(0u64).write(&mut writer); // page = 0 (invalid)
        }
        let mut reader = Reader::new(&bytes);
        let result = StepRequest::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_contracts_executions_size_consistency() {
        let request = StepRequest::ContractsExecutions(0, 100, Some(2));
        verify_size_request(&request);

        let request_no_page = StepRequest::ContractsExecutions(10, 500, None);
        verify_size_request(&request_no_page);
    }

    #[test]
    fn test_request_contracts_executions_kind() {
        let request = StepRequest::ContractsExecutions(0, 100, None);
        assert_eq!(request.kind(), StepKind::Contracts);
    }

    #[test]
    fn test_request_contracts_executions_topoheight() {
        let request = StepRequest::ContractsExecutions(10, 500, None);
        assert_eq!(request.get_requested_topoheight(), Some(500));
    }

    // ========================================================================
    // StepResponse::AssetsSupply tests
    // ========================================================================

    #[test]
    fn test_response_assets_supply_round_trip() {
        let supply = vec![Some(1000), None, Some(5000), Some(0)];
        let response = StepResponse::AssetsSupply(supply.clone());
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::AssetsSupply(decoded_supply) => {
                assert_eq!(decoded_supply, supply);
            }
            _ => panic!("Expected AssetsSupply response variant"),
        }
    }

    #[test]
    fn test_response_assets_supply_empty() {
        let response = StepResponse::AssetsSupply(vec![]);
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::AssetsSupply(decoded_supply) => {
                assert!(decoded_supply.is_empty());
            }
            _ => panic!("Expected AssetsSupply response variant"),
        }
    }

    #[test]
    fn test_response_assets_supply_all_none() {
        let supply = vec![None, None, None];
        let response = StepResponse::AssetsSupply(supply.clone());
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::AssetsSupply(decoded_supply) => {
                assert_eq!(decoded_supply, supply);
            }
            _ => panic!("Expected AssetsSupply response variant"),
        }
    }

    #[test]
    fn test_response_assets_supply_all_some() {
        let supply = vec![Some(100), Some(200), Some(u64::MAX)];
        let response = StepResponse::AssetsSupply(supply.clone());
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::AssetsSupply(decoded_supply) => {
                assert_eq!(decoded_supply, supply);
            }
            _ => panic!("Expected AssetsSupply response variant"),
        }
    }

    #[test]
    fn test_response_assets_supply_max_items() {
        let supply: Vec<Option<u64>> = (0..MAX_ITEMS_PER_PAGE as u64).map(Some).collect();
        let response = StepResponse::AssetsSupply(supply.clone());
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::AssetsSupply(decoded_supply) => {
                assert_eq!(decoded_supply.len(), MAX_ITEMS_PER_PAGE);
                assert_eq!(decoded_supply, supply);
            }
            _ => panic!("Expected AssetsSupply response variant"),
        }
    }

    #[test]
    fn test_response_assets_supply_too_many_rejected() {
        // Manually construct a response with len > MAX_ITEMS_PER_PAGE
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(11); // AssetsSupply response ID
            writer.write_u16((MAX_ITEMS_PER_PAGE + 1) as u16); // len > max
        }
        let mut reader = Reader::new(&bytes);
        let result = StepResponse::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_response_assets_supply_size_consistency() {
        let response = StepResponse::AssetsSupply(vec![Some(100), None, Some(200)]);
        verify_size_response(&response);

        let response_empty = StepResponse::AssetsSupply(vec![]);
        verify_size_response(&response_empty);
    }

    #[test]
    fn test_response_assets_supply_kind() {
        let response = StepResponse::AssetsSupply(vec![]);
        assert_eq!(response.kind(), StepKind::Assets);
    }

    // ========================================================================
    // StepResponse::ContractsExecutions tests
    // ========================================================================

    #[test]
    fn test_response_contracts_executions_round_trip() {
        let executions = vec![
            test_scheduled_execution(1, 100),
            test_scheduled_execution(2, 200),
        ];
        let response = StepResponse::ContractsExecutions(executions, None);
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::ContractsExecutions(decoded_execs, page) => {
                assert_eq!(decoded_execs.len(), 2);
                assert_eq!(page, None);
                assert_eq!(decoded_execs[0].contract, test_hash(1));
                assert_eq!(decoded_execs[1].contract, test_hash(2));
            }
            _ => panic!("Expected ContractsExecutions response variant"),
        }
    }

    #[test]
    fn test_response_contracts_executions_with_page() {
        let executions = vec![test_scheduled_execution(1, 50)];
        let response = StepResponse::ContractsExecutions(executions, Some(2));
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::ContractsExecutions(decoded_execs, page) => {
                assert_eq!(decoded_execs.len(), 1);
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected ContractsExecutions response variant"),
        }
    }

    #[test]
    fn test_response_contracts_executions_empty() {
        let response = StepResponse::ContractsExecutions(vec![], None);
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::ContractsExecutions(decoded_execs, page) => {
                assert!(decoded_execs.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected ContractsExecutions response variant"),
        }
    }

    #[test]
    fn test_response_contracts_executions_block_end_kind() {
        let mut exec = test_scheduled_execution(5, 300);
        exec.kind = ScheduledExecutionKind::BlockEnd;
        // Recompute hash with new kind
        exec.hash = ScheduledExecution::compute_hash(
            &exec.contract,
            &exec.kind,
            exec.registration_topoheight,
            exec.chunk_id,
        );

        let response = StepResponse::ContractsExecutions(vec![exec], None);
        let decoded = round_trip_response(&response);

        match decoded {
            StepResponse::ContractsExecutions(decoded_execs, _) => {
                assert_eq!(decoded_execs.len(), 1);
                assert_eq!(decoded_execs[0].kind, ScheduledExecutionKind::BlockEnd);
            }
            _ => panic!("Expected ContractsExecutions response variant"),
        }
    }

    #[test]
    fn test_response_contracts_executions_too_many_rejected() {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(12); // ContractsExecutions response ID
            writer.write_u16((MAX_ITEMS_PER_PAGE + 1) as u16); // len > max
        }
        let mut reader = Reader::new(&bytes);
        let result = StepResponse::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_response_contracts_executions_page_zero_rejected() {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(12); // ContractsExecutions response ID
            writer.write_u16(0); // empty list
            Some(0u64).write(&mut writer); // page = 0 (invalid)
        }
        let mut reader = Reader::new(&bytes);
        let result = StepResponse::read(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_response_contracts_executions_size_consistency() {
        let response =
            StepResponse::ContractsExecutions(vec![test_scheduled_execution(1, 100)], Some(5));
        verify_size_response(&response);

        let response_empty = StepResponse::ContractsExecutions(vec![], None);
        verify_size_response(&response_empty);
    }

    #[test]
    fn test_response_contracts_executions_kind() {
        let response = StepResponse::ContractsExecutions(vec![], None);
        assert_eq!(response.kind(), StepKind::Contracts);
    }

    // ========================================================================
    // Protocol ID consistency tests
    // ========================================================================

    #[test]
    fn test_request_wire_ids() {
        // Verify wire IDs match expected values for protocol compatibility
        let test_cases: Vec<(StepRequest, u8)> = vec![
            (
                StepRequest::AssetsSupply(
                    0,
                    Cow::Owned({
                        let mut s = IndexSet::new();
                        s.insert(test_hash(1));
                        s
                    }),
                ),
                11,
            ),
            (StepRequest::ContractsExecutions(0, 100, None), 12),
        ];

        for (request, expected_id) in test_cases {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                request.write(&mut writer);
            }
            assert_eq!(
                bytes[0],
                expected_id,
                "Wire ID mismatch for request {:?}",
                request.kind()
            );
        }
    }

    #[test]
    fn test_response_wire_ids() {
        // Verify response wire IDs
        let test_cases: Vec<(StepResponse, u8)> = vec![
            (StepResponse::AssetsSupply(vec![Some(1)]), 11),
            (StepResponse::ContractsExecutions(vec![], None), 12),
        ];

        for (response, expected_id) in test_cases {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                response.write(&mut writer);
            }
            assert_eq!(
                bytes[0],
                expected_id,
                "Wire ID mismatch for response {:?}",
                response.kind()
            );
        }
    }

    #[test]
    fn test_invalid_step_id_rejected() {
        // An unknown step ID should be rejected
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes);
            writer.write_u8(255); // invalid ID
        }
        let mut reader = Reader::new(&bytes);
        assert!(StepRequest::read(&mut reader).is_err());

        let mut reader = Reader::new(&bytes);
        assert!(StepResponse::read(&mut reader).is_err());
    }

    // ========================================================================
    // Positional matching verification (AssetsSupply request/response contract)
    // ========================================================================

    #[test]
    fn test_assets_supply_positional_contract() {
        // Verify that response order matches request order
        // This test documents the positional matching behavior:
        // request assets [A, B, C] -> response supplies [supply_A, supply_B, supply_C]
        let assets: IndexSet<Hash> = (0..5u8).map(test_hash).collect();
        let supply: Vec<Option<u64>> = vec![Some(100), None, Some(300), Some(400), None];

        // Request preserves insertion order
        let request = StepRequest::AssetsSupply(50, Cow::Owned(assets.clone()));
        let decoded_req = round_trip_request(&request);
        let decoded_assets = match decoded_req {
            StepRequest::AssetsSupply(_, a) => a.into_owned(),
            _ => panic!("Expected AssetsSupply"),
        };

        // Response preserves index order
        let response = StepResponse::AssetsSupply(supply.clone());
        let decoded_resp = round_trip_response(&response);
        let decoded_supply = match decoded_resp {
            StepResponse::AssetsSupply(s) => s,
            _ => panic!("Expected AssetsSupply"),
        };

        // Zip should pair correctly
        let pairs: Vec<(Hash, Option<u64>)> = decoded_assets
            .into_iter()
            .zip(decoded_supply.into_iter())
            .collect();

        assert_eq!(pairs.len(), 5);
        assert_eq!(pairs[0], (test_hash(0), Some(100)));
        assert_eq!(pairs[1], (test_hash(1), None));
        assert_eq!(pairs[2], (test_hash(2), Some(300)));
        assert_eq!(pairs[3], (test_hash(3), Some(400)));
        assert_eq!(pairs[4], (test_hash(4), None));
    }

    // ========================================================================
    // Other StepRequest variants (regression coverage)
    // ========================================================================

    #[test]
    fn test_request_assets_round_trip() {
        let request = StepRequest::Assets(10, 100, Some(2));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::Assets(min, max, page) => {
                assert_eq!(min, 10);
                assert_eq!(max, 100);
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected Assets variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_keys_round_trip() {
        let request = StepRequest::Keys(0, 500, None);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::Keys(min, max, page) => {
                assert_eq!(min, 0);
                assert_eq!(max, 500);
                assert_eq!(page, None);
            }
            _ => panic!("Expected Keys variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_contracts_round_trip() {
        let request = StepRequest::Contracts(50, 200, Some(1));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::Contracts(min, max, page) => {
                assert_eq!(min, 50);
                assert_eq!(max, 200);
                assert_eq!(page, Some(1));
            }
            _ => panic!("Expected Contracts variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_contract_module_round_trip() {
        let request = StepRequest::ContractModule(10, 100, Cow::Owned(test_hash(5)));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ContractModule(min, max, hash) => {
                assert_eq!(min, 10);
                assert_eq!(max, 100);
                assert_eq!(*hash, test_hash(5));
            }
            _ => panic!("Expected ContractModule variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_contract_balances_round_trip() {
        let request = StepRequest::ContractBalances(Cow::Owned(test_hash(3)), 50, Some(1));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ContractBalances(hash, topo, page) => {
                assert_eq!(*hash, test_hash(3));
                assert_eq!(topo, 50);
                assert_eq!(page, Some(1));
            }
            _ => panic!("Expected ContractBalances variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_contract_stores_round_trip() {
        let request = StepRequest::ContractStores(Cow::Owned(test_hash(7)), 200, None);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ContractStores(hash, topo, page) => {
                assert_eq!(*hash, test_hash(7));
                assert_eq!(topo, 200);
                assert_eq!(page, None);
            }
            _ => panic!("Expected ContractStores variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_blocks_metadata_round_trip() {
        let request = StepRequest::BlocksMetadata(12345);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::BlocksMetadata(topo) => {
                assert_eq!(topo, 12345);
            }
            _ => panic!("Expected BlocksMetadata variant"),
        }
        verify_size_request(&request);
    }

    // ========================================================================
    // TOS Extension StepRequest tests
    // ========================================================================

    #[test]
    fn test_request_kyc_data_round_trip() {
        let request = StepRequest::KycData(Some(5));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::KycData(page) => assert_eq!(page, Some(5)),
            _ => panic!("Expected KycData variant"),
        }
        verify_size_request(&request);

        let request_none = StepRequest::KycData(None);
        let decoded = round_trip_request(&request_none);
        match decoded {
            StepRequest::KycData(page) => assert_eq!(page, None),
            _ => panic!("Expected KycData variant"),
        }
        verify_size_request(&request_none);
    }

    #[test]
    fn test_request_committees_round_trip() {
        let request = StepRequest::Committees(Some(3));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::Committees(page) => assert_eq!(page, Some(3)),
            _ => panic!("Expected Committees variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_global_committee_round_trip() {
        let request = StepRequest::GlobalCommittee;
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::GlobalCommittee => {}
            _ => panic!("Expected GlobalCommittee variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_nft_collections_round_trip() {
        let request = StepRequest::NftCollections(500, Some(2));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::NftCollections(topo, page) => {
                assert_eq!(topo, 500);
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected NftCollections variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_nft_tokens_round_trip() {
        let request = StepRequest::NftTokens(Cow::Owned(test_hash(10)), 300, Some(1));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::NftTokens(collection, topo, page) => {
                assert_eq!(*collection, test_hash(10));
                assert_eq!(topo, 300);
                assert_eq!(page, Some(1));
            }
            _ => panic!("Expected NftTokens variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_nft_ownership_round_trip() {
        let request = StepRequest::NftOwnership(Cow::Owned(test_hash(20)), 400, None);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::NftOwnership(collection, topo, page) => {
                assert_eq!(*collection, test_hash(20));
                assert_eq!(topo, 400);
                assert_eq!(page, None);
            }
            _ => panic!("Expected NftOwnership variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_escrow_accounts_round_trip() {
        let request = StepRequest::EscrowAccounts(Some(7));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::EscrowAccounts(page) => assert_eq!(page, Some(7)),
            _ => panic!("Expected EscrowAccounts variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_arbitration_data_round_trip() {
        let request = StepRequest::ArbitrationData(None);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ArbitrationData(page) => assert_eq!(page, None),
            _ => panic!("Expected ArbitrationData variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_arbiter_accounts_round_trip() {
        let request = StepRequest::ArbiterAccounts(Some(1));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ArbiterAccounts(page) => assert_eq!(page, Some(1)),
            _ => panic!("Expected ArbiterAccounts variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_tns_names_round_trip() {
        let request = StepRequest::TnsNames(Some(10));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::TnsNames(page) => assert_eq!(page, Some(10)),
            _ => panic!("Expected TnsNames variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_energy_data_round_trip() {
        let key1 = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let key2 = PublicKey::from_bytes(&[2u8; 32]).unwrap();
        let keys = vec![key1, key2];
        let request = StepRequest::EnergyData(Cow::Owned(keys.clone()), 750);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::EnergyData(decoded_keys, topo) => {
                assert_eq!(decoded_keys.len(), 2);
                assert_eq!(topo, 750);
            }
            _ => panic!("Expected EnergyData variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_referral_records_round_trip() {
        let request = StepRequest::ReferralRecords(Some(4));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ReferralRecords(page) => assert_eq!(page, Some(4)),
            _ => panic!("Expected ReferralRecords variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_uno_balances_round_trip() {
        let key = PublicKey::from_bytes(&[5u8; 32]).unwrap();
        let request =
            StepRequest::UnoBalances(Cow::Owned(key), Cow::Owned(test_hash(99)), 800, Some(2));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::UnoBalances(k, asset, topo, page) => {
                assert_eq!(*k, PublicKey::from_bytes(&[5u8; 32]).unwrap());
                assert_eq!(*asset, test_hash(99));
                assert_eq!(topo, 800);
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected UnoBalances variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_agent_data_round_trip() {
        let request = StepRequest::AgentData(None);
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::AgentData(page) => assert_eq!(page, None),
            _ => panic!("Expected AgentData variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_a2a_nonces_round_trip() {
        let request = StepRequest::A2aNonces(Some(6));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::A2aNonces(page) => assert_eq!(page, Some(6)),
            _ => panic!("Expected A2aNonces variant"),
        }
        verify_size_request(&request);
    }

    #[test]
    fn test_request_contract_assets_round_trip() {
        let request = StepRequest::ContractAssets(Some(8));
        let decoded = round_trip_request(&request);
        match decoded {
            StepRequest::ContractAssets(page) => assert_eq!(page, Some(8)),
            _ => panic!("Expected ContractAssets variant"),
        }
        verify_size_request(&request);
    }

    // ========================================================================
    // TOS Extension StepResponse tests
    // ========================================================================

    #[test]
    fn test_response_kyc_data_empty() {
        let response = StepResponse::KycData(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::KycData(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected KycData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_kyc_data_with_entries() {
        let key = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let kyc = KycData::new(31, 1000, test_hash(1));
        let mut entries = IndexMap::new();
        entries.insert(key, kyc);

        let response = StepResponse::KycData(entries, Some(2));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::KycData(entries, page) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected KycData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_kyc_data_kind() {
        let response = StepResponse::KycData(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Kyc);
    }

    #[test]
    fn test_response_committees_empty() {
        let response = StepResponse::Committees(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::Committees(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected Committees variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_committees_kind() {
        let response = StepResponse::Committees(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Committees);
    }

    #[test]
    fn test_response_global_committee_some() {
        let response = StepResponse::GlobalCommittee(Some(test_hash(42)));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::GlobalCommittee(id) => {
                assert_eq!(id, Some(test_hash(42)));
            }
            _ => panic!("Expected GlobalCommittee variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_global_committee_none() {
        let response = StepResponse::GlobalCommittee(None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::GlobalCommittee(id) => {
                assert_eq!(id, None);
            }
            _ => panic!("Expected GlobalCommittee variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_global_committee_kind() {
        let response = StepResponse::GlobalCommittee(None);
        assert_eq!(response.kind(), StepKind::Committees);
    }

    #[test]
    fn test_response_nft_collections_empty() {
        let response = StepResponse::NftCollections(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::NftCollections(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected NftCollections variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_nft_collections_kind() {
        let response = StepResponse::NftCollections(IndexMap::new(), Some(3));
        assert_eq!(response.kind(), StepKind::Nft);
    }

    #[test]
    fn test_response_nft_tokens_empty() {
        let response = StepResponse::NftTokens(test_hash(5), IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::NftTokens(collection_id, tokens, page) => {
                assert_eq!(collection_id, test_hash(5));
                assert!(tokens.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected NftTokens variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_nft_tokens_kind() {
        let response = StepResponse::NftTokens(test_hash(1), IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Nft);
    }

    #[test]
    fn test_response_nft_ownership_empty() {
        let response = StepResponse::NftOwnership(test_hash(7), IndexMap::new(), Some(1));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::NftOwnership(collection_id, ownership, page) => {
                assert_eq!(collection_id, test_hash(7));
                assert!(ownership.is_empty());
                assert_eq!(page, Some(1));
            }
            _ => panic!("Expected NftOwnership variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_nft_ownership_with_entries() {
        let mut ownership = IndexMap::new();
        let owner = PublicKey::from_bytes(&[3u8; 32]).unwrap();
        ownership.insert(1u64, owner);
        ownership.insert(2u64, PublicKey::from_bytes(&[4u8; 32]).unwrap());

        let response = StepResponse::NftOwnership(test_hash(8), ownership, None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::NftOwnership(collection_id, entries, page) => {
                assert_eq!(collection_id, test_hash(8));
                assert_eq!(entries.len(), 2);
                assert_eq!(
                    *entries.get(&1u64).unwrap(),
                    PublicKey::from_bytes(&[3u8; 32]).unwrap()
                );
                assert_eq!(
                    *entries.get(&2u64).unwrap(),
                    PublicKey::from_bytes(&[4u8; 32]).unwrap()
                );
                assert_eq!(page, None);
            }
            _ => panic!("Expected NftOwnership variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_nft_ownership_kind() {
        let response = StepResponse::NftOwnership(test_hash(1), IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Nft);
    }

    #[test]
    fn test_response_escrow_accounts_empty() {
        let response = StepResponse::EscrowAccounts(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::EscrowAccounts(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected EscrowAccounts variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_escrow_accounts_kind() {
        let response = StepResponse::EscrowAccounts(IndexMap::new(), Some(5));
        assert_eq!(response.kind(), StepKind::Escrow);
    }

    #[test]
    fn test_response_arbitration_data_empty() {
        let response = StepResponse::ArbitrationData(Vec::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::ArbitrationData(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected ArbitrationData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_arbitration_data_kind() {
        let response = StepResponse::ArbitrationData(Vec::new(), None);
        assert_eq!(response.kind(), StepKind::Arbitration);
    }

    #[test]
    fn test_response_arbiter_accounts_empty() {
        let response = StepResponse::ArbiterAccounts(IndexMap::new(), Some(2));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::ArbiterAccounts(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected ArbiterAccounts variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_arbiter_accounts_kind() {
        let response = StepResponse::ArbiterAccounts(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Arbitration);
    }

    #[test]
    fn test_response_tns_names_empty() {
        let response = StepResponse::TnsNames(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::TnsNames(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected TnsNames variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_tns_names_with_entries() {
        let mut entries = IndexMap::new();
        let owner = PublicKey::from_bytes(&[10u8; 32]).unwrap();
        entries.insert(test_hash(1), owner);

        let response = StepResponse::TnsNames(entries, Some(3));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::TnsNames(decoded_entries, page) => {
                assert_eq!(decoded_entries.len(), 1);
                assert_eq!(
                    *decoded_entries.get(&test_hash(1)).unwrap(),
                    PublicKey::from_bytes(&[10u8; 32]).unwrap()
                );
                assert_eq!(page, Some(3));
            }
            _ => panic!("Expected TnsNames variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_tns_names_kind() {
        let response = StepResponse::TnsNames(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Tns);
    }

    #[test]
    fn test_response_energy_data_empty() {
        let response = StepResponse::EnergyData(Vec::new());
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::EnergyData(entries) => {
                assert!(entries.is_empty());
            }
            _ => panic!("Expected EnergyData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_energy_data_with_some_none() {
        let energy = EnergyResource::new();
        let entries = vec![Some(energy), None, None];

        let response = StepResponse::EnergyData(entries);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::EnergyData(decoded_entries) => {
                assert_eq!(decoded_entries.len(), 3);
                assert!(decoded_entries[0].is_some());
                assert!(decoded_entries[1].is_none());
                assert!(decoded_entries[2].is_none());
            }
            _ => panic!("Expected EnergyData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_energy_data_kind() {
        let response = StepResponse::EnergyData(Vec::new());
        assert_eq!(response.kind(), StepKind::Energy);
    }

    #[test]
    fn test_response_referral_records_empty() {
        let response = StepResponse::ReferralRecords(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::ReferralRecords(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected ReferralRecords variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_referral_records_with_entry() {
        let key = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let referrer = PublicKey::from_bytes(&[2u8; 32]).unwrap();
        let record =
            ReferralRecord::new(key.clone(), Some(referrer), 100, test_hash(1), 1234567890);
        let mut entries = IndexMap::new();
        entries.insert(key, record);

        let response = StepResponse::ReferralRecords(entries, Some(1));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::ReferralRecords(decoded_entries, page) => {
                assert_eq!(decoded_entries.len(), 1);
                assert_eq!(page, Some(1));
            }
            _ => panic!("Expected ReferralRecords variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_referral_records_kind() {
        let response = StepResponse::ReferralRecords(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Referral);
    }

    #[test]
    fn test_response_uno_balances_empty() {
        let response = StepResponse::UnoBalances(Vec::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::UnoBalances(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected UnoBalances variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_uno_balances_with_data() {
        use tos_common::account::{BalanceType, CiphertextCache};
        use tos_common::crypto::elgamal::Ciphertext;

        let entries = vec![
            UnoBalance {
                topoheight: 100,
                output_balance: None,
                final_balance: CiphertextCache::Decompressed(Ciphertext::zero()),
                balance_type: BalanceType::Input,
            },
            UnoBalance {
                topoheight: 200,
                output_balance: Some(CiphertextCache::Decompressed(Ciphertext::zero())),
                final_balance: CiphertextCache::Decompressed(Ciphertext::zero()),
                balance_type: BalanceType::Both,
            },
        ];
        let response = StepResponse::UnoBalances(entries, Some(3));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::UnoBalances(entries, page) => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].topoheight, 100);
                assert_eq!(entries[0].balance_type, BalanceType::Input);
                assert!(entries[0].output_balance.is_none());
                assert_eq!(entries[1].topoheight, 200);
                assert_eq!(entries[1].balance_type, BalanceType::Both);
                assert!(entries[1].output_balance.is_some());
                assert_eq!(page, Some(3));
            }
            _ => panic!("Expected UnoBalances variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_uno_balances_kind() {
        let response = StepResponse::UnoBalances(Vec::new(), Some(4));
        assert_eq!(response.kind(), StepKind::UnoBalance);
    }

    #[test]
    fn test_response_agent_data_empty() {
        let response = StepResponse::AgentData(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::AgentData(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected AgentData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_agent_data_with_entry() {
        let key = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let meta = AgentAccountMeta {
            owner: key.clone(),
            controller: PublicKey::from_bytes(&[2u8; 32]).unwrap(),
            policy_hash: test_hash(10),
            status: 0,
            energy_pool: None,
            session_key_root: None,
        };
        let mut entries = IndexMap::new();
        entries.insert(key.clone(), meta);

        let response = StepResponse::AgentData(entries, Some(1));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::AgentData(decoded_entries, page) => {
                assert_eq!(decoded_entries.len(), 1);
                assert_eq!(page, Some(1));
                let decoded_meta = decoded_entries.get(&key).unwrap();
                assert_eq!(decoded_meta.status, 0);
                assert_eq!(decoded_meta.policy_hash, test_hash(10));
            }
            _ => panic!("Expected AgentData variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_agent_data_kind() {
        let response = StepResponse::AgentData(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::Agent);
    }

    #[test]
    fn test_response_a2a_nonces_empty() {
        let response = StepResponse::A2aNonces(Vec::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::A2aNonces(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected A2aNonces variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_a2a_nonces_with_entries() {
        let entries = vec![
            (vec![0x01, 0x02, 0x03], 1000u64),
            (vec![0xAA, 0xBB], 2000u64),
            (vec![], 3000u64),
        ];

        let response = StepResponse::A2aNonces(entries, Some(5));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::A2aNonces(decoded_entries, page) => {
                assert_eq!(decoded_entries.len(), 3);
                assert_eq!(decoded_entries[0].0, vec![0x01, 0x02, 0x03]);
                assert_eq!(decoded_entries[0].1, 1000);
                assert_eq!(decoded_entries[1].0, vec![0xAA, 0xBB]);
                assert_eq!(decoded_entries[1].1, 2000);
                assert_eq!(decoded_entries[2].0, Vec::<u8>::new());
                assert_eq!(decoded_entries[2].1, 3000);
                assert_eq!(page, Some(5));
            }
            _ => panic!("Expected A2aNonces variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_a2a_nonces_kind() {
        let response = StepResponse::A2aNonces(Vec::new(), None);
        assert_eq!(response.kind(), StepKind::A2aNonce);
    }

    #[test]
    fn test_response_contract_assets_empty() {
        let response = StepResponse::ContractAssets(IndexMap::new(), None);
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::ContractAssets(entries, page) => {
                assert!(entries.is_empty());
                assert_eq!(page, None);
            }
            _ => panic!("Expected ContractAssets variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_contract_assets_with_entry() {
        let asset_data = ContractAssetData::default();
        let mut entries = IndexMap::new();
        entries.insert(test_hash(50), asset_data);

        let response = StepResponse::ContractAssets(entries, Some(2));
        let decoded = round_trip_response(&response);
        match decoded {
            StepResponse::ContractAssets(decoded_entries, page) => {
                assert_eq!(decoded_entries.len(), 1);
                assert!(decoded_entries.contains_key(&test_hash(50)));
                assert_eq!(page, Some(2));
            }
            _ => panic!("Expected ContractAssets variant"),
        }
        verify_size_response(&response);
    }

    #[test]
    fn test_response_contract_assets_kind() {
        let response = StepResponse::ContractAssets(IndexMap::new(), None);
        assert_eq!(response.kind(), StepKind::ContractAsset);
    }

    // ========================================================================
    // TOS Extension page validation tests
    // ========================================================================

    #[test]
    fn test_response_tos_page_zero_rejected() {
        // Test that page=0 is rejected for TOS extension variants
        let test_cases: Vec<(&str, u8)> = vec![
            ("KycData", 13),
            ("Committees", 14),
            ("NftCollections", 16),
            ("EscrowAccounts", 19),
            ("ArbitrationData", 20),
            ("ArbiterAccounts", 21),
            ("TnsNames", 22),
            ("ReferralRecords", 24),
            ("UnoBalances", 25),
            ("AgentData", 26),
            ("A2aNonces", 27),
            ("ContractAssets", 28),
        ];

        for (name, id) in test_cases {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                writer.write_u8(id);
                writer.write_u16(0); // empty collection
                                     // page = Some(0) which is invalid
                1u8.write(&mut writer); // Some variant
                0u64.write(&mut writer); // page = 0
            }
            let mut reader = Reader::new(&bytes);
            let result = StepResponse::read(&mut reader);
            assert!(
                result.is_err(),
                "Expected page=0 to be rejected for {} (ID {})",
                name,
                id
            );
        }
    }

    #[test]
    fn test_response_tos_too_many_items_rejected() {
        // Test that more than MAX_ITEMS_PER_PAGE is rejected
        let test_cases: Vec<(&str, u8)> = vec![
            ("KycData", 13),
            ("Committees", 14),
            ("NftCollections", 16),
            ("EscrowAccounts", 19),
            ("ArbitrationData", 20),
            ("ArbiterAccounts", 21),
            ("TnsNames", 22),
            ("EnergyData", 23),
            ("ReferralRecords", 24),
            ("UnoBalances", 25),
            ("AgentData", 26),
            ("A2aNonces", 27),
            ("ContractAssets", 28),
        ];

        for (name, id) in test_cases {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                writer.write_u8(id);
                writer.write_u16((MAX_ITEMS_PER_PAGE + 1) as u16);
            }
            let mut reader = Reader::new(&bytes);
            let result = StepResponse::read(&mut reader);
            assert!(
                result.is_err(),
                "Expected too many items to be rejected for {} (ID {})",
                name,
                id
            );
        }
    }

    // ========================================================================
    // TOS Extension wire ID verification tests
    // ========================================================================

    #[test]
    fn test_request_tos_extension_wire_ids() {
        // Verify that each TOS extension request serializes with the correct wire ID
        let test_cases: Vec<(StepRequest, u8)> = vec![
            (StepRequest::KycData(None), 13),
            (StepRequest::Committees(None), 14),
            (StepRequest::GlobalCommittee, 15),
            (StepRequest::NftCollections(0, None), 16),
            (
                StepRequest::NftTokens(Cow::Owned(test_hash(1)), 0, None),
                17,
            ),
            (
                StepRequest::NftOwnership(Cow::Owned(test_hash(1)), 0, None),
                18,
            ),
            (StepRequest::EscrowAccounts(None), 19),
            (StepRequest::ArbitrationData(None), 20),
            (StepRequest::ArbiterAccounts(None), 21),
            (StepRequest::TnsNames(None), 22),
            (StepRequest::EnergyData(Cow::Owned(vec![]), 0), 23),
            (StepRequest::ReferralRecords(None), 24),
            (
                StepRequest::UnoBalances(
                    Cow::Owned(PublicKey::from_bytes(&[0u8; 32]).unwrap()),
                    Cow::Owned(test_hash(1)),
                    0,
                    None,
                ),
                25,
            ),
            (StepRequest::AgentData(None), 26),
            (StepRequest::A2aNonces(None), 27),
            (StepRequest::ContractAssets(None), 28),
        ];

        for (request, expected_id) in test_cases {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                request.write(&mut writer);
            }
            assert_eq!(
                bytes[0],
                expected_id,
                "Wire ID mismatch for {:?}",
                request.kind()
            );
        }
    }

    #[test]
    fn test_response_tos_extension_wire_ids() {
        // Verify that each TOS extension response serializes with the correct wire ID
        let test_cases: Vec<(StepResponse, u8)> = vec![
            (StepResponse::KycData(IndexMap::new(), None), 13),
            (StepResponse::Committees(IndexMap::new(), None), 14),
            (StepResponse::GlobalCommittee(None), 15),
            (StepResponse::NftCollections(IndexMap::new(), None), 16),
            (
                StepResponse::NftTokens(test_hash(1), IndexMap::new(), None),
                17,
            ),
            (
                StepResponse::NftOwnership(test_hash(1), IndexMap::new(), None),
                18,
            ),
            (StepResponse::EscrowAccounts(IndexMap::new(), None), 19),
            (StepResponse::ArbitrationData(Vec::new(), None), 20),
            (StepResponse::ArbiterAccounts(IndexMap::new(), None), 21),
            (StepResponse::TnsNames(IndexMap::new(), None), 22),
            (StepResponse::EnergyData(Vec::new()), 23),
            (StepResponse::ReferralRecords(IndexMap::new(), None), 24),
            (StepResponse::UnoBalances(Vec::new(), None), 25),
            (StepResponse::AgentData(IndexMap::new(), None), 26),
            (StepResponse::A2aNonces(Vec::new(), None), 27),
            (StepResponse::ContractAssets(IndexMap::new(), None), 28),
        ];

        for (response, expected_id) in test_cases {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                response.write(&mut writer);
            }
            assert_eq!(
                bytes[0],
                expected_id,
                "Wire ID mismatch for {:?}",
                response.kind()
            );
        }
    }

    // ========================================================================
    // StepKind transition tests for TOS extensions
    // ========================================================================

    #[test]
    fn test_step_kind_tos_transitions() {
        assert_eq!(StepKind::Contracts.next(), Some(StepKind::Kyc));
        assert_eq!(StepKind::Kyc.next(), Some(StepKind::Committees));
        assert_eq!(StepKind::Committees.next(), Some(StepKind::Nft));
        assert_eq!(StepKind::Nft.next(), Some(StepKind::Escrow));
        assert_eq!(StepKind::Escrow.next(), Some(StepKind::Arbitration));
        assert_eq!(StepKind::Arbitration.next(), Some(StepKind::Tns));
        assert_eq!(StepKind::Tns.next(), Some(StepKind::Energy));
        assert_eq!(StepKind::Energy.next(), Some(StepKind::Referral));
        assert_eq!(StepKind::Referral.next(), Some(StepKind::UnoBalance));
        assert_eq!(StepKind::UnoBalance.next(), Some(StepKind::Agent));
        assert_eq!(StepKind::Agent.next(), Some(StepKind::A2aNonce));
        assert_eq!(StepKind::A2aNonce.next(), Some(StepKind::ContractAsset));
        assert_eq!(
            StepKind::ContractAsset.next(),
            Some(StepKind::BlocksMetadata)
        );
        assert_eq!(StepKind::BlocksMetadata.next(), None);
    }

    #[test]
    fn test_step_kind_tos_request_kind_mapping() {
        assert_eq!(StepRequest::KycData(None).kind(), StepKind::Kyc);
        assert_eq!(StepRequest::Committees(None).kind(), StepKind::Committees);
        assert_eq!(StepRequest::GlobalCommittee.kind(), StepKind::Committees);
        assert_eq!(StepRequest::NftCollections(0, None).kind(), StepKind::Nft);
        assert_eq!(
            StepRequest::NftTokens(Cow::Owned(test_hash(1)), 0, None).kind(),
            StepKind::Nft
        );
        assert_eq!(
            StepRequest::NftOwnership(Cow::Owned(test_hash(1)), 0, None).kind(),
            StepKind::Nft
        );
        assert_eq!(StepRequest::EscrowAccounts(None).kind(), StepKind::Escrow);
        assert_eq!(
            StepRequest::ArbitrationData(None).kind(),
            StepKind::Arbitration
        );
        assert_eq!(
            StepRequest::ArbiterAccounts(None).kind(),
            StepKind::Arbitration
        );
        assert_eq!(StepRequest::TnsNames(None).kind(), StepKind::Tns);
        assert_eq!(
            StepRequest::EnergyData(Cow::Owned(vec![]), 0).kind(),
            StepKind::Energy
        );
        assert_eq!(
            StepRequest::ReferralRecords(None).kind(),
            StepKind::Referral
        );
        assert_eq!(
            StepRequest::UnoBalances(
                Cow::Owned(PublicKey::from_bytes(&[0u8; 32]).unwrap()),
                Cow::Owned(test_hash(1)),
                0,
                None
            )
            .kind(),
            StepKind::UnoBalance
        );
        assert_eq!(StepRequest::AgentData(None).kind(), StepKind::Agent);
        assert_eq!(StepRequest::A2aNonces(None).kind(), StepKind::A2aNonce);
        assert_eq!(
            StepRequest::ContractAssets(None).kind(),
            StepKind::ContractAsset
        );
    }
}
