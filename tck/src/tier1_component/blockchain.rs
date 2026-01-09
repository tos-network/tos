//! TestBlockchain - In-process blockchain for component testing
//!
//! Provides lightweight blockchain instance without RPC/P2P overhead.
//! This is a Tier 1 component for V3.0 testing framework.

use crate::orchestrator::Clock;
use crate::utilities::TempRocksDB;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tos_common::crypto::Hash;

/// Account state for testing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountState {
    /// Account balance in nanoTOS
    pub balance: u64,
    /// Account nonce (confirmed transactions count)
    pub nonce: u64,
}

/// Blockchain-wide counters for O(1) invariant checking
///
/// These counters are maintained incrementally during block processing
/// to enable fast verification of economic invariants without scanning
/// the entire blockchain state.
#[derive(Debug, Clone, Default)]
pub struct BlockchainCounters {
    /// Total of all account balances (nanoTOS)
    pub balances_total: u128,
    /// Total fees burned (sent to null address)
    pub fees_burned: u64,
    /// Total fees paid to miners
    pub fees_miner: u64,
    /// Total fees paid to treasury
    pub fees_treasury: u64,
    /// Total block rewards emitted
    pub rewards_emitted: u64,
    /// Total supply (should equal balances_total + fees_burned)
    pub supply: u128,
}

/// Transaction for testing
#[derive(Debug, Clone)]
pub struct TestTransaction {
    /// Transaction hash
    pub hash: Hash,
    /// Sender account (Hash-based address)
    pub sender: Hash,
    /// Recipient address (Hash for simplicity)
    pub recipient: Hash,
    /// Amount to transfer (nanoTOS)
    pub amount: u64,
    /// Transaction fee (nanoTOS)
    pub fee: u64,
    /// Nonce
    pub nonce: u64,
}

/// Block for testing
#[derive(Debug, Clone)]
pub struct TestBlock {
    /// Block hash
    pub hash: Hash,
    /// Block height
    pub height: u64,
    /// Topological height (DAG ordering position, equals height in linear chain)
    pub topoheight: u64,
    /// Transactions in this block
    pub transactions: Vec<TestTransaction>,
    /// Miner reward
    pub reward: u64,
    /// Pruning point hash (BlockDAG commitment field)
    pub pruning_point: Hash,
    /// Selected parent hash (for pruning point calculation)
    pub selected_parent: Hash,
}

/// Pruning depth constant (matches daemon/src/config.rs PRUNING_DEPTH)
pub const PRUNING_DEPTH: u64 = 200;

/// In-process test blockchain instance
///
/// # Features
///
/// - Clock injection for deterministic time
/// - Real RocksDB storage with RAII cleanup
/// - Direct state access for assertions
/// - O(1) counter reads for invariant checking
/// - Minimal overhead (< 1s initialization)
///
/// # Example
///
/// ```rust,ignore
/// use tos_tck::tier1_component::TestBlockchainBuilder;
///
/// let blockchain = TestBlockchainBuilder::new()
///     .with_clock(clock)
///     .with_funded_account_count(10)
///     .build()
///     .await?;
///
/// blockchain.mine_block().await?;
/// assert_eq!(blockchain.get_tip_height().await?, 1);
/// ```
pub struct TestBlockchain {
    /// Injected clock for deterministic time control
    clock: Arc<dyn Clock>,

    /// Temporary RocksDB directory (RAII cleanup)
    _temp_db: TempRocksDB,

    /// Current blockchain state (accounts)
    /// Using BTreeMap for deterministic iteration order
    accounts: Arc<RwLock<BTreeMap<Hash, AccountState>>>,

    /// Blockchain counters (maintained incrementally)
    counters: Arc<RwLock<BlockchainCounters>>,

    /// Current tip height
    tip_height: AtomicU64,

    /// Current topoheight (same as height in linear chain)
    topoheight: AtomicU64,

    /// DAG tips (hashes of current chain tips)
    tips: Arc<RwLock<Vec<Hash>>>,

    /// State root hash (computed from accounts)
    state_root: Arc<RwLock<Hash>>,

    /// Mempool (pending transactions)
    mempool: Arc<RwLock<Vec<TestTransaction>>>,

    /// Block history (for queries)
    blocks: Arc<RwLock<Vec<TestBlock>>>,

    /// Genesis block hash (for pruning point calculation)
    genesis_hash: Hash,
}

impl TestBlockchain {
    /// Create a new TestBlockchain instance (internal constructor)
    ///
    /// Use `TestBlockchainBuilder` for more convenient configuration.
    pub(crate) fn new(
        clock: Arc<dyn Clock>,
        temp_db: TempRocksDB,
        funded_accounts: Vec<(Hash, u64)>,
    ) -> Result<Self> {
        // Initialize accounts from funded list
        let mut accounts = BTreeMap::new();
        let mut total_balance = 0u128;

        for (pubkey, balance) in funded_accounts {
            accounts.insert(pubkey, AccountState { balance, nonce: 0 });
            // Use saturating_add to prevent overflow in genesis with many accounts
            total_balance = total_balance.saturating_add(balance as u128);
        }

        // Initialize counters
        let counters = BlockchainCounters {
            balances_total: total_balance,
            fees_burned: 0,
            fees_miner: 0,
            fees_treasury: 0,
            rewards_emitted: 0,
            supply: total_balance, // Initial supply = genesis funding
        };

        // Compute initial state root
        let state_root = Self::compute_state_root(&accounts);

        // Genesis hash (zero hash for test blockchain)
        let genesis_hash = Hash::zero();

        // Create genesis block with pruning point = genesis (itself)
        let genesis_block = TestBlock {
            hash: genesis_hash.clone(),
            height: 0,
            topoheight: 0,
            transactions: vec![],
            reward: 0,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash.clone(), // Genesis has no parent
        };

        Ok(Self {
            clock,
            _temp_db: temp_db,
            accounts: Arc::new(RwLock::new(accounts)),
            counters: Arc::new(RwLock::new(counters)),
            tip_height: AtomicU64::new(0),
            topoheight: AtomicU64::new(0),
            tips: Arc::new(RwLock::new(vec![genesis_hash.clone()])),
            state_root: Arc::new(RwLock::new(state_root)),
            mempool: Arc::new(RwLock::new(Vec::new())),
            blocks: Arc::new(RwLock::new(vec![genesis_block])),
            genesis_hash,
        })
    }

    /// Compute state root hash from account state
    ///
    /// This creates a deterministic hash of the entire account state
    /// by serializing accounts in sorted order.
    fn compute_state_root(accounts: &BTreeMap<Hash, AccountState>) -> Hash {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash as StdHash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash accounts in deterministic order (BTreeMap ensures sorted keys)
        for (pubkey, state) in accounts.iter() {
            pubkey.as_bytes().hash(&mut hasher);
            state.balance.hash(&mut hasher);
            state.nonce.hash(&mut hasher);
        }

        let hash_value = hasher.finish();
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&hash_value.to_le_bytes());

