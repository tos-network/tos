use super::{ApiError, InternalRpcError};
use crate::{
    config::{
        get_hard_forks as get_configured_hard_forks, DEV_FEES, DEV_PUBLIC_KEY, MILLIS_PER_SECOND,
    },
    core::{
        blockchain::{get_block_dev_fee, get_block_reward, Blockchain, BroadcastOption},
        error::BlockchainError,
        hard_fork::{
            get_block_time_target_for_version, get_pow_algorithm_for_version, get_version_at_height,
        },
        mempool::Mempool,
        storage::*,
    },
    p2p::peer_list::Peer,
};
use anyhow::Context as AnyContext;
use human_bytes::human_bytes;
use log::{debug, info, trace};
use serde_json::{json, Value};
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

// ============================================================================
// Payment Request Storage (In-Memory)
// ============================================================================
// Payment requests are stored in-memory for quick access.
// They are automatically cleaned up when they expire.
// For production, consider persisting to database for durability.
// ============================================================================

use crate::rpc::callback::{send_payment_callback, send_payment_expired_callback, CallbackService};
use once_cell::sync::Lazy;
use tos_common::api::payment::StoredPaymentRequest;

/// Maximum number of payment requests to store (prevents memory exhaustion)
const MAX_PAYMENT_REQUESTS: usize = 10000;

/// In-memory payment request storage
/// Key: payment_id, Value: StoredPaymentRequest
static PAYMENT_REQUESTS: Lazy<RwLock<HashMap<String, StoredPaymentRequest>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static CALLBACK_SERVICE: Lazy<Arc<CallbackService>> =
    Lazy::new(|| Arc::new(CallbackService::new()));

/// Store a new payment request
async fn store_payment_request(request: StoredPaymentRequest) -> Result<(), InternalRpcError> {
    let mut store = PAYMENT_REQUESTS.write().await;

    // Check capacity limit
    if store.len() >= MAX_PAYMENT_REQUESTS {
        // Clean up expired entries first
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        store.retain(|_, req| {
            if let Some(exp) = req.expires_at {
                now <= exp || req.is_paid()
            } else {
                true
            }
        });

        // If still over limit, reject
        if store.len() >= MAX_PAYMENT_REQUESTS {
            return Err(InternalRpcError::InvalidRequestStr(
                "Payment request storage is full, please try again later",
            ));
        }
    }

    store.insert(request.payment_id.clone(), request);
    Ok(())
}

/// Get a payment request by ID
async fn get_payment_request(payment_id: &str) -> Option<StoredPaymentRequest> {
    let store = PAYMENT_REQUESTS.read().await;
    store.get(payment_id).cloned()
}

/// Update a payment request with transaction info
/// Note: Currently unused as get_payment_status uses blockchain scanning,
/// but kept for potential future caching/optimization
#[allow(dead_code)]
async fn update_payment_with_tx(
    payment_id: &str,
    tx_hash: tos_common::crypto::Hash,
    amount_received: u64,
    confirmed_at_topoheight: u64,
) {
    let mut store = PAYMENT_REQUESTS.write().await;
    if let Some(req) = store.get_mut(payment_id) {
        req.tx_hash = Some(tx_hash);
        req.amount_received = Some(amount_received);
        req.confirmed_at_topoheight = Some(confirmed_at_topoheight);
    }
}

async fn update_payment_callback_status(payment_id: &str, status: PaymentStatus) {
    let mut store = PAYMENT_REQUESTS.write().await;
    if let Some(req) = store.get_mut(payment_id) {
        req.last_callback_status = Some(status);
    }
}

async fn maybe_send_callback(
    payment_id: &str,
    status: PaymentStatus,
    tx_hash: Option<Hash>,
    amount: Option<u64>,
    confirmations: u64,
) {
    let stored = match get_payment_request(payment_id).await {
        Some(req) => req,
        None => return,
    };

    let callback_url = match stored.callback_url {
        Some(url) => url,
        None => return,
    };

    let secret = match CALLBACK_SERVICE.get_webhook_secret(&callback_url).await {
        Some(secret) => secret,
        None => return,
    };

    match status {
        PaymentStatus::Confirmed => {
            if stored.last_callback_status == Some(PaymentStatus::Confirmed) {
                return;
            }
            if let (Some(tx_hash), Some(amount)) = (tx_hash, amount) {
                send_payment_callback(
                    Arc::clone(&CALLBACK_SERVICE),
                    callback_url,
                    secret,
                    payment_id.to_string(),
                    tx_hash,
                    amount,
                    confirmations,
                );
                update_payment_callback_status(payment_id, PaymentStatus::Confirmed).await;
            }
        }
        PaymentStatus::Expired => {
            if stored.last_callback_status == Some(PaymentStatus::Expired) {
                return;
            }
            send_payment_expired_callback(
                Arc::clone(&CALLBACK_SERVICE),
                callback_url,
                secret,
                payment_id.to_string(),
            );
            update_payment_callback_status(payment_id, PaymentStatus::Expired).await;
        }
        PaymentStatus::Mempool | PaymentStatus::Confirming | PaymentStatus::Underpaid => {
            if stored.last_callback_status.is_some() {
                return;
            }
            if let (Some(tx_hash), Some(amount)) = (tx_hash, amount) {
                send_payment_callback(
                    Arc::clone(&CALLBACK_SERVICE),
                    callback_url,
                    secret,
                    payment_id.to_string(),
                    tx_hash,
                    amount,
                    confirmations,
                );
                update_payment_callback_status(payment_id, status).await;
            }
        }
        PaymentStatus::Pending => {}
    }
}
use tos_common::{
    ai_mining::{AIMiningStatistics, AIMiningTask, TaskStatus},
    api::{daemon::*, RPCContractOutput, RPCTransaction, SplitAddressParams, SplitAddressResult},
    asset::RPCAssetData,
    async_handler,
    block::{Block, BlockHeader, MinerWork, TopoHeight},
    config::{MAXIMUM_SUPPLY, MAX_TRANSACTION_SIZE, TOS_ASSET, VERSION},
    context::Context,
    contract::ScheduledExecution,
    crypto::{elgamal::CompressedPublicKey, Address, AddressType, Hash},
    difficulty::{CumulativeDifficulty, Difficulty},
    immutable::Immutable,
    rpc::{parse_params, require_no_params, server::ClientAddr, RPCHandler},
    serializer::Serializer,
    time::TimestampSeconds,
    transaction::{Transaction, TransactionType},
    utils::format_hashrate,
};

// Get the block type using the block hash and the blockchain current state
pub async fn get_block_type_for_block<
    S: Storage,
    P: DifficultyProvider + DagOrderProvider + BlocksAtHeightProvider + PrunedTopoheightProvider,
>(
    blockchain: &Blockchain<S>,
    provider: &P,
    hash: &Hash,
) -> Result<BlockType, InternalRpcError> {
    Ok(
        if blockchain
            .is_block_orphaned_for_storage(provider, hash)
            .await?
        {
            BlockType::Orphaned
        } else if blockchain
            .is_sync_block(provider, hash)
            .await
            .context("Error while checking if block is sync")?
        {
            BlockType::Sync
        } else if blockchain
            .is_side_block(provider, hash)
            .await
            .context("Error while checking if block is side")?
        {
            BlockType::Side
        } else {
            BlockType::Normal
        },
    )
}

async fn get_block_data<S: Storage, P>(
    blockchain: &Blockchain<S>,
    provider: &P,
    hash: &Hash,
) -> Result<
    (
        Option<TopoHeight>,
        Option<u64>,
        Option<u64>,
        BlockType,
        CumulativeDifficulty,
        Difficulty,
    ),
    InternalRpcError,
>
where
    P: DifficultyProvider
        + DagOrderProvider
        + BlocksAtHeightProvider
        + PrunedTopoheightProvider
        + BlockDagProvider,
{
    let (topoheight, supply, reward) = if provider.is_block_topological_ordered(hash).await? {
        let topoheight = provider
            .get_topo_height_for_hash(&hash)
            .await
            .context("Error while retrieving topo height")?;
        (
            Some(topoheight),
            Some(
                provider
                    .get_supply_at_topo_height(topoheight)
                    .await
                    .context("Error while retrieving supply")?,
            ),
            Some(
                provider
                    .get_block_reward_at_topo_height(topoheight)
                    .context("Error while retrieving block reward")?,
            ),
        )
    } else {
        (None, None, None)
    };

    let block_type = get_block_type_for_block(&blockchain, &*provider, hash).await?;
    let cumulative_difficulty = provider
        .get_cumulative_difficulty_for_block_hash(hash)
        .await
        .context("Error while retrieving cumulative difficulty")?;
    let difficulty = provider
        .get_difficulty_for_block_hash(&hash)
        .await
        .context("Error while retrieving difficulty")?;

    Ok((
        topoheight,
        supply,
        reward,
        block_type,
        cumulative_difficulty,
        difficulty,
    ))
}

pub async fn get_block_response<S: Storage, P>(
    blockchain: &Blockchain<S>,
    provider: &P,
    hash: &Hash,
    block: &Block,
    total_size_in_bytes: usize,
) -> Result<Value, InternalRpcError>
where
    P: DifficultyProvider
        + DagOrderProvider
        + BlocksAtHeightProvider
        + PrunedTopoheightProvider
        + BlockDagProvider
        + ClientProtocolProvider,
{
    let (topoheight, supply, reward, block_type, cumulative_difficulty, difficulty) =
        get_block_data(blockchain, provider, hash).await?;
    let mut total_fees = 0;
    if block_type != BlockType::Orphaned {
        for (tx, tx_hash) in block.get_transactions().iter().zip(block.get_txs_hashes()) {
            // check that the TX was correctly executed in this block
            // retrieve all fees for valid txs
            if provider
                .is_tx_executed_in_block(tx_hash, &hash)
                .context("Error while checking if tx was executed")?
            {
                total_fees += tx.get_fee();
            }
        }
    }

    let mainnet = blockchain.get_network().is_mainnet();
    let header = block.get_header();
    let transactions = block
        .get_transactions()
        .iter()
        .zip(block.get_txs_hashes())
        .map(|(tx, hash)| RPCTransaction::from_tx(tx, hash, mainnet))
        .collect::<Vec<RPCTransaction<'_>>>();

    let (dev_reward, miner_reward) = get_optional_block_rewards(header.get_height(), reward)
        .map(|(dev_reward, miner_reward)| (Some(dev_reward), Some(miner_reward)))
        .unwrap_or((None, None));

    Ok(json!(RPCBlockResponse {
        hash: Cow::Borrowed(hash),
        topoheight,
        block_type,
        cumulative_difficulty: Cow::Borrowed(&cumulative_difficulty),
        difficulty: Cow::Borrowed(&difficulty),
        supply,
        reward,
        dev_reward,
        miner_reward,
        total_fees: Some(total_fees),
        total_size_in_bytes,
        extra_nonce: Cow::Borrowed(header.get_extra_nonce()),
        timestamp: header.get_timestamp(),
        nonce: header.get_nonce(),
        height: header.get_height(),
        version: header.get_version(),
        miner: Cow::Owned(header.get_miner().as_address(mainnet)),
        tips: Cow::Borrowed(header.get_tips()),
        txs_hashes: Cow::Borrowed(header.get_txs_hashes()),
        transactions
    }))
}

// Get block rewards based on height and reward
fn get_block_rewards(height: u64, reward: u64) -> (u64, u64) {
    let dev_fee_percentage = get_block_dev_fee(height);
    let dev_reward = reward * dev_fee_percentage / 100;
    let miner_reward = reward - dev_reward;

    (dev_reward, miner_reward)
}

// Get optional block rewards based on height and reward
fn get_optional_block_rewards(height: u64, reward: Option<u64>) -> Option<(u64, u64)> {
    if let Some(reward) = reward {
        Some(get_block_rewards(height, reward))
    } else {
        None
    }
}

// Get a block response based on data in chain and from parameters
pub async fn get_block_response_for_hash<S: Storage>(
    blockchain: &Blockchain<S>,
    storage: &S,
    hash: &Hash,
    include_txs: bool,
) -> Result<Value, InternalRpcError> {
    if !storage
        .has_block_with_hash(&hash)
        .await
        .context("Error while checking if block exist")?
    {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::BlockNotFound(hash.clone()).into(),
        ));
    }

    let value: Value = if include_txs {
        let block = storage
            .get_block_by_hash(&hash)
            .await
            .context("Error while retrieving full block")?;
        let total_size_in_bytes = block.size();
        get_block_response(blockchain, storage, hash, &block, total_size_in_bytes).await?
    } else {
        let (topoheight, supply, reward, block_type, cumulative_difficulty, difficulty) =
            get_block_data(blockchain, storage, hash).await?;
        let header = storage
            .get_block_header_by_hash(&hash)
            .await
            .context("Error while retrieving full block")?;

        // calculate total size in bytes
        let mut total_size_in_bytes = header.size();
        for tx_hash in header.get_txs_hashes() {
            total_size_in_bytes += storage
                .get_transaction_size(tx_hash)
                .await
                .context(format!("Error while retrieving transaction {tx_hash} size"))?;
        }

        let mainnet = blockchain.get_network().is_mainnet();
        let (dev_reward, miner_reward) = get_optional_block_rewards(header.get_height(), reward)
            .map(|(dev_reward, miner_reward)| (Some(dev_reward), Some(miner_reward)))
            .unwrap_or((None, None));

        json!(RPCBlockResponse {
            hash: Cow::Borrowed(hash),
            topoheight,
            block_type,
            cumulative_difficulty: Cow::Owned(cumulative_difficulty),
            difficulty: Cow::Owned(difficulty),
            supply,
            reward,
            dev_reward,
            miner_reward,
            total_fees: None,
            total_size_in_bytes,
            extra_nonce: Cow::Borrowed(header.get_extra_nonce()),
            timestamp: header.get_timestamp(),
            nonce: header.get_nonce(),
            height: header.get_height(),
            version: header.get_version(),
            miner: Cow::Owned(header.get_miner().as_address(mainnet)),
            tips: Cow::Borrowed(header.get_tips()),
            txs_hashes: Cow::Borrowed(header.get_txs_hashes()),
            transactions: Vec::with_capacity(0),
        })
    };

    Ok(value)
}

// Transaction response based on data in chain/mempool and from parameters
pub async fn get_transaction_response<'a, S: Storage>(
    storage: &S,
    tx: &'a Transaction,
    hash: &'a Hash,
    in_mempool: bool,
    first_seen: Option<TimestampSeconds>,
) -> Result<TransactionResponse<'a>, InternalRpcError> {
    let blocks = if storage
        .has_tx_blocks(hash)
        .context("Error while checking if tx in included in blocks")?
    {
        Some(
            storage
                .get_blocks_for_tx(hash)
                .context("Error while retrieving in which blocks its included")?,
        )
    } else {
        None
    };

    let data = RPCTransaction::from_tx(tx, hash, storage.is_mainnet());
    let executed_in_block = storage.get_block_executor_for_tx(hash).ok();
    Ok(TransactionResponse {
        blocks,
        executed_in_block,
        data,
        in_mempool,
        first_seen,
    })
}

