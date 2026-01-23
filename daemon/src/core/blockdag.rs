use super::{
    error::BlockchainError,
    storage::{DifficultyProvider, Storage},
};
use indexmap::IndexSet;
use itertools::Either;
use log::{debug, trace};
use std::collections::{HashSet, VecDeque};
use tos_common::{
    block::BlockVersion, crypto::Hash, difficulty::CumulativeDifficulty, time::TimestampMillis,
};

use crate::config::get_stable_limit;

// sort the scores by cumulative difficulty and, if equals, by hash value
pub fn sort_descending_by_cumulative_difficulty<T>(scores: &mut Vec<(T, CumulativeDifficulty)>)
where
    T: AsRef<Hash>,
{
    if log::log_enabled!(log::Level::Trace) {
        trace!("sort descending by cumulative difficulty");
    }
    scores.sort_by(|(a_hash, a), (b_hash, b)| {
        if a != b {
            b.cmp(a)
        } else {
            b_hash.as_ref().cmp(a_hash.as_ref())
        }
    });

    if scores.len() >= 2 {
        debug_assert!(scores[0].1 >= scores[1].1);
    }
}

// sort the scores by cumulative difficulty and, if equals, by hash value
pub fn sort_ascending_by_cumulative_difficulty<T>(scores: &mut Vec<(T, CumulativeDifficulty)>)
where
    T: AsRef<Hash>,
{
    if log::log_enabled!(log::Level::Trace) {
        trace!("sort ascending by cumulative difficulty");
    }
    scores.sort_by(|(a_hash, a), (b_hash, b)| {
        if a != b {
            a.cmp(b)
        } else {
            a_hash.as_ref().cmp(b_hash.as_ref())
        }
    });

    if scores.len() >= 2 {
        debug_assert!(scores[0].1 <= scores[1].1);
    }
}

// Sort the TIPS by cumulative difficulty
// If the cumulative difficulty is the same, the hash value is used to sort
// Hashes are sorted in descending order
pub async fn sort_tips<S, I>(
    storage: &S,
    tips: I,
) -> Result<impl Iterator<Item = Hash> + ExactSizeIterator, BlockchainError>
where
    S: Storage,
    I: Iterator<Item = Hash> + ExactSizeIterator,
{
    if log::log_enabled!(log::Level::Trace) {
        trace!("sort tips");
    }
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        1 => Ok(Either::Left(tips)),
        _ => {
            let mut scores: Vec<(Hash, CumulativeDifficulty)> = Vec::with_capacity(tips_len);
            for hash in tips {
                let cumulative_difficulty = storage
                    .get_cumulative_difficulty_for_block_hash(&hash)
                    .await?;
                scores.push((hash, cumulative_difficulty));
            }

            sort_descending_by_cumulative_difficulty(&mut scores);
            Ok(Either::Right(scores.into_iter().map(|(hash, _)| hash)))
        }
    }
}

// determine he lowest height possible based on tips and do N+1
pub async fn calculate_height_at_tips<'a, D, I>(
    provider: &D,
    tips: I,
) -> Result<u64, BlockchainError>
where
    D: DifficultyProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator,
{
    if log::log_enabled!(log::Level::Trace) {
        trace!("calculate height at tips");
    }
    let mut height = 0;
    let tips_len = tips.len();
    for hash in tips {
        let past_height = provider.get_height_for_block_hash(hash).await?;
        if height <= past_height {
            height = past_height;
        }
    }

    if tips_len != 0 {
        height += 1;
    }
    Ok(height)
}

