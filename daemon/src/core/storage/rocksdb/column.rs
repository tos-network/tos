use strum::{AsRefStr, Display, EnumIter};

const PREFIX_TOPOHEIGHT_LEN: usize = 8;
const PREFIX_ID_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, EnumIter, Display, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum Column {
    // All transactions stored
    // {tx_hash} => {transaction}
    Transactions,
    // Which TXs are marked as executed
    // {tx_hash} => {block_hash}
    TransactionsExecuted,
    // In which blocks this TX was included
    // {tx_hash} => {block_hashes}
    TransactionInBlocks,
    // Transaction contract outputs
    // Standardized events that occurs on a contract call
    // {tx_hash} => {outputs}
    TransactionsOutputs,

    // Transaction execution results (Stake 2.0)
    // Stores actual fee burned and energy consumed after TX execution
    // {tx_hash} => {TransactionResult}
    TransactionResults,

    // ordered blocks hashes based on execution
    // {position} => {block_hash}
    BlocksExecutionOrder,
    // All blocks stored
    // {block_hash} => {block}
    Blocks,
    // All blocks hashes stored per height
    // {height} => {block_hashes}
    BlocksAtHeight,
    // Topoheight for a block hash
    // {block_hash} => {topoheight}
    TopoByHash,
    // Hash at a topoheight
    // {topoheight} => {block_hash}
    HashAtTopo,
    // Block difficulty / cumulative difficulty / covariance
    // {block_hash} => {difficulty}
    BlockDifficulty,
    // Misc data with no specific rules
    Common,
    // Topoheight Metadata
    // {topoheight} => {metadata}
    TopoHeightMetadata,

    // Each asset hash registered
    // {asset_hash} => {asset}
    Assets,
    // {asset_id} => {asset_hash}
    AssetById,
    // {topoheight}{asset_hash} => {asset}
    VersionedAssets,

    // {account_key} => {account}
    Account,
    // Column used as a "versioned" as its
    // prefixed with a topoheight to have
    // easier search per topoheight
    // {topoheight}{account_key} => {}
    PrefixedRegistrations,
    // This column is used as a reverse index
    // {account_id} => {account_key}
    AccountById,

    // {topoheight}{account_id} => {version}
    VersionedMultisig,
    // {topoheight}{account_id} => {version}
    VersionedNonces,

    // Account balances pointer
    // {account_id}{asset_id} => {topoheight}
    Balances,
    // {topoheight}{account_id}{asset_id} => {version}
    VersionedBalances,

    // UNO (privacy) balance pointer
    // {account_id}{asset_id} => {topoheight}
    UnoBalances,
    // Versioned UNO balances
    // {topoheight}{account_id}{asset_id} => {VersionedUnoBalance}
    VersionedUnoBalances,

    // Contains the contract module per hash
    // {contract_hash} => {contract}
    Contracts,
    // {contract_id} => {contract_hash}
    ContractById,

    // {topoheight}{contract_id} => {version}
    VersionedContracts,
    // {topoheight}{contract_id}{data_key} => {version}
    VersionedContractsData,
    // Represent the link between a contract and a data
    // {contract_id}{data_key} => {topoheight}
    ContractsData,

    // A contract data accessible by its ID
    // {data_id} => {data}
    ContractDataById,

    // {contract}{asset} => {topoheight}
    ContractsBalances,
    // {topoheight}{contract}{asset} => {version}
    VersionedContractsBalances,

    // {topoheight}{asset_id} => {version}
    VersionedAssetsSupply,

    // Versioned energy resources for each account
    // Energy pointer is now stored in Account.energy_pointer
    // {topoheight}_{account_address} => {energy_resource}
    VersionedEnergyResources,

    // ===== Stake 2.0 Delegation =====

    // Delegated resources: from -> to -> DelegatedResource
    // Key: {from_pubkey (32 bytes)}{to_pubkey (32 bytes)} => {DelegatedResource}
    DelegatedResources,
    // Index for looking up delegations received by an account
    // Key: {to_pubkey (32 bytes)}{from_pubkey (32 bytes)} => {}
    DelegatedResourcesIndex,

    // Global energy state for the network (Stake 2.0)
    // Single key "GLOBAL" => {GlobalEnergyState}
    GlobalEnergyState,

    // ===== Stake 2.0 Versioned Delegation (Reorg Support) =====

    // Versioned delegated resources for reorg support
    // Key: {topoheight}_{from_address}_{to_address} => {DelegatedResource}
    VersionedDelegatedResources,

    // Versioned global energy state for reorg support
    // Key: {topoheight} => {GlobalEnergyState}
    VersionedGlobalEnergyState,

    // AI mining state pointer
    // AI_MINING_STATE_TOPOHEIGHT => {topoheight}
    AIMiningState,
    // Versioned AI mining states
    // {topoheight} => {ai_mining_state}
    VersionedAIMiningStates,

    // Contract events storage for LOG0-LOG4 syscalls
    // {contract_id}{topoheight}{log_index} => {StoredContractEvent}
    ContractEvents,
    // Contract events indexed by transaction hash
    // {tx_hash}{log_index} => {StoredContractEvent}
    ContractEventsByTx,
    // Contract events indexed by topic0 (event signature)
    // {contract_id}{topic0}{topoheight}{log_index} => {StoredContractEvent}
    ContractEventsByTopic,

    // Scheduled/delayed contract executions
    // {topoheight}{contract_id} => {ScheduledExecution}
    DelayedExecution,
    // Registration metadata for efficient range queries
    // {registration_topoheight}{contract_id}{execution_topoheight} => {}
    DelayedExecutionRegistrations,
    // Priority index for OFFERCALL - enables efficient top-N selection by offer amount
    // Key format: {exec_topo}{inverted_offer}{reg_topo}{contract_id} => {}
    // - exec_topo: execution topoheight (8 bytes, ascending)
    // - inverted_offer: u64::MAX - offer_amount (8 bytes, so higher offers sort first)
    // - reg_topo: registration topoheight (8 bytes, FIFO for equal offers)
    // - contract_id: contract ID (8 bytes, deterministic tiebreaker)
    DelayedExecutionPriority,

    // ===== Referral System =====

    // Referral records: user -> referral data
    // {user_public_key} => {ReferralRecord}
    Referrals,
    // Direct referrals index: referrer -> list of direct referrals
    // {referrer_public_key}{page_number} => {Vec<PublicKey>}
    ReferralDirects,
    // Team volume records: per-user per-asset volume tracking
    // {user_public_key (32 bytes)}{asset_hash (32 bytes)} => {TeamVolumeRecord}
    TeamVolumes,

    // ===== KYC System =====

    // KYC data: user -> KycData (43 bytes)
    // {user_public_key} => {KycData}
    KycData,
    // KYC metadata: user -> (committee_id, topoheight, tx_hash)
    // {user_public_key} => {KycMetadata}
    KycMetadata,
    // Emergency suspension data
    // {user_public_key} => {(reason_hash, expires_at)}
    KycEmergencySuspension,
    // KYC appeal records
    // {user_public_key} => {KycAppealRecord}
    KycAppeal,
    // Previous KYC status before emergency suspension (for proper restoration)
    // {user_public_key} => {KycStatus}
    KycEmergencyPreviousStatus,

    // ===== Security Committee System =====

    // Committee data: committee_id -> SecurityCommittee
    // {committee_id (32 bytes)} => {SecurityCommittee}
    Committees,
    // Global committee ID pointer
    // GLOBAL_COMMITTEE_KEY => {committee_id}
    GlobalCommittee,
    // Committee by region index
    // {region (u8)}{committee_id (32 bytes)} => {}
    CommitteesByRegion,
    // Member to committees index: member -> list of committee IDs
    // {member_public_key (32 bytes)} => {Vec<Hash>}
    MemberCommittees,
    // Child committees index: parent_id -> list of child IDs
    // {parent_committee_id (32 bytes)} => {Vec<Hash>}
    ChildCommittees,
}