// first check on disk, then check in mempool
pub async fn get_transaction_response_for_hash<S: Storage>(
    storage: &S,
    mempool: &Mempool,
    hash: &Hash,
) -> Result<Value, InternalRpcError> {
    match storage.get_transaction(hash).await {
        Ok(tx) => {
            let tx = get_transaction_response(storage, &tx, hash, false, None).await?;
            Ok(json!(tx))
        }
        Err(_) => {
            let tx = mempool
                .get_sorted_tx(hash)
                .context("Error while retrieving transaction from disk and mempool")?;
            let tx = get_transaction_response(
                storage,
                &tx.get_tx(),
                hash,
                true,
                Some(tx.get_first_seen()),
            )
            .await?;
            Ok(json!(tx))
        }
    }
}

// Get a Peer Entry based on peer data
pub async fn get_peer_entry(peer: &Peer) -> PeerEntry<'_> {
    let top_block_hash = { peer.get_top_block_hash().lock().await.clone() };
    let peers = { peer.get_peers().lock().await.clone() };
    let cumulative_difficulty = { peer.get_cumulative_difficulty().lock().await.clone() };
    PeerEntry {
        id: peer.get_id(),
        addr: Cow::Borrowed(peer.get_connection().get_address()),
        local_port: peer.get_local_port(),
        tag: Cow::Borrowed(peer.get_node_tag()),
        version: Cow::Borrowed(peer.get_version()),
        top_block_hash: Cow::Owned(top_block_hash),
        topoheight: peer.get_topoheight(),
        height: peer.get_height(),
        last_ping: peer.get_last_ping(),
        peers: Cow::Owned(peers.into_iter().collect()),
        pruned_topoheight: peer.get_pruned_topoheight(),
        cumulative_difficulty: Cow::Owned(cumulative_difficulty),
        connected_on: peer.get_connection().connected_on(),
        bytes_recv: peer.get_connection().bytes_in(),
        bytes_sent: peer.get_connection().bytes_out(),
    }
}

// This function is used to register all the RPC methods
pub fn register_methods<S: Storage>(
    handler: &mut RPCHandler<Arc<Blockchain<S>>>,
    allow_mining_methods: bool,
    allow_admin_methods: bool,
) {
    info!("Registering RPC methods...");
    handler.register_method("get_version", async_handler!(version::<S>));
    handler.register_method("get_height", async_handler!(get_height::<S>));
    handler.register_method("get_topoheight", async_handler!(get_topoheight::<S>));
    handler.register_method(
        "get_pruned_topoheight",
        async_handler!(get_pruned_topoheight::<S>),
    );
    handler.register_method("get_info", async_handler!(get_info::<S>));
    handler.register_method("get_difficulty", async_handler!(get_difficulty::<S>));
    handler.register_method("get_tips", async_handler!(get_tips::<S>));
    handler.register_method(
        "get_dev_fee_thresholds",
        async_handler!(get_dev_fee_thresholds::<S>),
    );
    handler.register_method("get_size_on_disk", async_handler!(get_size_on_disk::<S>));

    // Retro compatibility, use stable_height
    handler.register_method("get_stableheight", async_handler!(get_stable_height::<S>));
    handler.register_method("get_stable_height", async_handler!(get_stable_height::<S>));
    handler.register_method(
        "get_stable_topoheight",
        async_handler!(get_stable_topoheight::<S>),
    );
    handler.register_method("get_hard_forks", async_handler!(get_hard_forks::<S>));

    handler.register_method(
        "get_block_at_topoheight",
        async_handler!(get_block_at_topoheight::<S>),
    );
    handler.register_method(
        "get_blocks_at_height",
        async_handler!(get_blocks_at_height::<S>),
    );
    handler.register_method("get_block_by_hash", async_handler!(get_block_by_hash::<S>));
    handler.register_method("get_top_block", async_handler!(get_top_block::<S>));

    handler.register_method("get_balance", async_handler!(get_balance::<S>));
    handler.register_method(
        "get_stable_balance",
        async_handler!(get_stable_balance::<S>),
    );
    handler.register_method("has_balance", async_handler!(has_balance::<S>));
    handler.register_method(
        "get_balance_at_topoheight",
        async_handler!(get_balance_at_topoheight::<S>),
    );

    handler.register_method("get_nonce", async_handler!(get_nonce::<S>));
    handler.register_method("has_nonce", async_handler!(has_nonce::<S>));
    handler.register_method(
        "get_nonce_at_topoheight",
        async_handler!(get_nonce_at_topoheight::<S>),
    );

    // Assets
    handler.register_method("get_asset", async_handler!(get_asset::<S>));
    handler.register_method("get_asset_supply", async_handler!(get_asset_supply::<S>));
    handler.register_method("get_assets", async_handler!(get_assets::<S>));

    handler.register_method("count_assets", async_handler!(count_assets::<S>));
    handler.register_method("count_accounts", async_handler!(count_accounts::<S>));
    handler.register_method(
        "count_transactions",
        async_handler!(count_transactions::<S>),
    );
    handler.register_method("count_contracts", async_handler!(count_contracts::<S>));

    handler.register_method(
        "submit_transaction",
        async_handler!(submit_transaction::<S>),
    );
    handler.register_method(
        "get_transaction_executor",
        async_handler!(get_transaction_executor::<S>),
    );
    handler.register_method("get_transaction", async_handler!(get_transaction::<S>));
    handler.register_method("get_transactions", async_handler!(get_transactions::<S>));
    handler.register_method(
        "get_transactions_summary",
        async_handler!(get_transactions_summary::<S>),
    );
    handler.register_method(
        "is_tx_executed_in_block",
        async_handler!(is_tx_executed_in_block::<S>),
    );

    handler.register_method("p2p_status", async_handler!(p2p_status::<S>));
    handler.register_method("get_peers", async_handler!(get_peers::<S>));

    handler.register_method("get_mempool", async_handler!(get_mempool::<S>));
    handler.register_method(
        "get_mempool_summary",
        async_handler!(get_mempool_summary::<S>),
    );
    handler.register_method("get_mempool_cache", async_handler!(get_mempool_cache::<S>));
    handler.register_method(
        "get_estimated_fee_rates",
        async_handler!(get_estimated_fee_rates::<S>),
    );

    handler.register_method("get_dag_order", async_handler!(get_dag_order::<S>));
    handler.register_method(
        "get_blocks_range_by_topoheight",
        async_handler!(get_blocks_range_by_topoheight::<S>),
    );
    handler.register_method(
        "get_blocks_range_by_height",
        async_handler!(get_blocks_range_by_height::<S>),
    );

    handler.register_method(
        "get_account_history",
        async_handler!(get_account_history::<S>),
    );
    handler.register_method(
        "get_account_assets",
        async_handler!(get_account_assets::<S>),
    );
    handler.register_method("get_accounts", async_handler!(get_accounts::<S>));
    handler.register_method(
        "is_account_registered",
        async_handler!(is_account_registered::<S>),
    );
    handler.register_method(
        "get_account_registration_topoheight",
        async_handler!(get_account_registration_topoheight::<S>),
    );

    // Useful methods
    handler.register_method("validate_address", async_handler!(validate_address::<S>));
    handler.register_method("split_address", async_handler!(split_address::<S>));
    handler.register_method(
        "extract_key_from_address",
        async_handler!(extract_key_from_address::<S>),
    );
    handler.register_method(
        "make_integrated_address",
        async_handler!(make_integrated_address::<S>),
    );
    handler.register_method(
        "decrypt_extra_data",
        async_handler!(decrypt_extra_data::<S>),
    );

    // Multisig
    handler.register_method(
        "get_multisig_at_topoheight",
        async_handler!(get_multisig_at_topoheight::<S>),
    );
    handler.register_method("get_multisig", async_handler!(get_multisig::<S>));
    handler.register_method("has_multisig", async_handler!(has_multisig::<S>));
    handler.register_method(
        "has_multisig_at_topoheight",
        async_handler!(has_multisig_at_topoheight::<S>),
    );

    // Contracts
    handler.register_method(
        "get_contract_outputs",
        async_handler!(get_contract_outputs::<S>),
    );
    handler.register_method(
        "get_contract_module",
        async_handler!(get_contract_module::<S>),
    );
    handler.register_method("get_contract_data", async_handler!(get_contract_data::<S>));
    handler.register_method(
        "get_contract_data_at_topoheight",
        async_handler!(get_contract_data_at_topoheight::<S>),
    );
    handler.register_method(
        "get_contract_balance",
        async_handler!(get_contract_balance::<S>),
    );
    handler.register_method(
        "get_contract_balance_at_topoheight",
        async_handler!(get_contract_balance_at_topoheight::<S>),
    );
    handler.register_method(
        "get_contract_assets",
        async_handler!(get_contract_assets::<S>),
    );
    handler.register_method(
        "get_contract_address_from_tx",
        async_handler!(get_contract_address_from_tx::<S>),
    );
    handler.register_method(
        "get_contract_events",
        async_handler!(get_contract_events::<S>),
    );
    handler.register_method(
        "get_contract_scheduled_executions_at_topoheight",
        async_handler!(get_contract_scheduled_executions_at_topoheight::<S>),
    );
    handler.register_method("get_contracts", async_handler!(get_contracts::<S>));
    handler.register_method(
        "get_contract_data_entries",
        async_handler!(get_contract_data_entries::<S>),
    );

    // Address utilities
    handler.register_method("key_to_address", async_handler!(key_to_address::<S>));

    // Block summaries (lightweight)
    handler.register_method(
        "get_block_summary_at_topoheight",
        async_handler!(get_block_summary_at_topoheight::<S>),
    );
    handler.register_method(
        "get_block_summary_by_hash",
        async_handler!(get_block_summary_by_hash::<S>),
    );

    // Batch balance query
    handler.register_method(
        "get_balances_at_maximum_topoheight",
        async_handler!(get_balances_at_maximum_topoheight::<S>),
    );

    // Block analytics
    handler.register_method(
        "get_block_difficulty_by_hash",
        async_handler!(get_block_difficulty_by_hash::<S>),
    );

    // Historical supply
    handler.register_method(
        "get_asset_supply_at_topoheight",
        async_handler!(get_asset_supply_at_topoheight::<S>),
    );

    // Contract registered executions
    handler.register_method(
        "get_contract_registered_executions_at_topoheight",
        async_handler!(get_contract_registered_executions_at_topoheight::<S>),
    );

    // P2p
    handler.register_method(
        "get_p2p_block_propagation",
        async_handler!(get_p2p_block_propagation::<S>),
    );

    // Energy management
    handler.register_method("get_energy", async_handler!(get_energy::<S>));

    // AI Mining management
    handler.register_method(
        "get_ai_mining_state",
        async_handler!(get_ai_mining_state::<S>),
    );
    handler.register_method(
        "get_ai_mining_state_at_topoheight",
        async_handler!(get_ai_mining_state_at_topoheight::<S>),
    );
    handler.register_method(
        "has_ai_mining_state_at_topoheight",
        async_handler!(has_ai_mining_state_at_topoheight::<S>),
    );
    handler.register_method(
        "get_ai_mining_statistics",
        async_handler!(get_ai_mining_statistics::<S>),
    );
    handler.register_method(
        "get_ai_mining_task",
        async_handler!(get_ai_mining_task::<S>),
    );
    handler.register_method(
        "get_ai_mining_miner",
        async_handler!(get_ai_mining_miner::<S>),
    );
    handler.register_method(
        "get_ai_mining_active_tasks",
        async_handler!(get_ai_mining_active_tasks::<S>),
    );

    // QR Code Payment methods
    handler.register_method(
        "create_payment_request",
        async_handler!(create_payment_request::<S>),
    );
    handler.register_method(
        "parse_payment_request",
        async_handler!(parse_payment_request::<S>),
    );
    handler.register_method(
        "get_payment_status",
        async_handler!(get_payment_status::<S>),
    );
    handler.register_method(
        "get_address_payments",
        async_handler!(get_address_payments::<S>),
    );
    handler.register_method(
        "register_payment_webhook",
        async_handler!(register_payment_webhook::<S>),
    );
    handler.register_method(
        "unregister_payment_webhook",
        async_handler!(unregister_payment_webhook::<S>),
    );

    // Referral system
    handler.register_method("has_referrer", async_handler!(has_referrer::<S>));
    handler.register_method("get_referrer", async_handler!(get_referrer::<S>));
    handler.register_method("get_uplines", async_handler!(get_uplines::<S>));
    handler.register_method(
        "get_direct_referrals",
        async_handler!(get_direct_referrals::<S>),
    );
    handler.register_method(
        "get_referral_record",
        async_handler!(get_referral_record::<S>),
    );
    handler.register_method("get_team_size", async_handler!(get_team_size::<S>));
    handler.register_method(
        "get_referral_level",
        async_handler!(get_referral_level::<S>),
    );

    // KYC system
    handler.register_method("has_kyc", async_handler!(has_kyc::<S>));
    handler.register_method("get_kyc", async_handler!(get_kyc::<S>));
    handler.register_method("get_kyc_tier", async_handler!(get_kyc_tier::<S>));
    handler.register_method("is_kyc_valid", async_handler!(is_kyc_valid::<S>));
    handler.register_method("meets_kyc_level", async_handler!(meets_kyc_level::<S>));
    handler.register_method(
        "get_verifying_committee",
        async_handler!(get_verifying_committee::<S>),
    );
    handler.register_method("get_committee", async_handler!(get_committee::<S>));
    handler.register_method(
        "get_global_committee",
        async_handler!(get_global_committee::<S>),
    );

    if allow_mining_methods {
        handler.register_method(
            "get_block_template",
            async_handler!(get_block_template::<S>),
        );
        handler.register_method("get_miner_work", async_handler!(get_miner_work::<S>));
        handler.register_method("submit_block", async_handler!(submit_block::<S>));
    }

    // Admin methods (require --enable-admin-rpc flag)
    // WARNING: These are dangerous operations. Only enable for trusted operators.
    if allow_admin_methods {
        handler.register_method("prune_chain", async_handler!(prune_chain::<S>));
        handler.register_method("rewind_chain", async_handler!(rewind_chain::<S>));
        handler.register_method("clear_caches", async_handler!(clear_caches::<S>));
    }
}

async fn version<S: Storage>(_: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    Ok(json!(VERSION))
}

async fn get_height<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    Ok(json!(blockchain.get_height()))
}

async fn get_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    Ok(json!(blockchain.get_topo_height()))
}

async fn get_pruned_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let pruned_topoheight = storage
        .get_pruned_topoheight()
        .await
        .context("Error while retrieving pruned topoheight")?;

    Ok(json!(pruned_topoheight))
}

async fn get_stable_height<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    Ok(json!(blockchain.get_stable_height()))
}

async fn get_stable_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    Ok(json!(blockchain.get_stable_topoheight()))
}

async fn get_hard_forks<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let hard_forks = get_configured_hard_forks(blockchain.get_network());

    Ok(json!(hard_forks))
}

async fn get_block_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlockAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let hash = storage
        .get_hash_at_topo_height(params.topoheight)
        .await
        .context("Error while retrieving hash at topo height")?;
    get_block_response_for_hash(&blockchain, &storage, &hash, params.include_txs).await
}

async fn get_block_by_hash<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlockByHashParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    get_block_response_for_hash(&blockchain, &storage, &params.hash, params.include_txs).await
}