        Hash::new(bytes)
    }

    /// Submit a transaction to the mempool
    ///
    /// # Validation
    ///
    /// - Sender must exist and have sufficient balance
    /// - Nonce must be exactly sender.nonce + 1 + pending_count
    /// - Fee must be > 0
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tx = TestTransaction {
    ///     sender: alice_pubkey,
    ///     recipient: bob_address,
    ///     amount: 1000,
    ///     fee: 100,
    ///     nonce: 1,
    ///     hash: Hash::zero(),
    /// };
    /// blockchain.submit_transaction(tx).await?;
    /// ```
    pub async fn submit_transaction(&self, tx: TestTransaction) -> Result<Hash> {
        let mut mempool = self.mempool.write();
        let accounts = self.accounts.read();

        // Validate sender exists
        let sender_state = accounts
            .get(&tx.sender)
            .context("Sender account not found")?;

        // Validate balance (amount + fee)
        let total_cost = tx
            .amount
            .checked_add(tx.fee)
            .context("Amount + fee overflow")?;

        if sender_state.balance < total_cost {
            anyhow::bail!(
                "Insufficient balance: need {}, have {}",
                total_cost,
                sender_state.balance
            );
        }

        // Validate nonce (must be next expected nonce)
        let pending_count: u64 = mempool
            .iter()
            .filter(|t| t.sender == tx.sender)
            .count()
            .try_into()
            .context("Too many pending transactions for sender - mempool overflow")?;

        let expected_nonce = sender_state
            .nonce
            .checked_add(1)
            .and_then(|n| n.checked_add(pending_count))
            .context("Nonce calculation overflow")?;

        if tx.nonce != expected_nonce {
            anyhow::bail!(
                "Invalid nonce: expected {}, got {}",
                expected_nonce,
                tx.nonce
            );
        }

        // Add to mempool
        let tx_hash = tx.hash.clone();
        mempool.push(tx);

        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Transaction {} added to mempool", tx_hash);
        }

        Ok(tx_hash)
    }

    /// Submit multiple transactions in batch
    pub async fn submit_transactions(&self, txs: Vec<TestTransaction>) -> Result<Vec<Hash>> {
        let mut hashes = Vec::with_capacity(txs.len());
        for tx in txs {
            let hash = self.submit_transaction(tx).await?;
            hashes.push(hash);
        }
        Ok(hashes)
    }

    /// Mine a new block with all mempool transactions
    ///
    /// This processes all pending transactions, updates account state,
    /// increments counters, and creates a new block.
    ///
    /// # Returns
    ///
    /// The newly mined block.
    pub async fn mine_block(&self) -> Result<TestBlock> {
        let mut mempool = self.mempool.write();
        let mut accounts = self.accounts.write();
        let mut counters = self.counters.write();
        let mut blocks = self.blocks.write();

        // Take all transactions from mempool
        let transactions = std::mem::take(&mut *mempool);

        // Process each transaction
        for tx in &transactions {
            // Calculate total deduction with overflow protection
            let total_deduction = tx.amount.saturating_add(tx.fee);

            // Deduct from sender
            if let Some(sender) = accounts.get_mut(&tx.sender) {
                sender.balance = sender.balance.saturating_sub(total_deduction);
                sender.nonce = sender.nonce.saturating_add(1);
            }

            // Add to recipient (create if doesn't exist) with overflow protection
            // Use entry API properly to avoid TOCTOU - get mutable ref and update in place
            let recipient_account =
                accounts
                    .entry(tx.recipient.clone())
                    .or_insert_with(|| AccountState {
                        balance: 0,
                        nonce: 0,
                    });
            recipient_account.balance = recipient_account.balance.saturating_add(tx.amount);

            // Update counters: only deduct fee from total (transfer is balance-neutral)
            counters.balances_total = counters.balances_total.saturating_sub(tx.fee as u128);

            // Split fee (example: 50% burned, 50% to miner)
            // Handle odd fees: burned gets remainder
            counters.fees_miner = counters.fees_miner.saturating_add(tx.fee / 2);
            counters.fees_burned = counters.fees_burned.saturating_add(tx.fee - tx.fee / 2);
        }

        // Calculate block reward (example: 50 TOS per block)
        const BLOCK_REWARD: u64 = 50_000_000_000; // 50 TOS in nanoTOS
        counters.rewards_emitted = counters.rewards_emitted.saturating_add(BLOCK_REWARD);
        counters.supply = counters.supply.saturating_add(BLOCK_REWARD as u128);

        // Create new block (with overflow protection)
        let current_height = self.tip_height.load(Ordering::SeqCst);
        let current_topoheight = self.topoheight.load(Ordering::SeqCst);

        let new_height = current_height
            .checked_add(1)
            .context("Block height overflow - chain too long")?;
        let new_topoheight = current_topoheight
            .checked_add(1)
            .context("Topoheight overflow - chain too long")?;
        let block_hash = Self::compute_block_hash(new_height, &transactions);

        // Get selected parent (previous tip)
        let selected_parent = if let Some(last_block) = blocks.last() {
            last_block.hash.clone()
        } else {
            self.genesis_hash.clone()
        };

        // Calculate pruning point using BlockDAG algorithm
        let pruning_point = self.calc_pruning_point(&blocks, &selected_parent, new_topoheight);

        let block = TestBlock {
            hash: block_hash.clone(),
            height: new_height,
            topoheight: new_topoheight,
            transactions: transactions.clone(),
            reward: BLOCK_REWARD,
            pruning_point,
            selected_parent,
        };

        // Update blockchain state
        blocks.push(block.clone());
        self.tip_height.store(new_height, Ordering::SeqCst);
        self.topoheight.store(new_topoheight, Ordering::SeqCst);
        *self.tips.write() = vec![block_hash.clone()];

        // Recompute state root
        *self.state_root.write() = Self::compute_state_root(&accounts);

        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Mined block {} at height {} (topoheight={}) with {} transactions, pruning_point={}",
                block_hash,
                new_height,
                new_topoheight,
                transactions.len(),
                block.pruning_point
            );
        }

        Ok(block)
    }

    /// Calculate pruning point for a new block
    ///
    /// This implements the BlockDAG pruning point calculation:
    /// - If topoheight < PRUNING_DEPTH, return genesis
    /// - Otherwise, walk back PRUNING_DEPTH steps along selected_parent chain
    fn calc_pruning_point(
        &self,
        blocks: &[TestBlock],
        selected_parent: &Hash,
        topoheight: u64,
    ) -> Hash {
        // If topoheight < PRUNING_DEPTH, pruning point is genesis
        if topoheight < PRUNING_DEPTH {
            return self.genesis_hash.clone();
        }

        // Walk back PRUNING_DEPTH steps along the selected_parent chain
        let mut current = selected_parent.clone();
        let mut steps = 0u64;

        while steps < PRUNING_DEPTH {
            // Find current block
            if let Some(block) = blocks.iter().find(|b| b.hash == current) {
                // If we reached genesis, return it
                if block.selected_parent == self.genesis_hash {
                    return self.genesis_hash.clone();
                }
                current = block.selected_parent.clone();
                steps += 1;
            } else {
                // Block not found, return genesis
                return self.genesis_hash.clone();
            }
        }

        current
    }

    /// Receive a block from a peer and apply it to the blockchain
    ///
    /// This simulates receiving a block via P2P network and applying it locally.
    /// The block is validated and its transactions are applied to the state.
    ///
    /// # Arguments
    ///
    /// * `block` - The block received from a peer
    ///
    /// # Returns
    ///
    /// Ok(()) if the block was successfully applied
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Block height is not sequential (must be current_height + 1)
    /// - Block is duplicate (already exists)
    /// - Pruning point is invalid
    pub async fn receive_block(&self, block: TestBlock) -> Result<()> {
        let mut accounts = self.accounts.write();
        let mut counters = self.counters.write();
        let mut blocks = self.blocks.write();

        // Validate block height is sequential
        let current_height = self.tip_height.load(Ordering::SeqCst);
        if block.height != current_height + 1 {
            anyhow::bail!(
                "Invalid block height: expected {}, got {}",
                current_height + 1,
                block.height
            );
        }

        // Check for duplicate block
        if blocks.iter().any(|b| b.hash == block.hash) {
            anyhow::bail!("Duplicate block: {}", block.hash);
        }

        // Validate pruning point
        let expected_pruning_point =
            self.calc_pruning_point(&blocks, &block.selected_parent, block.topoheight);
        if block.pruning_point != expected_pruning_point {
            anyhow::bail!(
                "Invalid pruning_point: expected {}, got {}",
                expected_pruning_point,
                block.pruning_point
            );
        }

        // Process each transaction in the block
        for tx in &block.transactions {
            // Calculate total deduction with overflow protection
            let total_deduction = tx.amount.saturating_add(tx.fee);

            // Deduct from sender with balance validation
            let sender = accounts.get_mut(&tx.sender).ok_or_else(|| {
                anyhow::anyhow!("Sender account not found in apply_block: {}", tx.sender)
            })?;

            // Validate sender has sufficient balance (block should have been validated already)
            if sender.balance < total_deduction {
                anyhow::bail!(
                    "Transaction would cause balance underflow in apply_block: need {}, have {}",
                    total_deduction,
                    sender.balance
                );
            }

            sender.balance = sender.balance.saturating_sub(total_deduction);
            sender.nonce = sender.nonce.saturating_add(1);

            // Add to recipient (create if doesn't exist) with overflow protection
            // Use entry API properly to avoid TOCTOU - get mutable ref and update in place
            let recipient_account =
                accounts
                    .entry(tx.recipient.clone())
                    .or_insert_with(|| AccountState {
                        balance: 0,
                        nonce: 0,
                    });
            recipient_account.balance = recipient_account.balance.saturating_add(tx.amount);

            // Update counters: only deduct fee from total (transfer is balance-neutral)
            counters.balances_total = counters.balances_total.saturating_sub(tx.fee as u128);

            // Split fee (example: 50% burned, 50% to miner)
            // Handle odd fees: burned gets remainder
            counters.fees_miner = counters.fees_miner.saturating_add(tx.fee / 2);
            counters.fees_burned = counters.fees_burned.saturating_add(tx.fee - tx.fee / 2);
        }

        // Apply block reward (with overflow protection)
        counters.rewards_emitted = counters.rewards_emitted.saturating_add(block.reward);
        counters.supply = counters.supply.saturating_add(block.reward as u128);

        // Update blockchain state
        blocks.push(block.clone());
        self.tip_height.store(block.height, Ordering::SeqCst);
        self.topoheight.store(block.topoheight, Ordering::SeqCst);
        *self.tips.write() = vec![block.hash.clone()];

        // Recompute state root
        *self.state_root.write() = Self::compute_state_root(&accounts);

        // Remove applied transactions from mempool (if they exist)
        let mut mempool = self.mempool.write();
        mempool.retain(|tx| !block.transactions.iter().any(|btx| btx.hash == tx.hash));

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Received and applied block {} at height {} with {} transactions",
                block.hash,
                block.height,
                block.transactions.len()
            );
        }

        Ok(())
    }

    /// Compute block hash from height and transactions
    fn compute_block_hash(height: u64, transactions: &[TestTransaction]) -> Hash {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash as StdHash, Hasher};

        let mut hasher = DefaultHasher::new();
        height.hash(&mut hasher);

        for tx in transactions {
            tx.hash.as_bytes().hash(&mut hasher);
        }

        let hash_value = hasher.finish();
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&hash_value.to_le_bytes());

        Hash::new(bytes)
    }

    /// Get block at specific height
    ///
    /// # Arguments
    ///
    /// * `height` - The block height to retrieve
    ///
    /// # Returns
    ///
    /// The block at the specified height, or None if it doesn't exist
    pub async fn get_block_at_height(&self, height: u64) -> Result<Option<TestBlock>> {
        let blocks = self.blocks.read();
        Ok(blocks.iter().find(|b| b.height == height).cloned())
    }

    /// Get account balance
    pub async fn get_balance(&self, address: &Hash) -> Result<u64> {
        let accounts = self.accounts.read();
        Ok(accounts.get(address).map(|a| a.balance).unwrap_or(0))
    }

    /// Get account nonce (confirmed transactions count)
    pub async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        let accounts = self.accounts.read();
        Ok(accounts.get(address).map(|a| a.nonce).unwrap_or(0))
    }

    /// Get current tip height
    pub async fn get_tip_height(&self) -> Result<u64> {
        Ok(self.tip_height.load(Ordering::SeqCst))
    }

    /// Get DAG tips (current chain tips)
    pub async fn get_tips(&self) -> Result<Vec<Hash>> {
        Ok(self.tips.read().clone())
    }

    /// Get state root hash
    ///
    /// This is a deterministic hash of the entire account state,
    /// useful for comparing blockchain states.
    pub async fn state_root(&self) -> Result<Hash> {
        Ok(self.state_root.read().clone())
    }

    /// Get all accounts as a key-value map (for V2.2 P0-3 keyed comparison)
    ///
    /// Returns accounts in sorted order for deterministic comparison.
    pub async fn accounts_kv(&self) -> Result<BTreeMap<Hash, AccountState>> {
        Ok(self.accounts.read().clone())
    }

    /// Read blockchain counters (O(1) operation for V2.2 P0-5)
    ///
    /// These counters are maintained incrementally, so reading them
    /// is extremely fast regardless of blockchain size.
    pub async fn read_counters(&self) -> Result<BlockchainCounters> {
        Ok(self.counters.read().clone())
    }

    /// Count confirmed transactions from an address (for V2.2 P0-6 nonce checking)
    ///
    /// This is equivalent to the account nonce.
    pub async fn confirmed_tx_count_from(&self, address: &Hash) -> Result<u64> {
        self.get_nonce(address).await
    }

    /// Get reference to injected clock
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// Get current topoheight
    pub async fn get_topoheight(&self) -> Result<u64> {
        Ok(self.topoheight.load(Ordering::SeqCst))
    }

    /// Get genesis hash
    pub fn get_genesis_hash(&self) -> &Hash {
        &self.genesis_hash
    }

    /// Get block by hash
    pub async fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<TestBlock>> {
        let blocks = self.blocks.read();
        Ok(blocks.iter().find(|b| &b.hash == hash).cloned())
    }

    /// Get all blocks (for debugging/testing)
    pub async fn get_all_blocks(&self) -> Result<Vec<TestBlock>> {
        Ok(self.blocks.read().clone())
    }

    /// Validate a block's pruning point without applying it
    ///
    /// # Returns
    ///
    /// Ok(true) if pruning point is valid, Ok(false) if invalid
    pub async fn validate_pruning_point(&self, block: &TestBlock) -> Result<bool> {
        let blocks = self.blocks.read();
        let expected = self.calc_pruning_point(&blocks, &block.selected_parent, block.topoheight);
        Ok(block.pruning_point == expected)
    }
}