impl Column {
    pub const fn prefix(&self) -> Option<usize> {
        use Column::*;

        match self {
            VersionedAssets
            | VersionedNonces
            | VersionedBalances
            | VersionedUnoBalances
            | VersionedMultisig
            | VersionedAssetsSupply
            | VersionedContracts
            | VersionedContractsBalances
            | VersionedContractsData
            | PrefixedRegistrations
            | VersionedEnergyResources
            | VersionedAIMiningStates
            | VersionedDelegatedResources
            | VersionedGlobalEnergyState => Some(PREFIX_TOPOHEIGHT_LEN),

            UnoBalances => Some(PREFIX_ID_LEN),

            ContractsBalances => Some(PREFIX_ID_LEN),
            Balances => Some(PREFIX_ID_LEN),

            // Contract events: prefix by contract_id (8 bytes)
            ContractEvents => Some(PREFIX_ID_LEN),
            // Events by tx: prefix by tx_hash (32 bytes)
            ContractEventsByTx => Some(32),
            // Events by topic: prefix by contract_id (8 bytes)
            ContractEventsByTopic => Some(PREFIX_ID_LEN),

            // Delayed executions: prefix by topoheight (8 bytes)
            DelayedExecution => Some(PREFIX_TOPOHEIGHT_LEN),
            DelayedExecutionRegistrations => Some(PREFIX_TOPOHEIGHT_LEN),
            // Priority index: prefix by exec_topoheight (8 bytes)
            DelayedExecutionPriority => Some(PREFIX_TOPOHEIGHT_LEN),

            // Referral directs: prefix by referrer public key (32 bytes)
            ReferralDirects => Some(32),
            // Team volumes: prefix by user public key (32 bytes)
            TeamVolumes => Some(32),

            // Committee by region: prefix by region (1 byte)
            CommitteesByRegion => Some(1),
            // Child committees: prefix by parent committee ID (32 bytes)
            ChildCommittees => Some(32),

            // Delegated resources: prefix by from_pubkey (32 bytes)
            DelegatedResources => Some(32),
            // Delegated resources index: prefix by to_pubkey (32 bytes)
            DelegatedResourcesIndex => Some(32),

            _ => None,
        }
    }
}