async fn get_top_block<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTopBlockParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let hash = blockchain
        .get_top_block_hash_for_storage(&storage)
        .await
        .context("Error while retrieving top block hash")?;
    get_block_response_for_hash(&blockchain, &storage, &hash, params.include_txs).await
}

async fn get_block_template<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlockTemplateParams = parse_params(body)?;
    if !params.address.is_normal() {
        return Err(InternalRpcError::InvalidParamsAny(
            ApiError::ExpectedNormalAddress.into(),
        ));
    }

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let block = blockchain
        .get_block_template_for_storage(&storage, params.address.into_owned().to_public_key())
        .await
        .context("Error while retrieving block template")?;
    let (difficulty, _) = blockchain
        .get_difficulty_at_tips(&*storage, block.get_tips().iter())
        .await
        .context("Error while retrieving difficulty at tips")?;
    let height = block.height;
    let algorithm = get_pow_algorithm_for_version(block.version);
    let topoheight = blockchain.get_topo_height();
    Ok(json!(GetBlockTemplateResult {
        template: block.to_hex(),
        algorithm,
        height,
        topoheight,
        difficulty
    }))
}

async fn get_miner_work<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetMinerWorkParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    let header = BlockHeader::from_hex(&params.template)?;
    let (difficulty, _) = {
        let storage = blockchain.get_storage().read().await;
        blockchain
            .get_difficulty_at_tips(&*storage, header.get_tips().iter())
            .await
            .context("Error while retrieving difficulty at tips")?
    };
    let version = header.get_version();
    let height = header.get_height();

    let mut work = MinerWork::from_block(header);
    if let Some(address) = params.address {
        if !address.is_normal() {
            return Err(InternalRpcError::InvalidParamsAny(
                ApiError::ExpectedNormalAddress.into(),
            ));
        }

        let blockchain: &Arc<Blockchain<S>> = context.get()?;
        if address.is_mainnet() != blockchain.get_network().is_mainnet() {
            return Err(InternalRpcError::InvalidParamsAny(
                BlockchainError::InvalidNetwork.into(),
            ));
        }

        work.set_miner(Cow::Owned(address.into_owned().to_public_key()));
    }

    let algorithm = get_pow_algorithm_for_version(version);
    let topoheight = blockchain.get_topo_height();

    Ok(json!(GetMinerWorkResult {
        miner_work: work.to_hex(),
        algorithm,
        difficulty,
        height,
        topoheight
    }))
}

async fn submit_block<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: SubmitBlockParams = parse_params(body)?;
    let mut header = BlockHeader::from_hex(&params.block_template)?;
    if let Some(work) = params.miner_work {
        let work = MinerWork::from_hex(&work)?;
        header
            .apply_miner_work(work)
            .map_err(|e| InternalRpcError::InvalidParams(e))?;
    }

    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    let block = blockchain
        .build_block_from_header(Immutable::Owned(header))
        .await?;
    blockchain
        .add_new_block(block, None, BroadcastOption::All, true)
        .await?;
    Ok(json!(true))
}

async fn get_balance<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBalanceParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let (topoheight, version) = storage
        .get_last_balance(params.address.get_public_key(), &params.asset)
        .await
        .context("Error while retrieving last balance")?;
    Ok(json!(GetBalanceResult {
        balance: version.get_balance(),
        topoheight
    }))
}

async fn get_stable_balance<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBalanceParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let top_topoheight = blockchain.get_topo_height();
    let stable_topoheight = blockchain.get_stable_topoheight();
    let storage = blockchain.get_storage().read().await;

    let mut stable_version = None;
    if let Some((output_topoheight, version)) = storage
        .get_output_balance_at_maximum_topoheight(
            params.address.get_public_key(),
            &params.asset,
            top_topoheight,
        )
        .await?
    {
        if output_topoheight >= stable_topoheight {
            stable_version = Some((output_topoheight, version));
        }
    }

    let (stable_topoheight, version) = if let Some((topoheight, version)) = stable_version {
        (topoheight, version)
    } else {
        storage
            .get_balance_at_maximum_topoheight(
                params.address.get_public_key(),
                &params.asset,
                stable_topoheight,
            )
            .await?
            .ok_or(InternalRpcError::InvalidRequestStr(
                "no stable balance found for this account",
            ))?
    };

    Ok(json!(GetStableBalanceResult {
        balance: version.get_balance(),
        stable_topoheight,
        stable_block_hash: storage
            .get_hash_at_topo_height(stable_topoheight)
            .await
            .context("Error while retrieving hash at topo height")?
    }))
}

async fn has_balance<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: HasBalanceParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let key = params.address.get_public_key();
    let storage = blockchain.get_storage().read().await;
    let exist = if let Some(topoheight) = params.topoheight {
        storage
            .has_balance_at_exact_topoheight(key, &params.asset, topoheight)
            .await
            .context("Error while checking balance at topo for account")?
    } else {
        storage
            .has_balance_for(key, &params.asset)
            .await
            .context("Error while checking balance for account")?
    };

    Ok(json!(HasBalanceResult { exist }))
}

async fn get_info<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let height = blockchain.get_height();
    let topoheight = blockchain.get_topo_height();
    let stableheight = blockchain.get_stable_height();
    let stable_topoheight = blockchain.get_stable_topoheight();
    let (top_block_hash, emitted_supply, burned_supply, pruned_topoheight, average_block_time) = {
        let storage = blockchain.get_storage().read().await;
        let top_block_hash = storage
            .get_hash_at_topo_height(topoheight)
            .await
            .context("Error while retrieving hash at topo height")?;
        let emitted_supply = storage
            .get_supply_at_topo_height(topoheight)
            .await
            .context("Error while retrieving supply at topo height")?;
        let burned_supply = storage
            .get_burned_supply_at_topo_height(topoheight)
            .await
            .context("Error while retrieving burned supply at topoheight")?;
        let pruned_topoheight = storage
            .get_pruned_topoheight()
            .await
            .context("Error while retrieving pruned topoheight")?;
        let average_block_time = blockchain
            .get_average_block_time::<S>(&storage)
            .await
            .context("Error while retrieving average block time")?;
        (
            top_block_hash,
            emitted_supply,
            burned_supply,
            pruned_topoheight,
            average_block_time,
        )
    };
    let difficulty = blockchain.get_difficulty().await;

    let mempool_size = blockchain.get_mempool_size().await;
    let version = VERSION.into();
    let network = *blockchain.get_network();
    let block_version = get_version_at_height(&network, height);
    let block_time_target = get_block_time_target_for_version(block_version);

    let block_reward = get_block_reward(emitted_supply, block_time_target);
    let (dev_reward, miner_reward) = get_block_rewards(height, block_reward);

    Ok(json!(GetInfoResult {
        height,
        topoheight,
        stableheight,
        stable_topoheight,
        pruned_topoheight,
        top_block_hash,
        circulating_supply: emitted_supply - burned_supply,
        burned_supply,
        emitted_supply,
        maximum_supply: MAXIMUM_SUPPLY,
        difficulty,
        block_time_target,
        average_block_time,
        block_reward,
        dev_reward,
        miner_reward,
        mempool_size,
        version,
        network,
        block_version: Some(block_version),
    }))
}

async fn get_balance_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBalanceAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let topoheight = blockchain.get_topo_height();
    if params.topoheight > topoheight {
        return Err(InternalRpcError::UnexpectedParams)
            .context("Topoheight cannot be greater than current chain topoheight")?;
    }

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let balance = storage
        .get_balance_at_exact_topoheight(
            params.address.get_public_key(),
            &params.asset,
            params.topoheight,
        )
        .await
        .context("Error while retrieving balance at exact topo height")?;
    Ok(json!(balance))
}

async fn has_nonce<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: HasNonceParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let exist = if let Some(topoheight) = params.topoheight {
        storage
            .has_nonce_at_exact_topoheight(params.address.get_public_key(), topoheight)
            .await
            .context("Error while checking nonce at topo for account")?
    } else {
        storage
            .has_nonce(params.address.get_public_key())
            .await
            .context("Error while checking nonce for account")?
    };

    Ok(json!(HasNonceResult { exist }))
}

async fn get_nonce<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: GetNonceParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let (topoheight, version) = storage
        .get_last_nonce(params.address.get_public_key())
        .await
        .context("Error while retrieving nonce for account")?;

    Ok(json!(GetNonceResult {
        topoheight,
        version
    }))
}

async fn get_nonce_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetNonceAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let topoheight = blockchain.get_topo_height();
    if params.topoheight > topoheight {
        return Err(InternalRpcError::UnexpectedParams)
            .context("Topoheight cannot be greater than current chain topoheight")?;
    }

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let nonce = storage
        .get_nonce_at_exact_topoheight(params.address.get_public_key(), params.topoheight)
        .await
        .context("Error while retrieving nonce at exact topo height")?;
    Ok(json!(nonce))
}

async fn get_asset<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: GetAssetParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let (topoheight, inner) = storage
        .get_asset(&params.asset)
        .await
        .context("Asset was not found")?;
    Ok(json!(RPCAssetData {
        asset: Cow::Borrowed(&params.asset),
        topoheight,
        inner: inner.take()
    }))
}

async fn get_asset_supply<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAssetParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let (topoheight, version) = storage
        .get_asset_supply_at_maximum_topoheight(&params.asset, blockchain.get_topo_height())
        .await
        .context("Asset was not found")?
        .context("No supply available")?;

    Ok(json!(RPCVersioned {
        topoheight,
        version
    }))
}

const MAX_ASSETS: usize = 100;

async fn get_assets<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: GetAssetsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let maximum = if let Some(maximum) = params.maximum {
        if maximum > MAX_ASSETS {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Maximum assets requested cannot be greater than {}",
                MAX_ASSETS
            ))?;
        }
        maximum
    } else {
        MAX_ASSETS
    };
    let skip = params.skip.unwrap_or(0);
    let storage = blockchain.get_storage().read().await;

    // TODO: verify params
    let min = params.minimum_topoheight;
    let max = params.maximum_topoheight;

    let assets = storage
        .get_assets_with_data_in_range(min, max)
        .await?
        .skip(skip)
        .take(maximum);

    let response = assets
        .map(|res| {
            let (asset, topoheight, inner) = res?;
            Ok(RPCAssetData {
                asset: Cow::Owned(asset),
                topoheight,
                inner,
            })
        })
        .collect::<Result<Vec<_>, BlockchainError>>()?;

    Ok(json!(response))
}

async fn count_assets<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let count = storage
        .count_assets()
        .await
        .context("Error while retrieving assets count")?;
    Ok(json!(count))
}

async fn count_accounts<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let count = storage
        .count_accounts()
        .await
        .context("Error while retrieving accounts count")?;
    Ok(json!(count))
}

async fn count_transactions<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let count = storage
        .count_transactions()
        .await
        .context("Error while retrieving transactions count")?;
    Ok(json!(count))
}

async fn count_contracts<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let count = storage
        .count_contracts()
        .await
        .context("Error while retrieving contracts count")?;
    Ok(json!(count))
}

async fn submit_transaction<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: SubmitTransactionParams = parse_params(body)?;
    // x2 because of hex encoding
    if params.data.len() > MAX_TRANSACTION_SIZE * 2 {
        return Err(InternalRpcError::InvalidJSONRequest).context(format!(
            "Transaction size cannot be greater than {}",
            human_bytes(MAX_TRANSACTION_SIZE as f64)
        ))?;
    }

    let transaction = Transaction::from_hex(&params.data)
        .map_err(|err| InternalRpcError::InvalidParamsAny(err.into()))?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    blockchain.add_tx_to_mempool(transaction, true).await?;

    Ok(json!(true))
}

async fn get_transaction<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTransactionParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let mempool = blockchain.get_mempool().read().await;

    get_transaction_response_for_hash(&*storage, &mempool, &params.hash).await
}

async fn get_transaction_executor<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTransactionExecutorParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let block_executor = storage.get_block_executor_for_tx(&params.hash)?;
    let block_topoheight = storage.get_topo_height_for_hash(&block_executor).await?;
    let block_timestamp = storage
        .get_timestamp_for_block_hash(&block_executor)
        .await?;

    Ok(json!(GetTransactionExecutorResult {
        block_topoheight,
        block_timestamp,
        block_hash: Cow::Borrowed(&block_executor)
    }))
}

async fn p2p_status<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let p2p = { blockchain.get_p2p().read().await.clone() };
    match p2p.as_ref() {
        Some(p2p) => {
            let tag = p2p.get_tag();
            let peer_id = p2p.get_peer_id();
            let best_topoheight = p2p.get_best_topoheight().await;
            let median_topoheight = p2p.get_median_topoheight_of_peers().await;
            let max_peers = p2p.get_max_peers();
            let our_topoheight = blockchain.get_topo_height();
            let peer_count = p2p.get_peer_count().await;

            Ok(json!(P2pStatusResult {
                peer_count,
                tag: Cow::Borrowed(tag),
                peer_id,
                our_topoheight,
                best_topoheight,
                median_topoheight,
                max_peers
            }))
        }
        None => Err(InternalRpcError::InvalidParamsAny(ApiError::NoP2p.into())),
    }
}

async fn get_peers<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let p2p = { blockchain.get_p2p().read().await.clone() };
    match p2p.as_ref() {
        Some(p2p) => {
            let peer_list = p2p.get_peer_list();
            let peers_availables = peer_list.get_cloned_peers().await;

            let mut peers = Vec::new();
            for p in peers_availables.iter().filter(|p| p.shareable()) {
                peers.push(get_peer_entry(p).await);
            }

            let total_peers = peers_availables.len();
            let sharable_peers = peers.len();
            Ok(json!(GetPeersResponse {
                peers,
                total_peers,
                hidden_peers: total_peers - sharable_peers,
            }))
        }
        None => Err(InternalRpcError::InvalidParamsAny(ApiError::NoP2p.into())),
    }
}

async fn get_mempool<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetMempoolParams = parse_params(body)?;

    let maximum = params.maximum.filter(|v| *v <= MAX_TXS).unwrap_or(MAX_TXS);
    let skip = params.skip.unwrap_or(0);

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let mempool = blockchain.get_mempool().read().await;
    let mut transactions = Vec::with_capacity(maximum);

    let txs = mempool.get_txs();
    let total = txs.len();
    for (hash, sorted_tx) in txs.iter().skip(skip).take(maximum) {
        let tx = get_transaction_response(
            &*storage,
            sorted_tx.get_tx(),
            hash,
            true,
            Some(sorted_tx.get_first_seen()),
        )
        .await?;
        transactions.push(tx);
    }

    Ok(json!(GetMempoolResult {
        transactions,
        total
    }))
}

pub const MAX_SUMMARY: usize = 1024;

async fn get_mempool_summary<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetMempoolParams = parse_params(body)?;

    let maximum = params
        .maximum
        .filter(|v| *v <= MAX_SUMMARY)
        .unwrap_or(MAX_SUMMARY);

    let skip = params.skip.unwrap_or(0);

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let mempool = blockchain.get_mempool().read().await;
    let txs = mempool.get_txs();
    let total = txs.len();
    let mut transactions = Vec::with_capacity(maximum.max(total));

    let mainnet = blockchain.get_network().is_mainnet();
    for (hash, sorted_tx) in txs.iter().skip(skip).take(maximum) {
        let tx = MempoolTransactionSummary {
            hash: Cow::Borrowed(hash),
            source: sorted_tx.get_tx().get_source().as_address(mainnet),
            fee: sorted_tx.get_fee(),
            first_seen: sorted_tx.get_first_seen(),
            size: sorted_tx.get_size(),
        };

        transactions.push(tx);
    }

    Ok(json!(GetMempoolSummaryResult {
        transactions,
        total
    }))
}