// find the best tip based on cumulative difficulty of the blocks
pub async fn find_best_tip_by_cumulative_difficulty<'a, D, I>(
    provider: &D,
    tips: I,
) -> Result<&'a Hash, BlockchainError>
where
    D: DifficultyProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator,
{
    if log::log_enabled!(log::Level::Trace) {
        trace!("find best tip by cumulative difficulty");
    }
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        // Single tip case: iterator must yield exactly one element
        1 => tips.into_iter().next().ok_or(BlockchainError::ExpectedTips),
        _ => {
            let mut highest_cumulative_difficulty = CumulativeDifficulty::zero();
            let mut selected_tip = None;
            for hash in tips {
                let cumulative_difficulty = provider
                    .get_cumulative_difficulty_for_block_hash(hash)
                    .await?;
                if highest_cumulative_difficulty < cumulative_difficulty {
                    highest_cumulative_difficulty = cumulative_difficulty;
                    selected_tip = Some(hash);
                }
            }

            selected_tip.ok_or(BlockchainError::ExpectedTips)
        }
    }
}

// Find the newest tip based on the timestamp of the blocks
pub async fn find_newest_tip_by_timestamp<'a, D, I>(
    provider: &D,
    tips: I,
) -> Result<(&'a Hash, TimestampMillis), BlockchainError>
where
    D: DifficultyProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator,
{
    if log::log_enabled!(log::Level::Trace) {
        trace!("find newest tip by timestamp");
    }
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        1 => {
            // Single tip case: iterator must yield exactly one element
            let hash = tips
                .into_iter()
                .next()
                .ok_or(BlockchainError::ExpectedTips)?;
            let timestamp = provider.get_timestamp_for_block_hash(hash).await?;
            Ok((hash, timestamp))
        }
        _ => {
            let mut timestamp = 0;
            let mut newest_tip = None;
            for hash in tips.into_iter() {
                let tip_timestamp = provider.get_timestamp_for_block_hash(hash).await?;
                if timestamp < tip_timestamp {
                    timestamp = tip_timestamp;
                    newest_tip = Some(hash);
                }
            }

            Ok((newest_tip.ok_or(BlockchainError::ExpectedTips)?, timestamp))
        }
    }
}

pub async fn build_reachability<P: DifficultyProvider>(
    provider: &P,
    hash: Hash,
    block_version: BlockVersion,
) -> Result<HashSet<Hash>, BlockchainError> {
    let mut set = HashSet::new();
    let mut stack: VecDeque<(Hash, u64)> = VecDeque::new();
    stack.push_back((hash, 0));

    let stable_limit = get_stable_limit(block_version);
    while let Some((current_hash, current_level)) = stack.pop_back() {
        if current_level >= 2 * stable_limit {
            if log::log_enabled!(log::Level::Trace) {
                trace!("Level limit reached, adding {}", current_hash);
            }
            set.insert(current_hash);
        } else {
            if log::log_enabled!(log::Level::Trace) {
                trace!("Level {} reached with hash {}", current_level, current_hash);
            }
            let tips = provider
                .get_past_blocks_for_block_hash(&current_hash)
                .await?;
            set.insert(current_hash);
            for past_hash in tips.iter() {
                if !set.contains(past_hash) {
                    stack.push_back((past_hash.clone(), current_level + 1));
                }
            }
        }
    }

    Ok(set)
}

// this function check that a TIP cannot be refered as past block in another TIP
pub async fn verify_non_reachability<P: DifficultyProvider>(
    provider: &P,
    tips: &IndexSet<Hash>,
    block_version: BlockVersion,
) -> Result<bool, BlockchainError> {
    if log::log_enabled!(log::Level::Trace) {
        trace!("Verifying non reachability for block");
    }
    let tips_count = tips.len();
    let mut reach = Vec::with_capacity(tips_count);
    for hash in tips {
        let set = build_reachability(provider, hash.clone(), block_version).await?;
        reach.push(set);
    }

    for i in 0..tips_count {
        for j in 0..tips_count {
            // if a tip can be referenced as another's past block, its not a tip
            if i != j && reach[j].contains(&tips[i]) {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Tip {} (index {}) is reachable from tip {} (index {})",
                        tips[i], i, tips[j], j
                    );
                }
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "reach: {}",
                        reach[j]
                            .iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<String>>()
                            .join(", ")
                    );
                }
                return Ok(false);
            }
        }
    }
    Ok(true)
}