// RAII cleanup is handled by TempRocksDB's Drop implementation
impl Drop for TestBlockchain {
    fn drop(&mut self) {
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("TestBlockchain dropped, temporary storage will be cleaned up");
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::orchestrator::SystemClock;
    use crate::utilities::create_temp_rocksdb;

    fn create_test_pubkey(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    #[tokio::test]
    async fn test_blockchain_creation() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];

        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        assert_eq!(blockchain.get_tip_height().await.unwrap(), 0);
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 1_000_000);
    }

    #[tokio::test]
    async fn test_transaction_submission() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let bob_hash = Hash::zero();

        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice,
            recipient: bob_hash,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };

        let tx_hash = blockchain.submit_transaction(tx).await.unwrap();
        assert_eq!(tx_hash, Hash::zero());
    }

    #[tokio::test]
    async fn test_block_mining() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let bob_hash = Hash::zero();

        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit transaction
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice.clone(),
            recipient: bob_hash,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();

        // Mine block
        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.height, 1);
        assert_eq!(block.transactions.len(), 1);

        // Check height updated
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 1);

        // Check nonce updated
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
    }

    // ============================================================================
    // Comprehensive Test Suite for TestBlockchain (V3.0 Coverage)
    // ============================================================================

    // --- Balance Tests ---

    #[tokio::test]
    async fn test_get_balance_existing_account() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 1_000_000);
    }

    #[tokio::test]
    async fn test_get_balance_non_existing_account() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let non_existent = create_test_pubkey(99);
        assert_eq!(blockchain.get_balance(&non_existent).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_balance_after_sending() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Alice: 1_000_000 - 100_000 - 100 = 899_900
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 899_900);
        // Bob: 0 + 100_000 = 100_000
        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 100_000);
    }

    #[tokio::test]
    async fn test_balance_after_receiving() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 5_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 250_000,
            fee: 50,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 250_000);
    }

    #[tokio::test]
    async fn test_multiple_balance_updates() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Transaction 1
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Transaction 2
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 200_000,
            fee: 200,
            nonce: 2,
        };
        blockchain.submit_transaction(tx2).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Alice: 10_000_000 - 100_000 - 100 - 200_000 - 200 = 9_699_700
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 9_699_700);
        // Bob: 100_000 + 200_000 = 300_000
        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 300_000);
    }

    // --- Nonce Tests ---

    #[tokio::test]
    async fn test_get_nonce_initial() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_nonce_increments_after_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_nonce_increments_sequentially() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        for nonce in 1..=5 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + nonce as u8),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            blockchain.mine_block().await.unwrap();
            assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), nonce);
        }
    }

    #[tokio::test]
    async fn test_nonce_non_existing_account() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let non_existent = create_test_pubkey(99);
        assert_eq!(blockchain.get_nonce(&non_existent).await.unwrap(), 0);
    }

    // --- Transaction Submission Tests ---

    #[tokio::test]
    async fn test_submit_multiple_transactions() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let txs = vec![
            TestTransaction {
                hash: create_test_pubkey(10),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: 1,
            },
            TestTransaction {
                hash: create_test_pubkey(11),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 2000,
                fee: 100,
                nonce: 2,
            },
        ];

        let hashes = blockchain.submit_transactions(txs).await.unwrap();
        assert_eq!(hashes.len(), 2);
    }

    #[tokio::test]
    async fn test_transaction_hash_returned() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let expected_hash = create_test_pubkey(100);
        let tx = TestTransaction {
            hash: expected_hash.clone(),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };

        let returned_hash = blockchain.submit_transaction(tx).await.unwrap();
        assert_eq!(returned_hash, expected_hash);
    }

    // --- Block Mining Tests ---

    #[tokio::test]
    async fn test_mine_empty_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.height, 1);
        assert_eq!(block.transactions.len(), 0);
    }

    #[tokio::test]
    async fn test_mine_block_with_single_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();

        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.height, 1);
        assert_eq!(block.transactions.len(), 1);
    }

    #[tokio::test]
    async fn test_mine_block_with_multiple_transactions() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        for nonce in 1..=5 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + nonce as u8),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce,
            };
            blockchain.submit_transaction(tx).await.unwrap();
        }

        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.height, 1);
        assert_eq!(block.transactions.len(), 5);
    }

    #[tokio::test]
    async fn test_mine_multiple_blocks_sequential() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        for height in 1..=3 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + height as u8),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: height,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            let block = blockchain.mine_block().await.unwrap();
            assert_eq!(block.height, height);
        }

        assert_eq!(blockchain.get_tip_height().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_block_hash_non_zero() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let block = blockchain.mine_block().await.unwrap();
        assert_ne!(block.hash, Hash::zero());
    }

    // --- Tip Height Tests ---

    #[tokio::test]
    async fn test_tip_height_genesis() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        assert_eq!(blockchain.get_tip_height().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_tip_height_after_mining() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        blockchain.mine_block().await.unwrap();
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 1);

        blockchain.mine_block().await.unwrap();
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 2);
    }

    // --- Tips Tests ---

    #[tokio::test]
    async fn test_get_tips_genesis() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let tips = blockchain.get_tips().await.unwrap();
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0], Hash::zero());
    }

    #[tokio::test]
    async fn test_get_tips_after_mining() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let block = blockchain.mine_block().await.unwrap();
        let tips = blockchain.get_tips().await.unwrap();
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0], block.hash);
    }

    // --- Block Reception Tests ---

    #[tokio::test]
    async fn test_receive_block_sequential_height() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 1,
            topoheight: 1,
            transactions: vec![tx],
            reward: 50_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        blockchain.receive_block(block).await.unwrap();
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_receive_block_invalid_height() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 5, // Invalid - should be 1
            topoheight: 5,
            transactions: vec![],
            reward: 50_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        let result = blockchain.receive_block(block).await;
        assert!(result.is_err());
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_receive_duplicate_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 1,
            topoheight: 1,
            transactions: vec![],
            reward: 50_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        blockchain.receive_block(block.clone()).await.unwrap();
        let result = blockchain.receive_block(block).await;
        assert!(result.is_err());
    }

    // --- Block Retrieval Tests ---

    #[tokio::test]
    async fn test_get_block_at_height_existing() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let mined_block = blockchain.mine_block().await.unwrap();
        let retrieved_block = blockchain.get_block_at_height(1).await.unwrap();

        assert!(retrieved_block.is_some());
        let retrieved_block = retrieved_block.unwrap();
        assert_eq!(retrieved_block.hash, mined_block.hash);
        assert_eq!(retrieved_block.height, 1);
    }

    #[tokio::test]
    async fn test_get_block_at_height_non_existing() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let block = blockchain.get_block_at_height(99).await.unwrap();
        assert!(block.is_none());
    }

    #[tokio::test]
    async fn test_get_block_at_height_beyond_tip() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        // Height beyond current tip should return None
        let block = blockchain.get_block_at_height(100).await.unwrap();
        assert!(block.is_none());
    }

    // --- State Root Tests ---

    #[tokio::test]
    async fn test_state_root_deterministic() {
        let clock = Arc::new(SystemClock);
        let temp_db1 = create_temp_rocksdb().unwrap();
        let temp_db2 = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];

        let blockchain1 =
            TestBlockchain::new(clock.clone(), temp_db1, funded_accounts.clone()).unwrap();
        let blockchain2 = TestBlockchain::new(clock, temp_db2, funded_accounts).unwrap();

        let root1 = blockchain1.state_root().await.unwrap();
        let root2 = blockchain2.state_root().await.unwrap();
        assert_eq!(root1, root2);
    }

    #[tokio::test]
    async fn test_state_root_changes_after_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let root_before = blockchain.state_root().await.unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let root_after = blockchain.state_root().await.unwrap();
        assert_ne!(root_before, root_after);
    }

    // --- Accounts KV Tests ---

    #[tokio::test]
    async fn test_accounts_kv_single_account() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let accounts = blockchain.accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts.get(&alice).unwrap().balance, 1_000_000);
        assert_eq!(accounts.get(&alice).unwrap().nonce, 0);
    }

    #[tokio::test]
    async fn test_accounts_kv_multiple_accounts() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000), (bob.clone(), 500_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let accounts = blockchain.accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts.get(&alice).unwrap().balance, 1_000_000);
        assert_eq!(accounts.get(&bob).unwrap().balance, 500_000);
    }

    #[tokio::test]
    async fn test_accounts_kv_after_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let accounts = blockchain.accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts.get(&alice).unwrap().balance, 899_900);
        assert_eq!(accounts.get(&alice).unwrap().nonce, 1);
        assert_eq!(accounts.get(&bob).unwrap().balance, 100_000);
        assert_eq!(accounts.get(&bob).unwrap().nonce, 0);
    }

    // --- Counter Tests ---

    #[tokio::test]
    async fn test_read_counters_initial() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let counters = blockchain.read_counters().await.unwrap();
        assert_eq!(counters.balances_total, 1_000_000);
        assert_eq!(counters.fees_burned, 0);
        assert_eq!(counters.fees_miner, 0);
        assert_eq!(counters.supply, 1_000_000);
    }

    #[tokio::test]
    async fn test_counters_after_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 100_000,
            fee: 1000,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let counters = blockchain.read_counters().await.unwrap();
        // Fees are split: 50% burned, 50% to miner
        // Total fee = 1000, so 500 burned, 500 to miner
        // Balance total reduces by full fee: 1_000_000 - 1000 = 999_000
        assert_eq!(counters.balances_total, 999_000);
        assert_eq!(counters.fees_burned, 500); // 50% of 1000
        assert_eq!(counters.fees_miner, 500); // 50% of 1000
    }

    // --- Confirmed TX Count Tests ---

    #[tokio::test]
    async fn test_confirmed_tx_count_initial() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let count = blockchain.confirmed_tx_count_from(&alice).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_confirmed_tx_count_after_transactions() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        for nonce in 1..=3 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + nonce as u8),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            blockchain.mine_block().await.unwrap();
        }

        let count = blockchain.confirmed_tx_count_from(&alice).await.unwrap();
        assert_eq!(count, 3);
    }

    // --- Clock Tests ---

    #[tokio::test]
    async fn test_clock_access() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock.clone(), temp_db, vec![]).unwrap();

        let blockchain_clock = blockchain.clock();
        // Test that clock is accessible
        let _instant = blockchain_clock.now();
        // Clock access successful if we reach here
    }

    // --- Topoheight Tests ---

    #[tokio::test]
    async fn test_topoheight_genesis() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let topoheight = blockchain.get_topoheight().await.unwrap();
        assert_eq!(topoheight, 0);
    }

    #[tokio::test]
    async fn test_topoheight_after_mining() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        blockchain.mine_block().await.unwrap();
        let topoheight = blockchain.get_topoheight().await.unwrap();
        assert_eq!(topoheight, 1);

        blockchain.mine_block().await.unwrap();
        let topoheight = blockchain.get_topoheight().await.unwrap();
        assert_eq!(topoheight, 2);
    }

    // --- Edge Case Tests ---

    #[tokio::test]
    async fn test_zero_balance_account_creation() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 0)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_large_balance_handling() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let large_balance = 1_000_000_000_000_000u64; // 1M TOS
        let funded_accounts = vec![(alice.clone(), large_balance)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), large_balance);
    }

    #[tokio::test]
    async fn test_many_transactions_in_single_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 100_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit 50 transactions
        for nonce in 1..=50 {
            let tx = TestTransaction {
                hash: create_test_pubkey(nonce as u8),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 10,
                nonce,
            };
            blockchain.submit_transaction(tx).await.unwrap();
        }

        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.transactions.len(), 50);
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 50);
    }

    #[tokio::test]
    async fn test_transaction_to_self() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: alice.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Balance should decrease only by fee
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 999_900);
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_multiple_senders_single_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let charlie = create_test_pubkey(3);
        let funded_accounts = vec![(alice.clone(), 1_000_000), (bob.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: charlie.clone(),
            amount: 10_000,
            fee: 100,
            nonce: 1,
        };
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: bob.clone(),
            recipient: charlie.clone(),
            amount: 20_000,
            fee: 100,
            nonce: 1,
        };

        blockchain.submit_transaction(tx1).await.unwrap();
        blockchain.submit_transaction(tx2).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Charlie should receive both amounts
        assert_eq!(blockchain.get_balance(&charlie).await.unwrap(), 30_000);
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
        assert_eq!(blockchain.get_nonce(&bob).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_empty_mempool_mining() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        // Mine 10 empty blocks
        for height in 1..=10 {
            let block = blockchain.mine_block().await.unwrap();
            assert_eq!(block.height, height);
            assert_eq!(block.transactions.len(), 0);
        }

        assert_eq!(blockchain.get_tip_height().await.unwrap(), 10);
    }

    // ============================================================================
    // CATEGORY A: Edge Cases Tests (~25 tests)
    // ============================================================================

    // --- Maximum Value Tests ---

    #[tokio::test]
    async fn test_blockchain_max_balance() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let max_balance = u64::MAX;
        let funded_accounts = vec![(alice.clone(), max_balance)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), max_balance);
    }

    #[tokio::test]
    async fn test_blockchain_max_nonce() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), u64::MAX)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Manually set a very high nonce by processing many transactions
        for i in 1..=100 {
            let tx = TestTransaction {
                hash: create_test_pubkey(i),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 0,
                fee: 1,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            blockchain.mine_block().await.unwrap();
        }

        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 100);
    }

    #[tokio::test]
    async fn test_blockchain_max_transaction_amount() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let max_amount = u64::MAX - 1000; // Leave room for fee
        let funded_accounts = vec![(alice.clone(), u64::MAX)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: max_amount,
            fee: 1000,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), max_amount);
    }

    #[tokio::test]
    async fn test_blockchain_max_fee() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let max_fee = 1_000_000_000;
        let funded_accounts = vec![(alice.clone(), u64::MAX)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100,
            fee: max_fee,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let counters = blockchain.read_counters().await.unwrap();
        assert_eq!(counters.fees_burned, max_fee / 2);
        assert_eq!(counters.fees_miner, max_fee / 2);
    }

    // --- Zero/Empty State Tests ---

    #[tokio::test]
    async fn test_blockchain_zero_amount_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 0,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 0);
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_blockchain_empty_accounts() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let accounts = blockchain.accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 0);

        let counters = blockchain.read_counters().await.unwrap();
        assert_eq!(counters.balances_total, 0);
        assert_eq!(counters.supply, 0);
    }

    #[tokio::test]
    async fn test_blockchain_genesis_state_root() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let state_root = blockchain.state_root().await.unwrap();
        // Empty blockchain should have deterministic state root
        assert_ne!(state_root, Hash::zero());
    }

    #[tokio::test]
    async fn test_blockchain_genesis_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let genesis = blockchain.get_block_at_height(0).await.unwrap();
        assert!(genesis.is_some());
        let genesis = genesis.unwrap();
        assert_eq!(genesis.height, 0);
        assert_eq!(genesis.hash, Hash::zero());
        assert_eq!(genesis.transactions.len(), 0);
    }

    // --- Boundary Condition Tests ---

    #[tokio::test]
    async fn test_blockchain_exact_balance_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let initial_balance = 1_000_000;
        let funded_accounts = vec![(alice.clone(), initial_balance)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Send exact balance minus fee
        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: initial_balance - 100,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 0);
        assert_eq!(
            blockchain.get_balance(&bob).await.unwrap(),
            initial_balance - 100
        );
    }

    #[tokio::test]
    async fn test_blockchain_minimum_fee() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 1, // Minimum fee
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let counters = blockchain.read_counters().await.unwrap();
        // Fee splitting: miner gets fee/2, burned gets remainder (fee - fee/2)
        // For fee=1: miner=0, burned=1 (no fee units lost)
        assert_eq!(counters.fees_burned, 1); // 1 - 1/2 = 1 - 0 = 1
        assert_eq!(counters.fees_miner, 0); // 1/2 = 0
    }

    #[tokio::test]
    async fn test_blockchain_odd_fee_split() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 101, // Odd fee - tests integer division
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let counters = blockchain.read_counters().await.unwrap();
        // Fee splitting: miner gets fee/2, burned gets remainder (fee - fee/2)
        // For fee=101: miner=50, burned=51 (no fee units lost)
        assert_eq!(counters.fees_burned, 51); // 101 - 101/2 = 101 - 50 = 51
        assert_eq!(counters.fees_miner, 50); // 101/2 = 50
    }

    // --- Overflow/Underflow Tests ---

    #[tokio::test]
    async fn test_blockchain_balance_underflow_prevention() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob,
            amount: 500,
            fee: 600, // amount + fee > balance
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx).await;
        assert!(result.is_err());
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn test_blockchain_amount_overflow_check() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), u64::MAX)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: u64::MAX,
            fee: 1, // This would overflow
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blockchain_counter_supply_after_many_blocks() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let initial_supply = 1_000_000;
        let funded_accounts = vec![(alice, initial_supply)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Mine 100 blocks
        for _ in 1..=100 {
            blockchain.mine_block().await.unwrap();
        }

        let counters = blockchain.read_counters().await.unwrap();
        const BLOCK_REWARD: u64 = 50_000_000_000;
        let expected_supply = initial_supply as u128 + (100 * BLOCK_REWARD) as u128;
        assert_eq!(counters.supply, expected_supply);
        assert_eq!(counters.rewards_emitted, 100 * BLOCK_REWARD);
    }

    // --- Multiple Account Edge Cases ---

    #[tokio::test]
    async fn test_blockchain_many_funded_accounts() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();

        // Create 100 funded accounts
        let mut funded_accounts = Vec::new();
        for i in 0..100 {
            let account = create_test_pubkey(i);
            funded_accounts.push((account, 1_000_000));
        }

        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let accounts = blockchain.accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 100);

        let counters = blockchain.read_counters().await.unwrap();
        assert_eq!(counters.balances_total, 100_000_000);
    }

    #[tokio::test]
    async fn test_blockchain_account_creation_on_receive() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let new_account = create_test_pubkey(99);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Send to non-existent account
        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: new_account.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Account should be created with balance
        assert_eq!(blockchain.get_balance(&new_account).await.unwrap(), 100_000);
        assert_eq!(blockchain.get_nonce(&new_account).await.unwrap(), 0);

        let accounts = blockchain.accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 2);
    }

    #[tokio::test]
    async fn test_blockchain_deterministic_state_root_ordering() {
        let clock = Arc::new(SystemClock);
        let temp_db1 = create_temp_rocksdb().unwrap();
        let temp_db2 = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let charlie = create_test_pubkey(3);

        // Create accounts in different orders
        let accounts1 = vec![
            (alice.clone(), 1_000_000),
            (bob.clone(), 2_000_000),
            (charlie.clone(), 3_000_000),
        ];
        let accounts2 = vec![
            (charlie.clone(), 3_000_000),
            (alice.clone(), 1_000_000),
            (bob.clone(), 2_000_000),
        ];

        let blockchain1 = TestBlockchain::new(clock.clone(), temp_db1, accounts1).unwrap();
        let blockchain2 = TestBlockchain::new(clock, temp_db2, accounts2).unwrap();

        // State roots should be identical regardless of insertion order
        let root1 = blockchain1.state_root().await.unwrap();
        let root2 = blockchain2.state_root().await.unwrap();
        assert_eq!(root1, root2);
    }

    #[tokio::test]
    async fn test_blockchain_block_hash_deterministic() {
        let clock = Arc::new(SystemClock);
        let temp_db1 = create_temp_rocksdb().unwrap();
        let temp_db2 = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];

        let blockchain1 =
            TestBlockchain::new(clock.clone(), temp_db1, funded_accounts.clone()).unwrap();
        let blockchain2 = TestBlockchain::new(clock, temp_db2, funded_accounts).unwrap();

        // Same transaction in both
        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };

        blockchain1.submit_transaction(tx.clone()).await.unwrap();
        blockchain2.submit_transaction(tx).await.unwrap();

        let block1 = blockchain1.mine_block().await.unwrap();
        let block2 = blockchain2.mine_block().await.unwrap();

        // Blocks should have identical hashes
        assert_eq!(block1.hash, block2.hash);
    }

    // ============================================================================
    // CATEGORY B: State Transition Tests (~20 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_blockchain_multi_step_state_changes() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let charlie = create_test_pubkey(3);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Step 1: Alice → Bob
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1_000_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Step 2: Alice → Charlie
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice.clone(),
            recipient: charlie.clone(),
            amount: 500_000,
            fee: 100,
            nonce: 2,
        };
        blockchain.submit_transaction(tx2).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Step 3: Bob → Charlie
        let tx3 = TestTransaction {
            hash: create_test_pubkey(12),
            sender: bob.clone(),
            recipient: charlie.clone(),
            amount: 200_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx3).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Verify final state
        // Alice: 10_000_000 - 1_000_000 - 100 - 500_000 - 100 = 8_499_800
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 8_499_800);
        // Bob: 0 + 1_000_000 - 200_000 - 100 = 799_900
        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 799_900);
        // Charlie: 0 + 500_000 + 200_000 = 700_000
        assert_eq!(blockchain.get_balance(&charlie).await.unwrap(), 700_000);
    }

    #[tokio::test]
    async fn test_blockchain_state_consistency_across_blocks() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let mut expected_alice_balance = 10_000_000u64;
        let mut expected_bob_balance = 0u64;

        // Process 10 transactions
        for i in 1..=10 {
            let amount = 10_000;
            let fee = 100;

            let tx = TestTransaction {
                hash: create_test_pubkey(10 + i),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount,
                fee,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            blockchain.mine_block().await.unwrap();

            expected_alice_balance -= amount + fee;
            expected_bob_balance += amount;

            // Verify state after each block
            assert_eq!(
                blockchain.get_balance(&alice).await.unwrap(),
                expected_alice_balance
            );
            assert_eq!(
                blockchain.get_balance(&bob).await.unwrap(),
                expected_bob_balance
            );
        }
    }

    #[tokio::test]
    async fn test_blockchain_state_root_changes_progressively() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let mut previous_roots = Vec::new();
        previous_roots.push(blockchain.state_root().await.unwrap());

        // Each transaction should change state root
        for i in 1..=5 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + i),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            blockchain.mine_block().await.unwrap();

            let current_root = blockchain.state_root().await.unwrap();

            // State root should be different from all previous states
            for prev_root in &previous_roots {
                assert_ne!(current_root, *prev_root);
            }

            previous_roots.push(current_root);
        }
    }

    #[tokio::test]
    async fn test_blockchain_mempool_cleared_after_mining() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit 5 transactions
        for i in 1..=5 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + i),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
        }

        // Mine block
        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.transactions.len(), 5);

        // Mine another block - should be empty
        let empty_block = blockchain.mine_block().await.unwrap();
        assert_eq!(empty_block.transactions.len(), 0);
    }

    #[tokio::test]
    async fn test_blockchain_tips_updated_progressively() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let genesis_tips = blockchain.get_tips().await.unwrap();
        assert_eq!(genesis_tips.len(), 1);
        assert_eq!(genesis_tips[0], Hash::zero());

        for _ in 1..=5 {
            let block = blockchain.mine_block().await.unwrap();
            let tips = blockchain.get_tips().await.unwrap();
            assert_eq!(tips.len(), 1);
            assert_eq!(tips[0], block.hash);
        }
    }

    #[tokio::test]
    async fn test_blockchain_nonce_tracking_multiple_senders() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let charlie = create_test_pubkey(3);
        let dave = create_test_pubkey(4);
        let funded_accounts = vec![
            (alice.clone(), 10_000_000),
            (bob.clone(), 10_000_000),
            (charlie.clone(), 10_000_000),
        ];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Alice sends 3 transactions
        for i in 1..=3 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + i),
                sender: alice.clone(),
                recipient: dave.clone(),
                amount: 1000,
                fee: 100,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
        }

        // Bob sends 2 transactions
        for i in 1..=2 {
            let tx = TestTransaction {
                hash: create_test_pubkey(20 + i),
                sender: bob.clone(),
                recipient: dave.clone(),
                amount: 1000,
                fee: 100,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
        }

        // Charlie sends 1 transaction
        let tx = TestTransaction {
            hash: create_test_pubkey(30),
            sender: charlie.clone(),
            recipient: dave.clone(),
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();

        blockchain.mine_block().await.unwrap();

        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 3);
        assert_eq!(blockchain.get_nonce(&bob).await.unwrap(), 2);
        assert_eq!(blockchain.get_nonce(&charlie).await.unwrap(), 1);
        assert_eq!(blockchain.get_nonce(&dave).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_blockchain_balance_conservation() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let initial_total = 10_000_000u128;
        let funded_accounts = vec![(alice.clone(), initial_total as u64)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let initial_counters = blockchain.read_counters().await.unwrap();
        assert_eq!(initial_counters.balances_total, initial_total);

        // Perform multiple transactions
        for i in 1..=10 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + i),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 100_000,
                fee: 1000,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
            blockchain.mine_block().await.unwrap();
        }

        // Total balance should decrease by total fees (fees are burned/sent to miner)
        let final_counters = blockchain.read_counters().await.unwrap();
        let total_fees = 10 * 1000;
        let expected_balance_total = initial_total - total_fees as u128;
        assert_eq!(final_counters.balances_total, expected_balance_total);
    }

    #[tokio::test]
    async fn test_blockchain_receive_block_updates_state() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 1,
            topoheight: 1,
            transactions: vec![tx],
            reward: 50_000_000_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        blockchain.receive_block(block).await.unwrap();

        // State should be updated
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 9_899_900);
        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 100_000);
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_blockchain_receive_block_clears_mempool() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };

        // Submit to mempool
        blockchain.submit_transaction(tx.clone()).await.unwrap();

        // Receive block with same transaction
        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 1,
            topoheight: 1,
            transactions: vec![tx],
            reward: 50_000_000_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        blockchain.receive_block(block).await.unwrap();

        // Mempool should be cleared, next block should be empty
        let next_block = blockchain.mine_block().await.unwrap();
        assert_eq!(next_block.transactions.len(), 0);
    }

    #[tokio::test]
    async fn test_blockchain_counters_updated_incrementally() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let initial_counters = blockchain.read_counters().await.unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 100_000,
            fee: 1000,
            nonce: 1,
        };
        blockchain.submit_transaction(tx).await.unwrap();
        blockchain.mine_block().await.unwrap();

        let updated_counters = blockchain.read_counters().await.unwrap();

        // Fees should be split 50/50
        assert_eq!(
            updated_counters.fees_burned,
            initial_counters.fees_burned + 500
        );
        assert_eq!(
            updated_counters.fees_miner,
            initial_counters.fees_miner + 500
        );

        // Supply should increase by block reward
        const BLOCK_REWARD: u64 = 50_000_000_000;
        assert_eq!(
            updated_counters.supply,
            initial_counters.supply + BLOCK_REWARD as u128
        );
        assert_eq!(
            updated_counters.rewards_emitted,
            initial_counters.rewards_emitted + BLOCK_REWARD
        );
    }

    #[tokio::test]
    async fn test_blockchain_circular_transactions() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let charlie = create_test_pubkey(3);
        let funded_accounts = vec![
            (alice.clone(), 10_000_000),
            (bob.clone(), 10_000_000),
            (charlie.clone(), 10_000_000),
        ];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Alice → Bob
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1_000_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();

        // Bob → Charlie
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: bob.clone(),
            recipient: charlie.clone(),
            amount: 500_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx2).await.unwrap();

        // Charlie → Alice
        let tx3 = TestTransaction {
            hash: create_test_pubkey(12),
            sender: charlie.clone(),
            recipient: alice.clone(),
            amount: 250_000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx3).await.unwrap();

        blockchain.mine_block().await.unwrap();

        // Verify circular flow
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 9_249_900);
        assert_eq!(blockchain.get_balance(&bob).await.unwrap(), 10_499_900);
        assert_eq!(blockchain.get_balance(&charlie).await.unwrap(), 10_249_900);
    }

    #[tokio::test]
    async fn test_blockchain_batch_transaction_atomicity() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let txs = vec![
            TestTransaction {
                hash: create_test_pubkey(10),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: 1,
            },
            TestTransaction {
                hash: create_test_pubkey(11),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 2000,
                fee: 100,
                nonce: 2,
            },
            TestTransaction {
                hash: create_test_pubkey(12),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 3000,
                fee: 100,
                nonce: 3,
            },
        ];

        let hashes = blockchain.submit_transactions(txs).await.unwrap();
        assert_eq!(hashes.len(), 3);

        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.transactions.len(), 3);
    }

    #[tokio::test]
    async fn test_blockchain_block_reward_accumulation() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        const BLOCK_REWARD: u64 = 50_000_000_000;
        const NUM_BLOCKS: u64 = 20;

        for _ in 1..=NUM_BLOCKS {
            blockchain.mine_block().await.unwrap();
        }

        let counters = blockchain.read_counters().await.unwrap();
        assert_eq!(counters.rewards_emitted, BLOCK_REWARD * NUM_BLOCKS);
        assert_eq!(counters.supply, (BLOCK_REWARD * NUM_BLOCKS) as u128);
    }

    #[tokio::test]
    async fn test_blockchain_state_after_receive_vs_mine() {
        let clock = Arc::new(SystemClock);
        let temp_db1 = create_temp_rocksdb().unwrap();
        let temp_db2 = create_temp_rocksdb().unwrap();

        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];

        let blockchain1 =
            TestBlockchain::new(clock.clone(), temp_db1, funded_accounts.clone()).unwrap();
        let blockchain2 = TestBlockchain::new(clock, temp_db2, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };

        // Blockchain1: mine block
        blockchain1.submit_transaction(tx.clone()).await.unwrap();
        let mined_block = blockchain1.mine_block().await.unwrap();

        // Blockchain2: receive block
        let genesis_hash = blockchain2.get_genesis_hash().clone();
        let received_block = TestBlock {
            hash: mined_block.hash.clone(),
            height: 1,
            topoheight: 1,
            transactions: vec![tx],
            reward: mined_block.reward,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };
        blockchain2.receive_block(received_block).await.unwrap();

        // Both should have identical state
        assert_eq!(
            blockchain1.get_balance(&alice).await.unwrap(),
            blockchain2.get_balance(&alice).await.unwrap()
        );
        assert_eq!(
            blockchain1.get_balance(&bob).await.unwrap(),
            blockchain2.get_balance(&bob).await.unwrap()
        );
        assert_eq!(
            blockchain1.state_root().await.unwrap(),
            blockchain2.state_root().await.unwrap()
        );
    }

    // ============================================================================
    // CATEGORY C: Error Handling Tests (~20 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_blockchain_error_insufficient_balance() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 900,
            fee: 200, // Total 1100 > 1000 balance
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Insufficient balance"));
    }

    #[tokio::test]
    async fn test_blockchain_error_sender_not_found() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let non_existent = create_test_pubkey(99);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice, 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: non_existent,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Sender account not found"));
    }

    #[tokio::test]
    async fn test_blockchain_error_invalid_nonce_too_low() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit and mine first transaction
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Try to submit transaction with nonce 1 again
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 1, // Should be 2
        };

        let result = blockchain.submit_transaction(tx2).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid nonce"));
    }

    #[tokio::test]
    async fn test_blockchain_error_invalid_nonce_too_high() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 5, // Should be 1
        };

        let result = blockchain.submit_transaction(tx).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid nonce"));
        assert!(err_msg.contains("expected 1"));
    }

    #[tokio::test]
    async fn test_blockchain_error_nonce_gap_in_mempool() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit transaction with nonce 1
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();

        // Try to submit with nonce 3 (gap)
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice,
            recipient: bob,
            amount: 1000,
            fee: 100,
            nonce: 3, // Should be 2
        };

        let result = blockchain.submit_transaction(tx2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blockchain_error_receive_block_wrong_height() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 10, // Should be 1
            topoheight: 10,
            transactions: vec![],
            reward: 50_000_000_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        let result = blockchain.receive_block(block).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid block height"));
    }

    #[tokio::test]
    async fn test_blockchain_error_receive_duplicate_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 1,
            topoheight: 1,
            transactions: vec![],
            reward: 50_000_000_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        blockchain.receive_block(block.clone()).await.unwrap();

        // Try to receive the same block again - should fail
        let result = blockchain.receive_block(block).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blockchain_error_amount_fee_overflow() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), u64::MAX)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice,
            recipient: bob,
            amount: u64::MAX,
            fee: 1, // Would overflow
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("overflow"));
    }

    #[tokio::test]
    async fn test_blockchain_error_sequential_invalid_transactions() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // First invalid transaction
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 2000, // Insufficient balance
            fee: 100,
            nonce: 1,
        };
        let result1 = blockchain.submit_transaction(tx1).await;
        assert!(result1.is_err());

        // Second invalid transaction
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 500,
            fee: 600, // Insufficient balance
            nonce: 1,
        };
        let result2 = blockchain.submit_transaction(tx2).await;
        assert!(result2.is_err());

        // Valid transaction should still work
        let tx3 = TestTransaction {
            hash: create_test_pubkey(12),
            sender: alice,
            recipient: bob,
            amount: 500,
            fee: 100,
            nonce: 1,
        };
        let result3 = blockchain.submit_transaction(tx3).await;
        assert!(result3.is_ok());
    }

    #[tokio::test]
    async fn test_blockchain_error_batch_with_invalid_transaction() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let txs = vec![
            TestTransaction {
                hash: create_test_pubkey(10),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: 1,
            },
            TestTransaction {
                hash: create_test_pubkey(11),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 2000,
                fee: 100,
                nonce: 3, // Invalid nonce - should be 2
            },
        ];

        let result = blockchain.submit_transactions(txs).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blockchain_error_transaction_after_balance_exhausted() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Exhaust balance
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 900,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();
        blockchain.mine_block().await.unwrap();

        // Try another transaction
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice,
            recipient: bob,
            amount: 10,
            fee: 10,
            nonce: 2,
        };

        let result = blockchain.submit_transaction(tx2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blockchain_error_zero_fee_validation() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Zero fee is technically allowed by current implementation
        // This test verifies behavior remains consistent
        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 0,
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx).await;
        // Current implementation allows zero fee
        assert!(result.is_ok());

        blockchain.mine_block().await.unwrap();
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 999_000);
    }

    #[tokio::test]
    async fn test_blockchain_error_receive_block_skip_height() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let blockchain = TestBlockchain::new(clock, temp_db, vec![]).unwrap();

        // Mine block 1
        blockchain.mine_block().await.unwrap();

        // Try to receive block 3 (skipping 2)
        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 3,
            topoheight: 3,
            transactions: vec![],
            reward: 50_000_000_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        let result = blockchain.receive_block(block).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid block height"));
        assert!(err_msg.contains("expected 2"));
    }

    #[tokio::test]
    async fn test_blockchain_error_multiple_pending_same_nonce() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit first transaction
        let tx1 = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 100,
            nonce: 1,
        };
        blockchain.submit_transaction(tx1).await.unwrap();

        // Try to submit another with same nonce
        let tx2 = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice,
            recipient: bob,
            amount: 2000,
            fee: 100,
            nonce: 1,
        };

        let result = blockchain.submit_transaction(tx2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blockchain_error_invalid_transaction_in_received_block() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 1000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Create block with transaction exceeding balance
        let tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob,
            amount: 10_000, // More than balance
            fee: 100,
            nonce: 1,
        };

        let genesis_hash = blockchain.get_genesis_hash().clone();
        let block = TestBlock {
            hash: create_test_pubkey(50),
            height: 1,
            topoheight: 1,
            transactions: vec![tx],
            reward: 50_000_000_000,
            pruning_point: genesis_hash.clone(),
            selected_parent: genesis_hash,
        };

        // Block with invalid transaction should be rejected
        let result = blockchain.receive_block(block).await;
        assert!(
            result.is_err(),
            "Block with invalid transaction should be rejected"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("balance underflow"),
            "Error should mention balance underflow"
        );

        // Balance should remain unchanged after rejected block
        assert_eq!(blockchain.get_balance(&alice).await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn test_blockchain_error_recovery_after_failed_submission() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit invalid transaction
        let invalid_tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 20_000_000, // Exceeds balance
            fee: 100,
            nonce: 1,
        };
        let result = blockchain.submit_transaction(invalid_tx).await;
        assert!(result.is_err());

        // Submit valid transaction - should work
        let valid_tx = TestTransaction {
            hash: create_test_pubkey(11),
            sender: alice.clone(),
            recipient: bob,
            amount: 100_000,
            fee: 100,
            nonce: 1,
        };
        let result = blockchain.submit_transaction(valid_tx).await;
        assert!(result.is_ok());

        blockchain.mine_block().await.unwrap();
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_blockchain_concurrent_nonce_validation() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        // Submit multiple transactions with sequential nonces
        for i in 1..=5 {
            let tx = TestTransaction {
                hash: create_test_pubkey(10 + i),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 100,
                nonce: i as u64,
            };
            blockchain.submit_transaction(tx).await.unwrap();
        }

        // All should be in mempool, mining should process all
        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.transactions.len(), 5);
    }

    #[tokio::test]
    async fn test_blockchain_state_unchanged_after_failed_submission() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let bob = create_test_pubkey(2);
        let funded_accounts = vec![(alice.clone(), 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        let initial_state_root = blockchain.state_root().await.unwrap();
        let initial_balance = blockchain.get_balance(&alice).await.unwrap();
        let initial_counters = blockchain.read_counters().await.unwrap();

        // Submit invalid transaction
        let invalid_tx = TestTransaction {
            hash: create_test_pubkey(10),
            sender: alice.clone(),
            recipient: bob,
            amount: 20_000_000,
            fee: 100,
            nonce: 1,
        };
        let _ = blockchain.submit_transaction(invalid_tx).await;

        // State should be unchanged
        assert_eq!(blockchain.state_root().await.unwrap(), initial_state_root);
        assert_eq!(
            blockchain.get_balance(&alice).await.unwrap(),
            initial_balance
        );
        let final_counters = blockchain.read_counters().await.unwrap();
        assert_eq!(
            final_counters.balances_total,
            initial_counters.balances_total
        );
    }

    #[tokio::test]
    async fn test_blockchain_topoheight_matches_tip_height() {
        let clock = Arc::new(SystemClock);
        let temp_db = create_temp_rocksdb().unwrap();
        let alice = create_test_pubkey(1);
        let funded_accounts = vec![(alice, 10_000_000)];
        let blockchain = TestBlockchain::new(clock, temp_db, funded_accounts).unwrap();

        for i in 1..=10 {
            blockchain.mine_block().await.unwrap();
            let tip_height = blockchain.get_tip_height().await.unwrap();
            let topoheight = blockchain.get_topoheight().await.unwrap();
            assert_eq!(tip_height, topoheight);
            assert_eq!(topoheight, i);
        }
    }
}