async fn get_estimated_fee_rates<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let mempool = blockchain.get_mempool().read().await;
    let estimated = mempool.estimate_fee_rates()?;
    Ok(json!(estimated))
}

async fn get_blocks_at_height<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlocksAtHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let mut blocks = Vec::new();
    for hash in storage
        .get_blocks_at_height(params.height)
        .await
        .context("Error while retrieving blocks at height")?
    {
        blocks.push(
            get_block_response_for_hash(&blockchain, &storage, &hash, params.include_txs).await?,
        )
    }
    Ok(json!(blocks))
}

async fn get_tips<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let tips = storage
        .get_tips()
        .await
        .context("Error while retrieving tips")?;
    Ok(json!(tips))
}

const MAX_DAG_ORDER: u64 = 64;
// get dag order based on params
// if no params found, get order of last 64 blocks
async fn get_dag_order<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTopoHeightRangeParams = parse_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let current = blockchain.get_topo_height();
    let (start_topoheight, end_topoheight) = get_range(
        params.start_topoheight,
        params.end_topoheight,
        MAX_DAG_ORDER,
        current,
    )?;
    let count = end_topoheight - start_topoheight;

    let storage = blockchain.get_storage().read().await;
    let mut order = Vec::with_capacity(count as usize);
    for i in start_topoheight..=end_topoheight {
        let hash = storage
            .get_hash_at_topo_height(i)
            .await
            .context("Error while retrieving hash at topo height")?;
        order.push(hash);
    }

    Ok(json!(order))
}

const MAX_BLOCKS: u64 = 20;

fn get_range(
    start: Option<TopoHeight>,
    end: Option<TopoHeight>,
    maximum: u64,
    current: TopoHeight,
) -> Result<(TopoHeight, TopoHeight), InternalRpcError> {
    let range_start = start.unwrap_or_else(|| {
        if end.is_none() && current > maximum {
            current - maximum
        } else {
            0
        }
    });

    let range_end = end.unwrap_or(current);
    if range_end < range_start || range_end > current {
        debug!(
            "get range: start = {}, end = {}, max = {}",
            range_start, range_end, current
        );
        return Err(InternalRpcError::InvalidJSONRequest).context(format!(
            "Invalid range requested, start: {}, end: {}",
            range_start, range_end
        ))?;
    }

    let count = range_end - range_start;
    if count > maximum {
        // only retrieve max 20 blocks hash per request
        if log::log_enabled!(log::Level::Debug) {
            debug!("get range requested count: {}", count);
        }
        return Err(InternalRpcError::InvalidJSONRequest).context(format!(
            "Invalid range count requested, received {} but maximum is {}",
            count, maximum
        ))?;
    }

    Ok((range_start, range_end))
}

// get blocks between range of topoheight
// if no params found, get last 20 blocks header
async fn get_blocks_range_by_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTopoHeightRangeParams = parse_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let current_topoheight = blockchain.get_topo_height();
    let (start_topoheight, end_topoheight) = get_range(
        params.start_topoheight,
        params.end_topoheight,
        MAX_BLOCKS,
        current_topoheight,
    )?;

    let storage = blockchain.get_storage().read().await;
    let mut blocks = Vec::with_capacity((end_topoheight - start_topoheight) as usize);
    for i in start_topoheight..=end_topoheight {
        let hash = storage
            .get_hash_at_topo_height(i)
            .await
            .context("Error while retrieving hash at topo height")?;
        let response = get_block_response_for_hash(&blockchain, &storage, &hash, false).await?;
        blocks.push(response);
    }

    Ok(json!(blocks))
}

// get blocks between range of height
// if no params found, get last 20 blocks header
// you can only request
async fn get_blocks_range_by_height<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetHeightRangeParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let current_height = blockchain.get_height();
    let (start_height, end_height) = get_range(
        params.start_height,
        params.end_height,
        MAX_BLOCKS,
        current_height,
    )?;

    let storage = blockchain.get_storage().read().await;
    let mut blocks = Vec::with_capacity((end_height - start_height) as usize);
    for i in start_height..=end_height {
        let blocks_at_height = storage
            .get_blocks_at_height(i)
            .await
            .context("Error while retrieving blocks at height")?;
        for hash in blocks_at_height {
            let response = get_block_response_for_hash(&blockchain, &storage, &hash, false).await?;
            blocks.push(response);
        }
    }

    Ok(json!(blocks))
}

const MAX_TXS: usize = 20;
// get up to 20 transactions at once
// if a tx hash is not present, we keep the order and put json "null" value
async fn get_transactions<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTransactionsParams = parse_params(body)?;

    let hashes = params.tx_hashes;
    if hashes.len() > MAX_TXS {
        return Err(InternalRpcError::InvalidJSONRequest).context(format!(
            "Too many requested txs: {}, maximum is {}",
            hashes.len(),
            MAX_TXS
        ))?;
    }

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let mempool = blockchain.get_mempool().read().await;
    let mut transactions: Vec<Option<Value>> = Vec::with_capacity(hashes.len());
    for hash in hashes {
        let tx = match get_transaction_response_for_hash(&*storage, &mempool, &hash).await {
            Ok(data) => Some(data),
            Err(e) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Error while retrieving tx {} from storage: {}", hash, e);
                }
                None
            }
        };
        transactions.push(tx);
    }

    Ok(json!(transactions))
}

const MAX_TXS_SUMMARY: usize = 100;

// get up to 100 transactions summary at once
// if a tx hash is not present, we keep the order and put json "null" value
async fn get_transactions_summary<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTransactionsParams = parse_params(body)?;

    let hashes = params.tx_hashes;
    if hashes.len() > MAX_TXS_SUMMARY {
        return Err(InternalRpcError::InvalidJSONRequest).context(format!(
            "Too many requested txs: {}, maximum is {}",
            hashes.len(),
            MAX_TXS
        ))?;
    }

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let mut transactions = Vec::with_capacity(hashes.len());
    for hash in hashes {
        let tx = if let Some(tx) = blockchain.get_tx(&hash).await.ok() {
            Some(TransactionSummary {
                hash: Cow::Owned(hash),
                source: tx
                    .get_source()
                    .as_address(blockchain.get_network().is_mainnet()),
                fee: tx.get_fee(),
                size: tx.size(),
            })
        } else {
            None
        };
        transactions.push(tx);
    }

    Ok(json!(transactions))
}

const MAX_HISTORY: usize = 20;
// retrieve all history changes for an account on an asset
async fn get_account_history<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAccountHistoryParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    if !params.incoming_flow && !params.outgoing_flow {
        return Err(InternalRpcError::InvalidParams(
            "No history type was selected",
        ));
    }

    let key = params.address.get_public_key();
    let minimum_topoheight = params.minimum_topoheight.unwrap_or(0);
    let storage = blockchain.get_storage().read().await;
    let pruned_topoheight = storage
        .get_pruned_topoheight()
        .await
        .context("Error while retrieving pruned topoheight")?
        .unwrap_or(0);
    let mut version: Option<(u64, Option<u64>, _)> = if let Some(topo) = params.maximum_topoheight {
        if topo < pruned_topoheight {
            return Err(InternalRpcError::InvalidParams(
                "Maximum topoheight is lower than pruned topoheight",
            ));
        }

        // if incoming flows aren't accepted
        // use nonce versions to determine topoheight
        if !params.incoming_flow {
            if let Some((topo, nonce)) = storage
                .get_nonce_at_maximum_topoheight(key, topo)
                .await
                .context("Error while retrieving last nonce")?
            {
                let version = storage
                    .get_balance_at_exact_topoheight(key, &params.asset, topo)
                    .await
                    .context(format!(
                        "Error while retrieving balance at nonce topo height {topo}"
                    ))?;
                Some((topo, nonce.get_previous_topoheight(), version))
            } else {
                None
            }
        } else {
            storage
                .get_balance_at_maximum_topoheight(key, &params.asset, topo)
                .await
                .context(format!(
                    "Error while retrieving balance at topo height {topo}"
                ))?
                .map(|(topo, version)| (topo, None, version))
        }
    } else {
        if !params.incoming_flow {
            // don't return any error, maybe this account never spend anything
            // (even if we force 0 nonce at first activity)
            let (topo, nonce) = storage
                .get_last_nonce(key)
                .await
                .context("Error while retrieving last topoheight for nonce")?;
            let version = storage
                .get_balance_at_exact_topoheight(key, &params.asset, topo)
                .await
                .context(format!(
                    "Error while retrieving balance at topo height {topo}"
                ))?;
            Some((topo, nonce.get_previous_topoheight(), version))
        } else {
            Some(
                storage
                    .get_last_balance(key, &params.asset)
                    .await
                    .map(|(topo, version)| (topo, None, version))
                    .context("Error while retrieving last balance")?,
            )
        }
    };

    let mut history_count = 0;
    let mut history = Vec::new();

    let is_dev_address = *key == *DEV_PUBLIC_KEY;
    while let Some((topo, prev_nonce, versioned_balance)) = version.take() {
        trace!(
            "Searching history of {} ({}) at topoheight {}, nonce: {:?}, type: {:?}",
            params.address,
            params.asset,
            topo,
            prev_nonce,
            versioned_balance.get_balance_type()
        );
        if topo < minimum_topoheight || topo < pruned_topoheight {
            break;
        }

        // Get the block header at topoheight
        // we will scan it below for transactions and rewards
        let (hash, block_header) =
            storage
                .get_block_header_at_topoheight(topo)
                .await
                .context(format!(
                    "Error while retrieving block header at topo height {topo}"
                ))?;

        // Block reward is only paid in TOS
        if params.asset == TOS_ASSET {
            let is_miner = *block_header.get_miner() == *key;
            if (is_miner || is_dev_address) && params.incoming_flow {
                let mut reward = storage
                    .get_block_reward_at_topo_height(topo)
                    .context(format!(
                        "Error while retrieving reward at topo height {topo}"
                    ))?;
                // subtract dev fee if any
                let dev_fee_percentage = get_block_dev_fee(block_header.get_height());
                if dev_fee_percentage != 0 {
                    let dev_fee = reward * dev_fee_percentage / 100;
                    if is_dev_address {
                        history.push(AccountHistoryEntry {
                            topoheight: topo,
                            hash: hash.clone(),
                            history_type: AccountHistoryType::DevFee { reward: dev_fee },
                            block_timestamp: block_header.get_timestamp(),
                        });
                    }
                    reward -= dev_fee;
                }

                if is_miner {
                    let history_type = AccountHistoryType::Mining { reward };
                    history.push(AccountHistoryEntry {
                        topoheight: topo,
                        hash: hash.clone(),
                        history_type,
                        block_timestamp: block_header.get_timestamp(),
                    });
                }
            }
        }

        // Reverse the order of transactions to get the latest first
        for tx_hash in block_header.get_transactions().iter().rev() {
            // Don't show unexecuted TXs in the history
            if !storage.is_tx_executed_in_block(tx_hash, &hash)? {
                continue;
            }

            if log::log_enabled!(log::Level::Trace) {
                trace!("Searching tx {} in block {}", tx_hash, hash);
            }
            let tx = storage.get_transaction(tx_hash).await.context(format!(
                "Error while retrieving transaction {tx_hash} from block {hash}"
            ))?;
            let is_sender = *tx.get_source() == *key;
            match tx.get_data() {
                TransactionType::Transfers(transfers) => {
                    for transfer in transfers {
                        if *transfer.get_asset() == params.asset {
                            if *transfer.get_destination() == *key && params.incoming_flow {
                                history.push(AccountHistoryEntry {
                                    topoheight: topo,
                                    hash: tx_hash.clone(),
                                    history_type: AccountHistoryType::Incoming {
                                        from: tx
                                            .get_source()
                                            .as_address(blockchain.get_network().is_mainnet()),
                                    },
                                    block_timestamp: block_header.get_timestamp(),
                                });
                            }

                            if is_sender && params.outgoing_flow {
                                history.push(AccountHistoryEntry {
                                    topoheight: topo,
                                    hash: tx_hash.clone(),
                                    history_type: AccountHistoryType::Outgoing {
                                        to: transfer
                                            .get_destination()
                                            .as_address(blockchain.get_network().is_mainnet()),
                                    },
                                    block_timestamp: block_header.get_timestamp(),
                                });
                            }
                        }
                    }
                }
                TransactionType::Burn(payload) => {
                    if payload.asset == params.asset {
                        if is_sender && params.outgoing_flow {
                            history.push(AccountHistoryEntry {
                                topoheight: topo,
                                hash: tx_hash.clone(),
                                history_type: AccountHistoryType::Burn {
                                    amount: payload.amount,
                                },
                                block_timestamp: block_header.get_timestamp(),
                            });
                        }
                    }
                }
                TransactionType::MultiSig(payload) => {
                    if is_sender {
                        let mainnet = blockchain.get_network().is_mainnet();
                        history.push(AccountHistoryEntry {
                            topoheight: topo,
                            hash: tx_hash.clone(),
                            history_type: AccountHistoryType::MultiSig {
                                participants: payload
                                    .participants
                                    .iter()
                                    .map(|p| p.as_address(mainnet))
                                    .collect(),
                                threshold: payload.threshold,
                            },
                            block_timestamp: block_header.get_timestamp(),
                        });
                    }
                }
                TransactionType::InvokeContract(payload) => {
                    if is_sender {
                        history.push(AccountHistoryEntry {
                            topoheight: topo,
                            hash: tx_hash.clone(),
                            history_type: AccountHistoryType::InvokeContract {
                                contract: payload.contract.clone(),
                                entry_id: payload.entry_id,
                            },
                            block_timestamp: block_header.get_timestamp(),
                        });
                    }
                }
                TransactionType::DeployContract(_) => {
                    if is_sender {
                        history.push(AccountHistoryEntry {
                            topoheight: topo,
                            hash: tx_hash.clone(),
                            history_type: AccountHistoryType::DeployContract,
                            block_timestamp: block_header.get_timestamp(),
                        });
                    }
                }
                TransactionType::Energy(payload) => {
                    if is_sender {
                        match payload {
                            tos_common::transaction::EnergyPayload::FreezeTos {
                                amount,
                                duration,
                            } => {
                                history.push(AccountHistoryEntry {
                                    topoheight: topo,
                                    hash: tx_hash.clone(),
                                    history_type: AccountHistoryType::FreezeTos {
                                        amount: *amount,
                                        duration: format!("{}_days", duration.get_days()),
                                    },
                                    block_timestamp: block_header.get_timestamp(),
                                });
                            }
                            tos_common::transaction::EnergyPayload::UnfreezeTos { amount } => {
                                history.push(AccountHistoryEntry {
                                    topoheight: topo,
                                    hash: tx_hash.clone(),
                                    history_type: AccountHistoryType::UnfreezeTos {
                                        amount: *amount,
                                    },
                                    block_timestamp: block_header.get_timestamp(),
                                });
                            }
                        }
                    }
                }
                TransactionType::AIMining(_) => {
                    // AI Mining transactions don't affect account history for now
                    // This could be extended to track AI mining activities
                }
                TransactionType::BindReferrer(payload) => {
                    if is_sender {
                        history.push(AccountHistoryEntry {
                            topoheight: topo,
                            hash: tx_hash.clone(),
                            history_type: AccountHistoryType::BindReferrer {
                                referrer: payload
                                    .get_referrer()
                                    .as_address(blockchain.get_network().is_mainnet()),
                            },
                            block_timestamp: block_header.get_timestamp(),
                        });
                    }
                }
                TransactionType::BatchReferralReward(_) => {
                    // BatchReferralReward transactions are tracked by the referral system
                    // History entries for individual upline rewards would require additional storage
                    // For now, similar to AIMining, we don't add to account history
                }
                // KYC transaction types
                TransactionType::SetKyc(_)
                | TransactionType::RevokeKyc(_)
                | TransactionType::RenewKyc(_)
                | TransactionType::TransferKyc(_)
                | TransactionType::BootstrapCommittee(_)
                | TransactionType::RegisterCommittee(_)
                | TransactionType::UpdateCommittee(_)
                | TransactionType::EmergencySuspend(_) => {
                    // KYC transactions don't affect account history for now
                    // This could be extended to track KYC activities
                }
            }
        }

        history_count += 1;
        if history_count >= MAX_HISTORY {
            break;
        }

        // if incoming flows aren't accepted
        // use nonce versions to determine topoheight
        if let Some(previous) = prev_nonce.filter(|_| !params.incoming_flow) {
            let nonce_version = storage
                .get_nonce_at_exact_topoheight(key, previous)
                .await
                .context(format!(
                    "Error while retrieving nonce at topo height {previous}"
                ))?;
            version = Some((
                previous,
                nonce_version.get_previous_topoheight(),
                storage
                    .get_balance_at_exact_topoheight(key, &params.asset, previous)
                    .await
                    .context(format!(
                        "Error while retrieving previous balance at topo height {previous}"
                    ))?,
            ));
        } else if let Some(previous) = versioned_balance
            .get_previous_topoheight()
            .filter(|_| params.incoming_flow)
        {
            if previous < pruned_topoheight {
                break;
            }

            version = Some((
                previous,
                None,
                storage
                    .get_balance_at_exact_topoheight(key, &params.asset, previous)
                    .await
                    .context(format!(
                        "Error while retrieving previous balance at topo height {previous}"
                    ))?,
            ));
        }
    }

    Ok(json!(history))
}

