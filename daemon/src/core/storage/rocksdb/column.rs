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
}

impl Column {
    pub const fn prefix(&self) -> Option<usize> {
        use Column::*;

        match self {
            VersionedAssets
            | VersionedNonces
            | VersionedBalances
            | VersionedMultisig
            | VersionedAssetsSupply
            | VersionedContracts
            | VersionedContractsBalances
            | VersionedContractsData
            | PrefixedRegistrations
            | VersionedEnergyResources
            | VersionedAIMiningStates => Some(PREFIX_TOPOHEIGHT_LEN),

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

            _ => None,
        }
    }
}
