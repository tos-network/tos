use std::{
    cmp::Ordering,
    collections::{hash_map::Entry, BinaryHeap, HashMap, VecDeque},
    sync::Arc,
};
use tos_common::{
    crypto::{Hash, PublicKey},
    transaction::{FeeType, Transaction},
};

// this struct is used to store transaction with its hash and its size in bytes
pub struct TxSelectorEntry<'a> {
    // Hash of the transaction
    pub hash: &'a Arc<Hash>,
    // Current transaction
    pub tx: &'a Arc<Transaction>,
    // Size in bytes of the TX
    pub size: usize,
}

impl PartialEq for TxSelectorEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for TxSelectorEntry<'_> {}

// this struct is used to store transactions in a queue
// and to order them by fees
// Each Transactions is for a specific sender
#[derive(PartialEq, Eq)]
struct Transactions<'a>(VecDeque<TxSelectorEntry<'a>>);

/// Compares two transactions for ordering in the TX selector.
/// TOS-fee transactions have priority over Energy-fee transactions.
/// Within the same fee type, higher fees have priority.
fn compare_tx_priority(a: &Transaction, b: &Transaction) -> Ordering {
    // First compare by fee_type: TOS (0) < Energy (1) in raw value,
    // but we want TOS to have HIGHER priority, so reverse the comparison
    let a_is_energy = *a.get_fee_type() == FeeType::Energy;
    let b_is_energy = *b.get_fee_type() == FeeType::Energy;

    match (a_is_energy, b_is_energy) {
        (false, true) => Ordering::Greater, // TOS > Energy
        (true, false) => Ordering::Less,    // Energy < TOS
        (true, true) => {
            // Both Energy: compare by energy cost (transfer count), not fee field
            // This prevents ordering manipulation by inflating the fee field
            a.calculate_energy_cost().cmp(&b.calculate_energy_cost())
        }
        (false, false) => {
            // Both TOS: compare by fee value
            a.get_fee().cmp(&b.get_fee())
        }
    }
}

impl PartialOrd for Transactions<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self.0.front(), other.0.front()) {
            (Some(a), Some(b)) => Some(compare_tx_priority(a.tx, b.tx)),
            (Some(_), None) => Some(Ordering::Greater),
            (None, Some(_)) => Some(Ordering::Less),
            (None, None) => Some(Ordering::Equal),
        }
    }
}

impl Ord for Transactions<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0.front(), other.0.front()) {
            (Some(a), Some(b)) => compare_tx_priority(a.tx, b.tx),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        }
    }
}

// TX selector is used to select transactions from the mempool
// It create sub groups of transactions by sender and order them by nonces
// It joins all sub groups in a queue that is ordered by fees
pub struct TxSelector<'a> {
    queue: BinaryHeap<Transactions<'a>>,
}

impl<'a> TxSelector<'a> {
    // Create a TxSelector from a list of groups
    pub fn grouped<I>(groups: I) -> Self
    where
        I: Iterator<Item = Vec<TxSelectorEntry<'a>>> + ExactSizeIterator,
    {
        let mut queue = BinaryHeap::with_capacity(groups.len());

        // push every group to the queue
        queue.extend(groups.map(|v| Transactions(VecDeque::from(v))));

        Self { queue }
    }

    // Create a TxSelector with a given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: BinaryHeap::with_capacity(capacity),
        }
    }

    // Create a TxSelector from a list of transactions with their hash and size
    pub fn new<I>(iter: I) -> Self
    where
        I: Iterator<Item = (usize, &'a Arc<Hash>, &'a Arc<Transaction>)>,
    {
        let mut groups: HashMap<&PublicKey, Vec<TxSelectorEntry>> = HashMap::new();

        // Create groups of transactions
        for (size, hash, tx) in iter {
            let entry = TxSelectorEntry { hash, tx, size };

            match groups.entry(tx.get_source()) {
                Entry::Occupied(mut e) => {
                    e.get_mut().push(entry);
                }
                Entry::Vacant(e) => {
                    e.insert(vec![entry]);
                }
            }
        }

        // Order each group by nonces and push it to the queue
        let iter = groups.into_iter().map(|(_, mut v)| {
            v.sort_by(|a, b| a.tx.get_nonce().cmp(&b.tx.get_nonce()));
            v
        });
        Self::grouped(iter)
    }

    // Add a new group
    pub fn push_group<V: Into<VecDeque<TxSelectorEntry<'a>>>>(&mut self, group: V) {
        self.queue.push(Transactions(group.into()));
    }

    // Get the next transaction with the highest fee
    pub fn next(&mut self) -> Option<TxSelectorEntry<'a>> {
        // get the group with the highest fee
        let mut group = self.queue.pop()?;
        // get the entry with the highest fee from this group
        let entry = group.0.pop_front()?;

        // if its not empty, push it back to the queue
        if !group.0.is_empty() {
            self.queue.push(group);
        }

        Some(entry)
    }

    // Check if the selector is empty
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