async fn get_account_assets<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAccountAssetsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let maximum = if let Some(maximum) = params.maximum {
        if maximum > MAX_ACCOUNTS {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Maximum accounts requested cannot be greater than {}",
                MAX_ACCOUNTS
            ))?;
        }
        maximum
    } else {
        MAX_ACCOUNTS
    };
    let skip = params.skip.unwrap_or(0);

    let key = params.address.get_public_key();
    let storage = blockchain.get_storage().read().await;
    let assets: Vec<_> = storage
        .get_assets_for(key)
        .await?
        .skip(skip)
        .take(maximum)
        .collect::<Result<_, BlockchainError>>()
        .context("Error while retrieving assets for account")?;
    Ok(json!(assets))
}

const MAX_ACCOUNTS: usize = 100;
// retrieve all available accounts (each account got at least one interaction on chain)
async fn get_accounts<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAccountsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let topoheight = blockchain.get_topo_height();
    let maximum = if let Some(maximum) = params.maximum {
        if maximum > MAX_ACCOUNTS {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Maximum accounts requested cannot be greater than {}",
                MAX_ACCOUNTS
            ))?;
        }
        maximum
    } else {
        MAX_ACCOUNTS
    };
    let skip = params.skip.unwrap_or(0);
    let minimum_topoheight = if let Some(minimum) = params.minimum_topoheight {
        if minimum > topoheight {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Minimum topoheight requested cannot be greater than {}",
                topoheight
            ))?;
        }

        minimum
    } else {
        0
    };
    let maximum_topoheight = if let Some(maximum) = params.maximum_topoheight {
        if maximum > topoheight {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Maximum topoheight requested cannot be greater than {}",
                topoheight
            ))?;
        }

        if maximum < minimum_topoheight {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Maximum topoheight requested must be greater or equal to {}",
                minimum_topoheight
            ))?;
        }
        maximum
    } else {
        topoheight
    };

    let storage = blockchain.get_storage().read().await;
    let mainnet = storage.is_mainnet();
    let accounts = storage
        .get_registered_keys(Some(minimum_topoheight), Some(maximum_topoheight))
        .await?
        .skip(skip)
        .take(maximum)
        .map(|key| key.map(|key| key.to_address(mainnet)))
        .collect::<Result<Vec<_>, BlockchainError>>()?;

    Ok(json!(accounts))
}

// Check if the account is registered on chain or not
async fn is_account_registered<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: IsAccountRegisteredParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let key = params.address.get_public_key();
    let registered = if params.in_stable_height {
        storage
            .is_account_registered_for_topoheight(key, blockchain.get_stable_topoheight())
            .await
            .context("Error while checking if account is registered in stable height")?
    } else {
        storage
            .is_account_registered(key)
            .await
            .context("Error while checking if account is registered")?
    };

    Ok(json!(registered))
}

// Search the account registration topoheight
async fn get_account_registration_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAccountRegistrationParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let key = params.address.get_public_key();
    let topoheight = storage
        .get_account_registration_topoheight(key)
        .await
        .context("Error while retrieving registration topoheight")?;
    Ok(json!(topoheight))
}

// Check if the asked TX is executed in the block
async fn is_tx_executed_in_block<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: IsTxExecutedInBlockParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    Ok(json!(storage
        .is_tx_executed_in_block(&params.tx_hash, &params.block_hash)
        .context(
            "Error while checking if tx was executed in block"
        )?))
}

// Get the configured dev fees
async fn get_dev_fee_thresholds<S: Storage>(
    _: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;

    Ok(json!(DEV_FEES))
}

// Get size on disk of the chain database
async fn get_size_on_disk<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let size_bytes = storage
        .get_size_on_disk()
        .await
        .context("Error while retrieving size on disk")?;
    let size_formatted = human_bytes(size_bytes as f64);

    Ok(json!(SizeOnDiskResult {
        size_bytes,
        size_formatted
    }))
}

// Retrieve the mempool cache for an account
async fn get_mempool_cache<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetMempoolCacheParams = parse_params(body)?;
    if !params.address.is_normal() {
        return Err(InternalRpcError::InvalidParamsAny(
            ApiError::ExpectedNormalAddress.into(),
        ));
    }

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let mempool = blockchain.get_mempool().read().await;
    let cache = mempool
        .get_cache_for(params.address.get_public_key())
        .context("Account not found while retrieving mempool cache")?;

    Ok(json!(cache))
}

async fn get_difficulty<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let difficulty = blockchain.get_difficulty().await;
    let version = get_version_at_height(blockchain.get_network(), blockchain.get_height());
    let block_time_target = get_block_time_target_for_version(version);

    let hashrate = difficulty / (block_time_target / MILLIS_PER_SECOND);
    let hashrate_formatted = format_hashrate(hashrate.into());
    Ok(json!(GetDifficultyResult {
        hashrate,
        hashrate_formatted,
        difficulty,
    }))
}

async fn validate_address<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: ValidateAddressParams = parse_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    Ok(json!(ValidateAddressResult {
        is_valid: (params.address.is_normal()
            || (!params.address.is_normal() && params.allow_integrated))
            && params
                .max_integrated_data_size
                .and_then(|size| params
                    .address
                    .get_extra_data()
                    .map(|data| data.size() <= size))
                .unwrap_or(true),
        is_integrated: !params.address.is_normal(),
    }))
}

async fn extract_key_from_address<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: ExtractKeyFromAddressParams = parse_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    if params.as_hex {
        Ok(json!(ExtractKeyFromAddressResult::Hex(
            params.address.get_public_key().to_hex()
        )))
    } else {
        Ok(json!(ExtractKeyFromAddressResult::Bytes(
            params.address.get_public_key().to_bytes()
        )))
    }
}

// Split an integrated address into its address and data
async fn split_address<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: SplitAddressParams = parse_params(body)?;
    let address = params.address;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let (data, address) = address.extract_data();
    let integrated_data = data.ok_or(InternalRpcError::InvalidParams(
        "Address is not an integrated address",
    ))?;
    let size = integrated_data.size();
    Ok(json!(SplitAddressResult {
        address,
        integrated_data,
        size,
    }))
}

async fn make_integrated_address<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: MakeIntegratedAddressParams = parse_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    if !params.address.is_normal() {
        return Err(InternalRpcError::InvalidParams(
            "Address is not a normal address",
        ));
    }

    let address = Address::new(
        params.address.is_mainnet(),
        AddressType::Data(params.integrated_data.into_owned()),
        params.address.into_owned().to_public_key(),
    );

    Ok(json!(address))
}

async fn decrypt_extra_data<S: Storage>(
    _: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: DecryptExtraDataParams = parse_params(body)?;
    let data = params
        .extra_data
        .decrypt_with_shared_key(&params.shared_key)
        .context("Error while decrypting using provided shared key")?;

    Ok(json!(data))
}

async fn get_multisig_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetMultisigAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let multisig = storage
        .get_multisig_at_topoheight_for(&params.address.get_public_key(), params.topoheight)
        .await
        .context("Error while retrieving multisig at topoheight")?;

    let state = match multisig.take() {
        Some(multisig) => {
            let multisig = multisig.into_owned();
            let mainnet = storage.is_mainnet();
            let participants = multisig
                .participants
                .into_iter()
                .map(|p| p.to_address(mainnet))
                .collect();
            MultisigState::Active {
                participants,
                threshold: multisig.threshold,
            }
        }
        None => MultisigState::Deleted,
    };

    Ok(json!(GetMultisigAtTopoHeightResult { state }))
}

async fn get_multisig<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetMultisigParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let (topoheight, multisig) = storage
        .get_last_multisig(&params.address.get_public_key())
        .await
        .context("Error while retrieving multisig")?;

    let state = match multisig.take() {
        Some(multisig) => {
            let multisig = multisig.into_owned();
            let mainnet = storage.is_mainnet();
            let participants = multisig
                .participants
                .into_iter()
                .map(|p| p.to_address(mainnet))
                .collect();
            MultisigState::Active {
                participants,
                threshold: multisig.threshold,
            }
        }
        None => MultisigState::Deleted,
    };

    Ok(json!(GetMultisigResult { state, topoheight }))
}

async fn has_multisig<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: HasMultisigParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let multisig = storage
        .has_multisig(&params.address.get_public_key())
        .await
        .context("Error while checking if account has multisig")?;

    Ok(json!(multisig))
}

async fn has_multisig_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: HasMultisigAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let multisig = storage
        .has_multisig_at_exact_topoheight(&params.address.get_public_key(), params.topoheight)
        .await
        .context("Error while checking if account has multisig at topoheight")?;

    Ok(json!(multisig))
}

async fn get_contract_outputs<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractOutputsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let is_mainnet = blockchain.get_network().is_mainnet();
    let storage = blockchain.get_storage().read().await;
    let outputs = storage
        .get_contract_outputs_for_tx(&params.transaction)
        .await
        .context("Error while retrieving contract outputs")?;

    let rpc_outputs = outputs
        .iter()
        .map(|output| RPCContractOutput::from_output(&output, is_mainnet))
        .collect::<Vec<_>>();

    Ok(json!(rpc_outputs))
}

async fn get_contract_module<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractModuleParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let Some(topoheight) = storage
        .get_last_topoheight_for_contract(&params.contract)
        .await?
    else {
        return Err(InternalRpcError::InvalidParams(
            "no contract module available",
        ));
    };
    let module = storage
        .get_contract_at_topoheight_for(&params.contract, topoheight)
        .await
        .context("Error while retrieving contract module")?;

    Ok(json!(module))
}

async fn get_contract_data<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractDataParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let topoheight = storage
        .get_last_topoheight_for_contract_data(&params.contract, &params.key)
        .await?
        .context("No data found with requested key")?;

    let version = storage
        .get_contract_data_at_exact_topoheight_for(&params.contract, &params.key, topoheight)
        .await?;

    Ok(json!(RPCVersioned {
        topoheight,
        version,
    }))
}

async fn get_contract_data_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractDataAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let version = storage
        .get_contract_data_at_exact_topoheight_for(&params.contract, &params.key, params.topoheight)
        .await?;

    Ok(json!(version))
}

async fn get_contract_balance<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    use crate::core::error::BlockchainError;

    let params: GetContractBalanceParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    match storage
        .get_last_contract_balance(&params.contract, &params.asset)
        .await
    {
        Ok((topoheight, version)) => Ok(json!(RPCVersioned {
            topoheight,
            version,
        })),
        Err(BlockchainError::NoContractBalance) => {
            // No balance record means balance is 0
            Ok(json!(RPCVersioned {
                topoheight: 0,
                version: 0u64,
            }))
        }
        Err(e) => Err(e).context("Error while retrieving contract balance")?,
    }
}

async fn get_contract_assets<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractBalancesParams = parse_params(body)?;
    let maximum = if let Some(maximum) = params.maximum {
        if maximum > MAX_ASSETS {
            return Err(InternalRpcError::InvalidJSONRequest).context(format!(
                "Maximum assets requested cannot be greater than {}",
                MAX_ASSETS
            ))?;
        }
        maximum
    } else {
        MAX_ASSETS
    };

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let iter = storage
        .get_contract_assets_for(&params.contract)
        .await
        .context("Error while retrieving contract balance")?;

    let assets = iter
        .skip(params.skip.unwrap_or_default())
        .take(maximum)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!(assets))
}

async fn get_contract_balance_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractBalanceAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let version = storage
        .get_contract_balance_at_exact_topoheight(
            &params.contract,
            &params.asset,
            params.topoheight,
        )
        .await
        .context("Error while retrieving contract balance")?;

    Ok(json!(version))
}

async fn get_p2p_block_propagation<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetP2pBlockPropagation = parse_params(body)?;

    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let p2p = { blockchain.get_p2p().read().await.clone() }
        .ok_or(InternalRpcError::InvalidParamsAny(ApiError::NoP2p.into()))?;

    let mut peers = HashMap::new();
    let mut first_seen = None;

    let hash = params.hash.into_owned();
    for peer in p2p.get_peer_list().get_cloned_peers().await {
        let blocks_propagation = peer.get_blocks_propagation().lock().await;
        if let Some((timed_direction, is_common)) = blocks_propagation.peek(&hash).copied() {
            // We don't count common peers
            // Because we haven't really sent them it
            if !is_common {
                if (timed_direction.contains_out() && params.outgoing)
                    || (timed_direction.contains_in() && params.incoming)
                {
                    peers.insert(peer.get_id(), timed_direction);
                }

                match timed_direction {
                    TimedDirection::In { received_at }
                    | TimedDirection::Both { received_at, .. } => {
                        if first_seen.map(|v| v > received_at).unwrap_or(true) {
                            first_seen = Some(received_at);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let processing_at = p2p.get_block_propagation_timestamp(&hash).await;
    Ok(json!(P2pBlockPropagationResult {
        peers,
        first_seen,
        processing_at
    }))
}

// Energy management RPC methods

/// Get energy information for an account
async fn get_energy<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: GetEnergyParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    // Get current topoheight
    let current_topoheight = storage.get_top_height().await?;

    // Get energy resource for the account
    let pubkey = params.address.into_owned().to_public_key();
    let energy_resource = storage.get_energy_resource(&pubkey).await?;

    let result = if let Some(energy_resource) = energy_resource {
        // Convert freeze records to FreezeRecordInfo format
        let freeze_records = energy_resource
            .freeze_records
            .iter()
            .map(|record| FreezeRecordInfo {
                amount: record.amount,
                duration: format!("{}_days", record.duration.get_days()),
                freeze_topoheight: record.freeze_topoheight,
                unlock_topoheight: record.unlock_topoheight,
                energy_gained: record.energy_gained,
                can_unlock: record.can_unlock(current_topoheight),
                remaining_blocks: if record.can_unlock(current_topoheight) {
                    0
                } else {
                    record.unlock_topoheight.saturating_sub(current_topoheight)
                },
            })
            .collect::<Vec<_>>();

        json!(GetEnergyResult {
            frozen_tos: energy_resource.frozen_tos,
            total_energy: energy_resource.total_energy,
            used_energy: energy_resource.used_energy,
            available_energy: energy_resource.available_energy(),
            last_update: energy_resource.last_update,
            freeze_records,
        })
    } else {
        json!(GetEnergyResult {
            frozen_tos: 0,
            total_energy: 0,
            used_energy: 0,
            available_energy: 0,
            last_update: current_topoheight,
            freeze_records: Vec::new(),
        })
    };

    Ok(result)
}

// Get the current AI mining state
async fn get_ai_mining_state<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let state = storage.get_ai_mining_state().await?;
    Ok(json!(state))
}

// Get AI mining state at a specific topoheight
async fn get_ai_mining_state_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    #[derive(serde::Deserialize)]
    struct Params {
        topoheight: u64,
    }

    let params: Params = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let state = storage
        .get_ai_mining_state_at_topoheight(params.topoheight)
        .await?;
    Ok(json!(state))
}

// Check if AI mining state exists at a specific topoheight
async fn has_ai_mining_state_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    #[derive(serde::Deserialize)]
    struct Params {
        topoheight: u64,
    }

    let params: Params = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let has_state = storage
        .has_ai_mining_state_at_topoheight(params.topoheight)
        .await?;
    Ok(json!(has_state))
}

// Get only AI mining statistics
async fn get_ai_mining_statistics<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let state = storage.get_ai_mining_state().await?;
    match state {
        Some(ai_state) => Ok(json!(ai_state.statistics)),
        None => {
            // Return default empty statistics when no AI mining activity yet
            Ok(json!(AIMiningStatistics::default()))
        }
    }
}

// Get a specific AI mining task by ID
async fn get_ai_mining_task<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    #[derive(serde::Deserialize)]
    struct Params {
        task_id: Hash,
    }

    let params: Params = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let state = storage.get_ai_mining_state().await?;

    match state {
        Some(ai_state) => match ai_state.tasks.get(&params.task_id) {
            Some(task) => Ok(json!(task)),
            None => Err(InternalRpcError::InvalidRequestStr("Task not found")),
        },
        None => Err(InternalRpcError::InvalidRequestStr(
            "AI mining state not initialized",
        )),
    }
}

// Get a specific miner's information by address
async fn get_ai_mining_miner<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    #[derive(serde::Deserialize)]
    struct Params {
        address: CompressedPublicKey,
    }

    let params: Params = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let state = storage.get_ai_mining_state().await?;

    match state {
        Some(ai_state) => match ai_state.miners.get(&params.address) {
            Some(miner) => Ok(json!(miner)),
            None => Err(InternalRpcError::InvalidRequestStr("Miner not found")),
        },
        None => Err(InternalRpcError::InvalidRequestStr(
            "AI mining state not initialized",
        )),
    }
}

// Get all active AI mining tasks
async fn get_ai_mining_active_tasks<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let state = storage.get_ai_mining_state().await?;

    match state {
        Some(ai_state) => {
            let active_tasks: std::collections::HashMap<&Hash, &AIMiningTask> = ai_state
                .tasks
                .iter()
                .filter(|(_, task)| matches!(task.status, TaskStatus::Active))
                .collect();

            Ok(json!(active_tasks))
        }
        None => {
            // Return empty map when no AI mining activity yet
            let empty: std::collections::HashMap<Hash, AIMiningTask> =
                std::collections::HashMap::new();
            Ok(json!(empty))
        }
    }
}

// Get contract address from a DeployContract transaction
async fn get_contract_address_from_tx<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractAddressFromTxParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let is_mainnet = blockchain.get_network().is_mainnet();
    let storage = blockchain.get_storage().read().await;

    // Load the transaction
    let tx = storage
        .get_transaction(&params.transaction)
        .await
        .context("Transaction not found")?;

    // Check if it's a DeployContract transaction
    let TransactionType::DeployContract(payload) = tx.get_data() else {
        return Err(InternalRpcError::InvalidParams(
            "Transaction is not a DeployContract transaction",
        ));
    };

    // Get the bytecode from the module
    let bytecode = payload
        .module
        .get_bytecode()
        .map(|b| b.to_vec())
        .unwrap_or_default();

    // Compute the deterministic contract address
    let contract_address =
        tos_common::crypto::compute_deterministic_contract_address(tx.get_source(), &bytecode);

    // Get deployer's address for reference
    let deployer = tx.get_source().as_address(is_mainnet);

    Ok(json!(GetContractAddressFromTxResult {
        contract_address,
        deployer: deployer.to_string(),
    }))
}

/// Get contract events (LOG0-LOG4 syscalls) with filtering options
///
/// This endpoint allows querying contract events by:
/// - Contract address (with optional topic0 filter)
/// - Transaction hash
/// - Topoheight range
///
/// Returns a list of events matching the filters.
async fn get_contract_events<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractEventsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let mut events = Vec::new();

    // Query by transaction hash takes priority
    if let Some(tx_hash) = &params.tx_hash {
        let tx_events = storage.get_events_by_tx(tx_hash).await?;
        for event in tx_events {
            events.push(RPCContractEvent {
                contract: event.contract,
                tx_hash: event.tx_hash,
                block_hash: event.block_hash,
                topoheight: event.topoheight,
                log_index: event.log_index,
                topics: event.topics.iter().map(hex::encode).collect(),
                data: hex::encode(&event.data),
            });
        }
    } else if let Some(contract) = &params.contract {
        // Query by contract (with optional topic0 filter)
        let stored_events = if let Some(topic0_hex) = &params.topic0 {
            // Parse topic0 from hex
            let topic0_bytes = hex::decode(topic0_hex)
                .map_err(|_| InternalRpcError::InvalidParams("Invalid topic0 hex string"))?;
            if topic0_bytes.len() != 32 {
                return Err(InternalRpcError::InvalidParams("topic0 must be 32 bytes"));
            }
            let mut topic0 = [0u8; 32];
            topic0.copy_from_slice(&topic0_bytes);

            storage
                .get_events_by_topic(
                    contract,
                    &topic0,
                    params.from_topoheight,
                    params.to_topoheight,
                    params.limit,
                )
                .await?
        } else {
            storage
                .get_events_by_contract(
                    contract,
                    params.from_topoheight,
                    params.to_topoheight,
                    params.limit,
                )
                .await?
        };

        for event in stored_events {
            events.push(RPCContractEvent {
                contract: event.contract,
                tx_hash: event.tx_hash,
                block_hash: event.block_hash,
                topoheight: event.topoheight,
                log_index: event.log_index,
                topics: event.topics.iter().map(hex::encode).collect(),
                data: hex::encode(&event.data),
            });
        }
    } else {
        return Err(InternalRpcError::InvalidParams(
            "Either 'contract' or 'tx_hash' parameter is required",
        ));
    }

    Ok(json!(events))
}

// ============================================================================
// QR Code Payment RPC Methods
// ============================================================================

use tos_common::api::{
    callback::{
        RegisterWebhookParams, RegisterWebhookResult, UnregisterWebhookParams,
        UnregisterWebhookResult,
    },
    payment::{
        decode_payment_extra_data, is_valid_payment_id, validate_payment_id,
        CreatePaymentRequestParams, CreatePaymentRequestResult, GetPaymentStatusParams,
        ParsePaymentRequestParams, ParsePaymentRequestResult, PaymentParseError, PaymentRequest,
        PaymentStatus, PaymentStatusResponse,
    },
};

/// Maximum expiration time for payment requests (1 hour)
const MAX_PAYMENT_EXPIRATION_SECS: u64 = 3600;

/// Default expiration time for payment requests (5 minutes)
const DEFAULT_PAYMENT_EXPIRATION_SECS: u64 = 300;

/// Default number of blocks to scan when searching for payments (~10 min at 3s/block)
const DEFAULT_SCAN_BLOCKS: u64 = 200;

/// Number of confirmations required for a payment to be considered stable
const STABLE_CONFIRMATIONS: u64 = 8;

/// Create a payment request and return the QR code URI
async fn create_payment_request<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: CreatePaymentRequestParams = parse_params(body)?;
    let _blockchain: &Arc<Blockchain<S>> = context.get()?;

    // Validate the address
    if !params.address.is_normal() {
        return Err(invalid_params_data(
            "invalid_address",
            "Address must be in normal format (not integrated)",
        ));
    }

    // Validate and cap expiration time
    let expires_in = params
        .expires_in_seconds
        .unwrap_or(DEFAULT_PAYMENT_EXPIRATION_SECS)
        .min(MAX_PAYMENT_EXPIRATION_SECS);

    // Generate a unique payment ID
    let payment_id = generate_payment_id();
    if !is_valid_payment_id(&payment_id) {
        return Err(invalid_params_data(
            "invalid_payment_id",
            "Generated payment ID is invalid",
        ));
    }

    // Build the payment request
    let mut request = PaymentRequest::new(payment_id.clone(), params.address.clone());

    if let Some(amount) = params.amount {
        request = request.with_amount(amount);
    }

    if let Some(asset) = params.asset.clone() {
        request = request.with_asset(asset);
    }

    if let Some(ref memo) = params.memo {
        // Validate memo length
        if memo.len() > PaymentRequest::MAX_MEMO_LENGTH {
            return Err(invalid_params_data(
                "memo_too_long",
                "Memo exceeds maximum length of 64 bytes",
            ));
        }
        request = request.with_memo(memo.clone());
    }

    if let Some(ref callback) = params.callback {
        if !callback.starts_with("https://") {
            return Err(invalid_params_data(
                "invalid_callback",
                "Callback URL must use HTTPS",
            ));
        }
        request = request.with_callback(callback.clone());
    }

    request = request.with_expires_in(expires_in);
    let expires_at = request.expires_at;

    // Store the payment request for later status checks
    let stored = StoredPaymentRequest::from_request(&request);
    store_payment_request(stored).await?;

    let uri = request.to_uri();

    if log::log_enabled!(log::Level::Debug) {
        debug!(
            "Created payment request {} for {} (expires in {}s)",
            payment_id, params.address, expires_in
        );
    }

    Ok(json!(CreatePaymentRequestResult {
        payment_id,
        uri: uri.clone(),
        qr_data: uri,
        expires_at,
    }))
}

/// Parse a payment URI without executing payment
async fn parse_payment_request<S: Storage>(
    _context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: ParsePaymentRequestParams = parse_params(body)?;

    let request = match PaymentRequest::from_uri(&params.uri) {
        Ok(request) => request,
        Err(err) => return Err(map_payment_parse_error(err)),
    };

    let is_expired = request.is_expired();

    Ok(json!(ParsePaymentRequestResult {
        address: request.address,
        amount: request.amount,
        asset: request.asset.map(|a| a.into_owned()),
        memo: request.memo.map(|m| m.into_owned()),
        payment_id: Some(request.payment_id.into_owned()),
        expires_at: request.expires_at,
        is_expired,
    }))
}

/// Get payment status by scanning the blockchain for matching transactions
///
/// This endpoint scans the blockchain (mempool + blocks) to find payments matching
/// the given payment_id and address. This allows ANY node to verify payment status
/// without requiring local storage synchronization.
///
/// Status values:
/// - pending: No matching transaction found
/// - mempool: Transaction found in mempool (0 confirmations)
/// - confirming: Transaction in block but < STABLE_CONFIRMATIONS (8) confirmations
/// - confirmed: Transaction has >= STABLE_CONFIRMATIONS (8) confirmations (stable)
/// - expired: Payment request has expired (only if `exp` provided)
/// - underpaid: Amount received < expected_amount (only if `expected_amount` provided)
async fn get_payment_status<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetPaymentStatusParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Step 1: Validate payment_id
    if let Err(reason) = validate_payment_id(&params.payment_id) {
        return Err(invalid_params_data(
            "invalid_payment_id",
            &reason.to_string(),
        ));
    }

    // Step 2: Check expiration (if exp provided)
    if params.exp.map(|exp| now > exp).unwrap_or(false) {
        maybe_send_callback(&params.payment_id, PaymentStatus::Expired, None, None, 0).await;
        return Ok(json!(PaymentStatusResponse {
            payment_id: Cow::Owned(params.payment_id),
            status: PaymentStatus::Expired,
            tx_hash: None,
            amount_received: None,
            confirmations: None,
            confirmed_at: None,
        }));
    }

    // Get current blockchain state
    let current_topoheight = blockchain.get_topo_height();
    let target_key = params.address.to_public_key();

    // Step 3: Scan mempool first (0-conf transactions)
    {
        let mempool = blockchain.get_mempool().read().await;
        for (tx_hash, sorted_tx) in mempool.get_txs() {
            let tx = sorted_tx.get_tx();
            if let TransactionType::Transfers(transfers) = tx.get_data() {
                for transfer in transfers {
                    if transfer.get_destination() == &target_key {
                        if let Some(extra_data) = transfer.get_extra_data() {
                            if let Some((found_id, _)) = decode_payment_extra_data(&extra_data.0) {
                                if found_id == params.payment_id {
                                    let amount = transfer.get_amount();
                                    let status = if params
                                        .expected_amount
                                        .map(|exp| amount < exp)
                                        .unwrap_or(false)
                                    {
                                        PaymentStatus::Underpaid
                                    } else {
                                        PaymentStatus::Mempool
                                    };

                                    maybe_send_callback(
                                        &params.payment_id,
                                        status,
                                        Some((**tx_hash).clone()),
                                        Some(amount),
                                        0,
                                    )
                                    .await;

                                    return Ok(json!(PaymentStatusResponse {
                                        payment_id: Cow::Owned(params.payment_id),
                                        status,
                                        tx_hash: Some(Cow::Owned((**tx_hash).clone())),
                                        amount_received: Some(amount),
                                        confirmations: Some(0),
                                        confirmed_at: None,
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 4: Scan blockchain history
    // Determine scan range
    let start_topoheight = params
        .min_topoheight
        .unwrap_or_else(|| current_topoheight.saturating_sub(DEFAULT_SCAN_BLOCKS));

    // Track best match (highest topoheight)
    let mut best_match: Option<(Hash, u64, u64, u64)> = None; // (tx_hash, amount, topo, timestamp)

    let storage = blockchain.get_storage().read().await;

    // Scan blocks from start_topoheight to current
    for topo in start_topoheight..=current_topoheight {
        let block_result = storage.get_block_header_at_topoheight(topo).await;
        let (block_hash, block_header) = match block_result {
            Ok(result) => result,
            Err(_) => continue, // Skip if block not found (pruned)
        };

        // Check each transaction in the block
        for tx_hash in block_header.get_transactions() {
            // Skip unexecuted transactions
            if !storage
                .is_tx_executed_in_block(tx_hash, &block_hash)
                .unwrap_or(false)
            {
                continue;
            }

            let tx = match storage.get_transaction(tx_hash).await {
                Ok(tx) => tx,
                Err(_) => continue,
            };

            if let TransactionType::Transfers(transfers) = tx.get_data() {
                for transfer in transfers {
                    if transfer.get_destination() == &target_key {
                        if let Some(extra_data) = transfer.get_extra_data() {
                            if let Some((found_id, _)) = decode_payment_extra_data(&extra_data.0) {
                                if found_id == params.payment_id {
                                    let amount = transfer.get_amount();
                                    let timestamp = block_header.get_timestamp();

                                    // Keep the highest topoheight match
                                    if best_match
                                        .as_ref()
                                        .map(|(_, _, best_topo, _)| topo > *best_topo)
                                        .unwrap_or(true)
                                    {
                                        best_match =
                                            Some((tx_hash.clone(), amount, topo, timestamp));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 5: Return result based on best match
    if let Some((tx_hash, amount, confirmed_topo, timestamp)) = best_match {
        // Calculate confirmations: current_topoheight - block_topoheight + 1
        let confirmations = if current_topoheight >= confirmed_topo {
            current_topoheight - confirmed_topo + 1
        } else {
            0
        };

        // Determine status
        let status = if params
            .expected_amount
            .map(|exp| amount < exp)
            .unwrap_or(false)
        {
            PaymentStatus::Underpaid
        } else if confirmations >= STABLE_CONFIRMATIONS {
            PaymentStatus::Confirmed
        } else {
            PaymentStatus::Confirming
        };

        let confirmed_at = if status == PaymentStatus::Confirmed {
            Some(timestamp / 1000) // Convert ms to seconds
        } else {
            None
        };

        maybe_send_callback(
            &params.payment_id,
            status,
            Some(tx_hash.clone()),
            Some(amount),
            confirmations,
        )
        .await;

        return Ok(json!(PaymentStatusResponse {
            payment_id: Cow::Owned(params.payment_id),
            status,
            tx_hash: Some(Cow::Owned(tx_hash)),
            amount_received: Some(amount),
            confirmations: Some(confirmations),
            confirmed_at,
        }));
    }

    // Step 6: No match found - return pending
    Ok(json!(PaymentStatusResponse {
        payment_id: Cow::Owned(params.payment_id),
        status: PaymentStatus::Pending,
        tx_hash: None,
        amount_received: None,
        confirmations: None,
        confirmed_at: None,
    }))
}

/// Register a webhook secret for payment callbacks
async fn register_payment_webhook<S: Storage>(
    _context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: RegisterWebhookParams = parse_params(body)?;

    if !params.url.starts_with("https://") {
        return Err(invalid_params_data(
            "invalid_callback",
            "Callback URL must use HTTPS",
        ));
    }

    let secret = hex::decode(&params.secret_hex)
        .map_err(|_| invalid_params_data("invalid_secret", "Webhook secret must be hex"))?;

    if secret.is_empty() {
        return Err(invalid_params_data(
            "invalid_secret",
            "Webhook secret must not be empty",
        ));
    }

    CALLBACK_SERVICE.register_webhook(params.url, secret).await;

    Ok(json!(RegisterWebhookResult { success: true }))
}

/// Unregister a webhook secret
async fn unregister_payment_webhook<S: Storage>(
    _context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: UnregisterWebhookParams = parse_params(body)?;

    CALLBACK_SERVICE.unregister_webhook(&params.url).await;

    Ok(json!(UnregisterWebhookResult { success: true }))
}

fn map_payment_parse_error(err: PaymentParseError) -> InternalRpcError {
    match err {
        PaymentParseError::InvalidPaymentId(reason) => {
            invalid_params_data("invalid_payment_id", &reason.to_string())
        }
        other => InternalRpcError::InvalidParamsAny(other.into()),
    }
}

fn invalid_params_data(code: &str, reason: &str) -> InternalRpcError {
    InternalRpcError::InvalidParamsData {
        message: code.to_string(),
        data: json!({ "code": code, "reason": reason }),
    }
}

/// Watch for incoming payments to an address
/// Returns account balance and recent transaction info
///
/// Note: For full transaction history, use wallet sync or subscribe to events.
/// This endpoint provides a quick balance check for payment verification.
async fn get_address_payments<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAddressPaymentsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    // Validate address
    let address = params.address;
    if !address.is_normal() {
        return Err(InternalRpcError::InvalidParams(
            "Address must be in normal format",
        ));
    }

    let key = address.clone().to_public_key();

    // Get stable topoheight for confirmation info
    let stable_topoheight = blockchain.get_stable_topoheight();
    let current_topoheight = blockchain.get_topo_height();

    // Check if account exists and get balance
    let (balance, last_topoheight) = if storage.has_balance_for(&key, &TOS_ASSET).await? {
        match storage.get_last_balance(&key, &TOS_ASSET).await {
            Ok((topo, versioned_balance)) => {
                // Get the actual balance value from VersionedBalance
                (Some(versioned_balance.get_balance()), Some(topo))
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    // Calculate confirmations if we have balance update info
    let confirmations = last_topoheight.map(|topo| {
        if topo <= stable_topoheight {
            stable_topoheight - topo + 1
        } else {
            current_topoheight - topo
        }
    });

    let status = match confirmations {
        Some(c) if c >= 8 => PaymentStatus::Confirmed,
        Some(c) if c >= 1 => PaymentStatus::Confirming,
        Some(_) => PaymentStatus::Mempool,
        None => PaymentStatus::Pending,
    };

    Ok(json!({
        "address": address,
        "balance": balance,
        "last_topoheight": last_topoheight,
        "stable_topoheight": stable_topoheight,
        "current_topoheight": current_topoheight,
        "confirmations": confirmations,
        "status": status,
    }))
}

/// Generate a unique payment ID
fn generate_payment_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let random: u32 = rand::random();
    format!("pr_{:x}_{:08x}", timestamp, random)
}

/// Maximum number of scheduled executions to return in a single RPC call
const MAX_SCHEDULED_EXECUTIONS: usize = 100;

/// Get contract scheduled executions at a specific topoheight
///
/// Returns scheduled executions that are planned to execute at the given topoheight.
async fn get_contract_scheduled_executions_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractScheduledExecutionsAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.max.is_some_and(|max| max > MAX_SCHEDULED_EXECUTIONS) {
        return Err(InternalRpcError::InvalidParams(
            "Maximum scheduled executions requested cannot be greater than 100",
        ));
    }

    let max = params.max.unwrap_or(MAX_SCHEDULED_EXECUTIONS);

    let storage = blockchain.get_storage().read().await;
    let executions: Vec<ScheduledExecution> = storage
        .get_contract_scheduled_executions_at_topoheight(params.topoheight)
        .await
        .context("Error while retrieving contract scheduled executions")?
        .skip(params.skip.unwrap_or(0))
        .take(max)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!(executions))
}

// Maximum number of contracts to return in a single request
const MAX_CONTRACTS: usize = 100;

// Maximum number of contract data entries to return in a single request
const MAX_CONTRACT_DATA_ENTRIES: usize = 20;

/// Get all deployed contracts with pagination
///
/// Returns a list of contract hashes deployed within the specified topoheight range.
async fn get_contracts<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    let maximum = if let Some(max) = params.maximum {
        if max > MAX_CONTRACTS {
            return Err(InternalRpcError::InvalidParams(
                "Maximum contracts requested cannot be greater than 100",
            ));
        }
        max
    } else {
        MAX_CONTRACTS
    };

    let current_topoheight = blockchain.get_topo_height();

    // Validate minimum_topoheight
    let minimum_topoheight = params.minimum_topoheight.unwrap_or(0);
    if minimum_topoheight > current_topoheight {
        return Err(InternalRpcError::InvalidParams(
            "Minimum topoheight cannot be greater than current topoheight",
        ));
    }

    // Validate maximum_topoheight
    let maximum_topoheight = if let Some(max_topo) = params.maximum_topoheight {
        if max_topo > current_topoheight {
            return Err(InternalRpcError::InvalidParams(
                "Maximum topoheight requested cannot be greater than current topoheight",
            ));
        }
        max_topo
    } else {
        current_topoheight
    };

    // Validate minimum <= maximum
    if minimum_topoheight > maximum_topoheight {
        return Err(InternalRpcError::InvalidParams(
            "Minimum topoheight cannot be greater than maximum topoheight",
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let contracts: Vec<Hash> = storage
        .get_contracts(minimum_topoheight, maximum_topoheight)
        .await
        .context("Error while retrieving contracts")?
        .skip(params.skip.unwrap_or(0))
        .take(maximum)
        .collect::<Result<Vec<_>, _>>()
        .context("Error while collecting contracts")?;

    Ok(json!(contracts))
}

/// Get contract storage data entries with pagination
///
/// Returns all key-value pairs stored in the contract's storage.
async fn get_contract_data_entries<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    use futures::{StreamExt, TryStreamExt};

    let params: GetContractDataEntriesParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let current_topoheight = blockchain.get_topo_height();
    let maximum_topoheight = if let Some(max_topo) = params.maximum_topoheight {
        if max_topo > current_topoheight {
            return Err(InternalRpcError::InvalidParams(
                "Maximum topoheight requested cannot be greater than current topoheight",
            ));
        }
        max_topo
    } else {
        current_topoheight
    };

    // Validate maximum parameter
    let maximum = if let Some(max) = params.maximum {
        if max > MAX_CONTRACT_DATA_ENTRIES {
            return Err(InternalRpcError::InvalidParams(
                "Maximum entries requested cannot be greater than 20",
            ));
        }
        max
    } else {
        MAX_CONTRACT_DATA_ENTRIES
    };

    let stream = storage
        .get_contract_data_entries_at_maximum_topoheight(&params.contract, maximum_topoheight)
        .await
        .context("Error while retrieving contract data entries")?;

    let stream = stream.boxed();
    let entries: Vec<ContractDataEntry> = stream
        .skip(params.skip.unwrap_or(0))
        .take(maximum)
        .map_ok(|(key, value)| ContractDataEntry { key, value })
        .try_collect()
        .await
        .context("Error while collecting contract data entries")?;

    Ok(json!(entries))
}

/// Convert a public key to an address
///
/// Takes a hex-encoded public key and returns the corresponding address.
async fn key_to_address<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: KeyToAddressParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    let key_bytes = hex::decode(&params.key)
        .map_err(|_| InternalRpcError::InvalidJSONRequest)
        .context("Invalid hex encoding for public key")?;

    let pubkey = CompressedPublicKey::from_bytes(&key_bytes)
        .map_err(|_| InternalRpcError::InvalidJSONRequest)
        .context("Invalid public key format")?;

    let address = pubkey.as_address(blockchain.get_network().is_mainnet());

    Ok(json!(address))
}

/// Get lightweight block summary at a specific topoheight
///
/// Returns block metadata without full transaction data - optimized for light clients.
async fn get_block_summary_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlockSummaryAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let hash = storage
        .get_hash_at_topo_height(params.topoheight)
        .await
        .context("Error while retrieving block hash")?;

    let header = storage
        .get_block_header_by_hash(&hash)
        .await
        .context("Error while retrieving block header")?;

    let block_type = get_block_type_for_block(blockchain, &*storage, &hash).await?;
    let difficulty = storage
        .get_difficulty_for_block_hash(&hash)
        .await
        .context("Error while retrieving difficulty")?;
    let cumulative_difficulty = storage
        .get_cumulative_difficulty_for_block_hash(&hash)
        .await
        .context("Error while retrieving cumulative difficulty")?;
    let reward = storage
        .get_block_reward_at_topo_height(params.topoheight)
        .ok();
    let mainnet = blockchain.get_network().is_mainnet();

    // Calculate total block size (header + all transactions)
    let mut total_size_in_bytes = header.size();
    for tx_hash in header.get_txs_hashes() {
        total_size_in_bytes += storage
            .get_transaction_size(tx_hash)
            .await
            .context("Error while retrieving transaction size")?;
    }

    Ok(json!(BlockSummary {
        hash: Cow::Owned(hash.clone()),
        topoheight: Some(params.topoheight),
        height: header.get_height(),
        timestamp: header.get_timestamp(),
        nonce: header.get_nonce(),
        block_type,
        miner: Cow::Owned(header.get_miner().as_address(mainnet)),
        difficulty: Cow::Owned(difficulty),
        cumulative_difficulty: Cow::Owned(cumulative_difficulty),
        txs_count: header.get_transactions().len(),
        total_size_in_bytes,
        reward,
        total_fees: None,
    }))
}

/// Get lightweight block summary by hash
///
/// Returns block metadata without full transaction data - optimized for light clients.
async fn get_block_summary_by_hash<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlockSummaryByHashParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let hash = params.hash;
    let header = storage
        .get_block_header_by_hash(&hash)
        .await
        .context("Error while retrieving block header")?;

    // Get topoheight if block is topologically ordered
    let topoheight = if storage.is_block_topological_ordered(&hash).await? {
        Some(
            storage
                .get_topo_height_for_hash(&hash)
                .await
                .context("Error while retrieving topoheight")?,
        )
    } else {
        None
    };

    let block_type = get_block_type_for_block(blockchain, &*storage, &hash).await?;
    let difficulty = storage
        .get_difficulty_for_block_hash(&hash)
        .await
        .context("Error while retrieving difficulty")?;
    let cumulative_difficulty = storage
        .get_cumulative_difficulty_for_block_hash(&hash)
        .await
        .context("Error while retrieving cumulative difficulty")?;
    let reward = topoheight.and_then(|topo| storage.get_block_reward_at_topo_height(topo).ok());
    let mainnet = blockchain.get_network().is_mainnet();

    // Calculate total block size (header + all transactions)
    let mut total_size_in_bytes = header.size();
    for tx_hash in header.get_txs_hashes() {
        total_size_in_bytes += storage
            .get_transaction_size(tx_hash)
            .await
            .context("Error while retrieving transaction size")?;
    }

    Ok(json!(BlockSummary {
        hash: Cow::Owned(hash.clone()),
        topoheight,
        height: header.get_height(),
        timestamp: header.get_timestamp(),
        nonce: header.get_nonce(),
        block_type,
        miner: Cow::Owned(header.get_miner().as_address(mainnet)),
        difficulty: Cow::Owned(difficulty),
        cumulative_difficulty: Cow::Owned(cumulative_difficulty),
        txs_count: header.get_transactions().len(),
        total_size_in_bytes,
        reward,
        total_fees: None,
    }))
}

// Maximum number of assets to query in batch balance request
const MAX_ASSETS_BATCH: usize = 100;

/// Get balances for multiple assets at a maximum topoheight
///
/// Returns a list of optional versioned balances for each requested asset.
async fn get_balances_at_maximum_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBalancesAtMaximumTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    if params.assets.len() > MAX_ASSETS_BATCH {
        return Err(InternalRpcError::InvalidParams(
            "Maximum assets requested cannot be greater than 100",
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let current_topoheight = blockchain.get_topo_height();

    if params.maximum_topoheight > current_topoheight {
        return Err(InternalRpcError::InvalidParams(
            "Maximum topoheight cannot be greater than current chain topoheight",
        ));
    }

    let mut versions = Vec::with_capacity(params.assets.len());
    for asset in params.assets {
        let balance = storage
            .get_balance_at_maximum_topoheight(
                params.address.get_public_key(),
                &asset,
                params.maximum_topoheight,
            )
            .await
            .context("Error while retrieving balance at maximum topoheight")?
            .map(|(topoheight, version)| RPCVersioned {
                topoheight,
                version,
            });

        versions.push(balance);
    }

    Ok(json!(versions))
}

/// Get block difficulty by hash
///
/// Returns difficulty and estimated hashrate for a specific block.
async fn get_block_difficulty_by_hash<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetBlockDifficultyByHashParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let difficulty = storage
        .get_difficulty_for_block_hash(&params.block_hash)
        .await
        .context("Error while retrieving difficulty for block")?;
    let height = storage
        .get_height_for_block_hash(&params.block_hash)
        .await
        .context("Error while retrieving block height")?;

    let version = get_version_at_height(blockchain.get_network(), height);
    let block_time_target = get_block_time_target_for_version(version);

    let hashrate = difficulty / (block_time_target / MILLIS_PER_SECOND);
    let hashrate_formatted = format_hashrate(hashrate.into());

    Ok(json!(GetDifficultyResult {
        difficulty,
        hashrate,
        hashrate_formatted,
    }))
}

/// Get asset supply at a specific topoheight
///
/// Returns the circulating supply for an asset at the specified topoheight.
async fn get_asset_supply_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAssetSupplyAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;

    let version = storage
        .get_asset_supply_at_maximum_topoheight(&params.asset, params.topoheight)
        .await
        .context("Error while retrieving asset supply")?
        .ok_or_else(|| {
            InternalRpcError::InvalidParams("Supply not found for this asset at topoheight")
        })?;

    Ok(json!(RPCVersioned {
        topoheight: version.0,
        version: version.1,
    }))
}

// Note: get_estimated_fee_per_kb is not implemented in TOS
// TOS uses get_estimated_fee_rates which provides fee rate percentiles from mempool.
// For fee estimation, use get_estimated_fee_rates.

/// Get contract registered executions at a specific topoheight
///
/// Returns registered contract executions that were scheduled at the given topoheight.
async fn get_contract_registered_executions_at_topoheight<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetContractScheduledExecutionsAtTopoHeightParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.max.is_some_and(|max| max > MAX_SCHEDULED_EXECUTIONS) {
        return Err(InternalRpcError::InvalidParams(
            "Maximum executions requested cannot be greater than 100",
        ));
    }

    let max = params.max.unwrap_or(MAX_SCHEDULED_EXECUTIONS);

    let storage = blockchain.get_storage().read().await;
    let executions: Vec<RegisteredExecution> = storage
        .get_registered_contract_scheduled_executions_at_topoheight(params.topoheight)
        .await
        .context("Error while retrieving registered contract executions")?
        .skip(params.skip.unwrap_or(0))
        .take(max)
        .map(|result| {
            result.map(
                |(execution_topoheight, execution_hash)| RegisteredExecution {
                    execution_hash,
                    execution_topoheight,
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .context("Error while collecting registered executions")?;

    Ok(json!(executions))
}

// ============================================================================
// Admin RPC Methods (require --enable-admin-rpc flag)
// WARNING: These are dangerous operations. Only enable for trusted operators.
// SECURITY: These methods are restricted to localhost (loopback) connections only.
// ============================================================================

/// Verify that the request is coming from localhost (loopback address).
/// Admin methods must only be accessible from the local machine for security.
/// SECURITY: Fail-closed policy - reject if client address is unknown or non-loopback.
fn require_localhost(context: &Context) -> Result<(), InternalRpcError> {
    let client_addr: Option<&ClientAddr> = context.get_optional();
    match client_addr {
        Some(addr) if addr.is_loopback() => Ok(()),
        Some(_) => Err(InternalRpcError::InvalidRequestStr(
            "Admin methods are only accessible from localhost",
        )),
        // SECURITY: Fail-closed - if client address is unknown (e.g., reverse proxy,
        // missing peer_addr), reject the request to prevent bypass attacks.
        None => Err(InternalRpcError::InvalidRequestStr(
            "Admin methods require client address verification (localhost only)",
        )),
    }
}

/// Prune the chain to a specific topoheight
///
/// Removes all block data before the specified topoheight.
/// This is a destructive operation and cannot be undone.
/// SECURITY: Only accessible from localhost.
async fn prune_chain<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_localhost(context)?;

    let params: PruneChainParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    let pruned_topoheight = blockchain
        .prune_until_topoheight(params.topoheight)
        .await
        .context("Error while pruning chain")?;

    Ok(json!(PruneChainResult { pruned_topoheight }))
}

/// Rewind the chain by a number of blocks
///
/// Removes the most recent blocks from the chain.
/// All transactions in those blocks will be returned to the mempool.
/// SECURITY: Only accessible from localhost.
async fn rewind_chain<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_localhost(context)?;

    let params: RewindChainParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    let (topoheight, txs) = blockchain
        .rewind_chain(params.count, params.until_stable_height)
        .await
        .context("Error while rewinding chain")?;

    Ok(json!(RewindChainResult {
        topoheight,
        txs: txs.into_iter().map(|(tx_hash, _)| tx_hash).collect(),
    }))
}

/// Clear all caches in storage
///
/// Clears internal caches to free memory.
/// This is a debugging tool and does not affect chain data.
/// SECURITY: Only accessible from localhost.
async fn clear_caches<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_localhost(context)?;

    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let mut storage = blockchain.get_storage().write().await;

    storage
        .clear_caches()
        .await
        .context("Error while clearing caches")?;

    Ok(json!({}))
}

// ============================================================================
// Referral System RPC Handlers
// ============================================================================

/// Check if a user has bound a referrer
async fn has_referrer<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: HasReferrerParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let has_referrer = storage
        .has_referrer(params.address.get_public_key())
        .await
        .context("Error while checking if user has referrer")?;

    Ok(json!(HasReferrerResult { has_referrer }))
}

/// Get the referrer for a user
async fn get_referrer<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetReferrerParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let network = blockchain.get_network();

    if params.address.is_mainnet() != network.is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let referrer_key = storage
        .get_referrer(params.address.get_public_key())
        .await
        .context("Error while retrieving referrer")?;

    let referrer = referrer_key.map(|key| key.to_address(network.is_mainnet()));

    Ok(json!(GetReferrerResult { referrer }))
}

/// Get N levels of uplines for a user
async fn get_uplines<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetUplinesParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let network = blockchain.get_network();

    if params.address.is_mainnet() != network.is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    // Cap levels to MAX_UPLINE_LEVELS (20)
    let levels = params.levels.min(tos_common::referral::MAX_UPLINE_LEVELS);

    let storage = blockchain.get_storage().read().await;
    let result = storage
        .get_uplines(params.address.get_public_key(), levels)
        .await
        .context("Error while retrieving uplines")?;

    let uplines: Vec<Address> = result
        .uplines
        .iter()
        .map(|key| key.to_address(network.is_mainnet()))
        .collect();

    Ok(json!(GetUplinesResult {
        uplines,
        levels_returned: result.levels_returned,
    }))
}

/// Get direct referrals with pagination
async fn get_direct_referrals<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetDirectReferralsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let network = blockchain.get_network();

    if params.address.is_mainnet() != network.is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    // Cap limit to MAX_DIRECT_REFERRALS_PER_PAGE (1000)
    let limit = params
        .limit
        .min(tos_common::referral::MAX_DIRECT_REFERRALS_PER_PAGE);

    let storage = blockchain.get_storage().read().await;
    let result = storage
        .get_direct_referrals(params.address.get_public_key(), params.offset, limit)
        .await
        .context("Error while retrieving direct referrals")?;

    let referrals: Vec<Address> = result
        .referrals
        .iter()
        .map(|key| key.to_address(network.is_mainnet()))
        .collect();

    Ok(json!(GetDirectReferralsResult {
        referrals,
        total_count: result.total_count,
        offset: result.offset,
        has_more: result.has_more,
    }))
}

/// Get the full referral record for a user
async fn get_referral_record<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetReferralRecordParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let network = blockchain.get_network();

    if params.address.is_mainnet() != network.is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let record = storage
        .get_referral_record(params.address.get_public_key())
        .await
        .context("Error while retrieving referral record")?;

    match record {
        Some(rec) => {
            let user = rec.user.to_address(network.is_mainnet());
            let referrer = rec.referrer.map(|r| r.to_address(network.is_mainnet()));

            Ok(json!(GetReferralRecordResult {
                user,
                referrer,
                bound_at_topoheight: rec.bound_at_topoheight,
                bound_tx_hash: rec.bound_tx_hash,
                bound_timestamp: rec.bound_timestamp,
                direct_referrals_count: rec.direct_referrals_count,
                team_size: rec.team_size,
            }))
        }
        None => Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::ReferralRecordNotFound.into(),
        )),
    }
}

/// Get the total team size for a user
async fn get_team_size<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetTeamSizeParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let team_size = storage
        .get_team_size(params.address.get_public_key(), params.use_cache)
        .await
        .context("Error while retrieving team size")?;

    Ok(json!(GetTeamSizeResult {
        team_size,
        from_cache: params.use_cache,
    }))
}

/// Get the level (depth) of a user in the referral tree
async fn get_referral_level<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetReferralLevelParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let level = storage
        .get_level(params.address.get_public_key())
        .await
        .context("Error while retrieving referral level")?;

    Ok(json!(GetReferralLevelResult { level }))
}

// ============================================================================
// KYC System RPC Handlers
// ============================================================================

/// Check if a user has KYC verification
async fn has_kyc<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: HasKycParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let has_kyc = storage
        .has_kyc(params.address.get_public_key())
        .await
        .context("Error while checking if user has KYC")?;

    Ok(json!(HasKycResult { has_kyc }))
}

/// Get KYC data for a user
async fn get_kyc<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: GetKycParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let kyc_data = storage
        .get_kyc(params.address.get_public_key())
        .await
        .context("Error while retrieving KYC data")?;

    let kyc = kyc_data.map(|data| {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let days_until_expiry = {
            let days = data.days_until_expiry(current_time);
            if days == u64::MAX {
                None // No expiration
            } else {
                Some(days)
            }
        };

        KycRpcData {
            level: data.level,
            tier: data.get_tier(),
            status: data.status.as_str().to_string(),
            verified_at: data.verified_at,
            expires_at: data.get_expires_at(),
            days_until_expiry,
            is_valid: data.is_valid(current_time),
        }
    });

    Ok(json!(GetKycResult { kyc }))
}

/// Get effective KYC tier for a user
async fn get_kyc_tier<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetKycTierParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let storage = blockchain.get_storage().read().await;
    let tier = storage
        .get_effective_tier(params.address.get_public_key(), current_time)
        .await
        .context("Error while retrieving KYC tier")?;

    let level = storage
        .get_effective_level(params.address.get_public_key(), current_time)
        .await
        .context("Error while retrieving KYC level")?;

    let is_valid = storage
        .is_kyc_valid(params.address.get_public_key(), current_time)
        .await
        .context("Error while checking KYC validity")?;

    Ok(json!(GetKycTierResult {
        tier,
        level,
        is_valid,
    }))
}

/// Check if a user's KYC is currently valid
async fn is_kyc_valid<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: IsKycValidParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let storage = blockchain.get_storage().read().await;
    let is_valid = storage
        .is_kyc_valid(params.address.get_public_key(), current_time)
        .await
        .context("Error while checking KYC validity")?;

    Ok(json!(IsKycValidResult { is_valid }))
}

/// Check if a user meets a required KYC level
async fn meets_kyc_level<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: MeetsKycLevelParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let storage = blockchain.get_storage().read().await;
    let meets_level = storage
        .meets_kyc_level(
            params.address.get_public_key(),
            params.required_level,
            current_time,
        )
        .await
        .context("Error while checking KYC level")?;

    Ok(json!(MeetsKycLevelResult { meets_level }))
}

/// Get the verifying committee for a user's KYC
async fn get_verifying_committee<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetVerifyingCommitteeParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }

    let storage = blockchain.get_storage().read().await;
    let committee_id = storage
        .get_verifying_committee(params.address.get_public_key())
        .await
        .context("Error while retrieving verifying committee")?;

    Ok(json!(GetVerifyingCommitteeResult { committee_id }))
}

/// Get committee information by ID
async fn get_committee<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetCommitteeParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let network = blockchain.get_network();

    let storage = blockchain.get_storage().read().await;
    let committee_opt = storage
        .get_committee(&params.committee_id)
        .await
        .context("Error while retrieving committee")?;

    let committee = committee_opt.map(|c| convert_committee_to_rpc(&c, network.is_mainnet()));

    Ok(json!(GetCommitteeResult { committee }))
}

/// Get the global committee
async fn get_global_committee<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_no_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let network = blockchain.get_network();

    let storage = blockchain.get_storage().read().await;
    let is_bootstrapped = storage
        .is_global_committee_bootstrapped()
        .await
        .context("Error while checking global committee status")?;

    let committee = if is_bootstrapped {
        storage
            .get_global_committee()
            .await
            .context("Error while retrieving global committee")?
            .map(|c| convert_committee_to_rpc(&c, network.is_mainnet()))
    } else {
        None
    };

    Ok(json!(GetGlobalCommitteeResult {
        committee,
        is_bootstrapped,
    }))
}

/// Convert SecurityCommittee to RPC-friendly format
fn convert_committee_to_rpc(
    committee: &tos_common::kyc::SecurityCommittee,
    is_mainnet: bool,
) -> CommitteeRpc {
    use tos_common::kyc::level_to_tier;

    let members: Vec<CommitteeMemberRpc> = committee
        .members
        .iter()
        .map(|m| CommitteeMemberRpc {
            public_key: m.public_key.to_address(is_mainnet),
            name: m.name.clone(),
            role: m.role.as_str().to_string(),
            status: m.status.as_str().to_string(),
            joined_at: m.joined_at,
            last_active_at: m.last_active_at,
        })
        .collect();

    CommitteeRpc {
        id: committee.id.clone(),
        region: committee.region.as_str().to_string(),
        name: committee.name.clone(),
        members,
        threshold: committee.threshold,
        kyc_threshold: committee.kyc_threshold,
        max_kyc_level: committee.max_kyc_level,
        max_kyc_tier: level_to_tier(committee.max_kyc_level),
        status: committee.status.as_str().to_string(),
        parent_id: committee.parent_id.clone(),
        created_at: committee.created_at,
        updated_at: committee.updated_at,
    }
}
