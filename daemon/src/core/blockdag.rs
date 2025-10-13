use indexmap::IndexSet;
use log::trace;
use tos_common::{
    time::TimestampMillis,
    crypto::Hash,
};
use super::{
    storage::{
        Storage,
        GhostdagDataProvider,
        DifficultyProvider
    },
    error::BlockchainError,
    ghostdag::BlueWorkType,
};


// sort the scores by GHOSTDAG blue work and, if equals, by hash value
pub fn sort_ascending_by_blue_work<T>(scores: &mut Vec<(T, BlueWorkType)>)
where
    T: AsRef<Hash>,
{
    trace!("sort ascending by blue work");
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

// Sort the TIPS by GHOSTDAG blue_work
// If the blue_work is the same, the hash value is used to sort
// Hashes are sorted in descending order (higher blue_work first)
pub async fn sort_tips<S, I>(storage: &S, tips: I) -> Result<IndexSet<Hash>, BlockchainError>
where
    S: Storage,
    I: Iterator<Item = Hash> + ExactSizeIterator,
{
    trace!("sort tips");
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        1 => Ok(tips.into_iter().collect()),
        _ => {
            // Use GHOSTDAG blue_work for sorting
            let mut scores: Vec<(Hash, BlueWorkType)> = Vec::with_capacity(tips_len);
            for hash in tips {
                let blue_work = storage.get_ghostdag_blue_work(&hash).await?;
                scores.push((hash, blue_work));
            }

            // Sort by blue_work (descending - higher blue_work first)
            // If equal, sort by hash value for deterministic ordering
            scores.sort_by(|(a_hash, a), (b_hash, b)| {
                if a != b {
                    b.cmp(a)
                } else {
                    b_hash.cmp(a_hash)
                }
            });

            Ok(scores.into_iter().map(|(hash, _)| hash).collect())
        }
    }
}

// GHOSTDAG: Calculate the expected blue score for a block with given tips
//
// In GHOSTDAG, blue_score is calculated as max(tips.blue_score) + tips.len().
// This is the DAG equivalent of height calculation, but accounts for mergeset size.
//
// For a DAG with multiple parents, this finds the parent with highest blue_score
// and adds the number of tips being merged (the mergeset size).
pub async fn calculate_blue_score_at_tips<'a, G, I>(provider: &G, tips: I) -> Result<u64, BlockchainError>
where
    G: GhostdagDataProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator
{
    trace!("calculate blue score at tips (GHOSTDAG)");
    let mut blue_score = 0;
    let tips_len = tips.len();

    for hash in tips {
        let past_blue_score = provider.get_ghostdag_blue_score(hash).await?;
        if blue_score < past_blue_score {
            blue_score = past_blue_score;
        }
    }

    if tips_len != 0 {
        // GHOSTDAG: blue_score increases by the number of blocks in the mergeset
        // When merging N tips, the mergeset contains all N blocks, so blue_score += N
        blue_score += tips_len as u64;
    }

    Ok(blue_score)
}

// GHOSTDAG: Find the best tip based on blue work
//
// In GHOSTDAG, the "heaviest" chain is determined by blue_work (cumulative difficulty of blue blocks).
// This is the DAG equivalent of selecting the chain with highest cumulative difficulty.
//
// For multiple parents, this selects the parent with highest blue_work,
// which becomes the "selected parent" in GHOSTDAG terminology.
// This implements the core GHOSTDAG chain selection rule.
pub async fn find_best_tip_by_blue_work<'a, G, I>(provider: &G, tips: I) -> Result<&'a Hash, BlockchainError>
where
    G: GhostdagDataProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator
{
    trace!("find best tip by blue work (GHOSTDAG)");
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        1 => Ok(tips.into_iter().next().unwrap()),
        _ => {
            let mut highest_blue_work = BlueWorkType::zero();
            let mut selected_tip = None;
            for hash in tips {
                let blue_work = provider.get_ghostdag_blue_work(hash).await?;
                if highest_blue_work < blue_work {
                    highest_blue_work = blue_work;
                    selected_tip = Some(hash);
                }
            }

            selected_tip.ok_or(BlockchainError::ExpectedTips)
        }
    }
}

// Find the newest tip based on the timestamp of the blocks
pub async fn find_newest_tip_by_timestamp<'a, D, I>(provider: &D, tips: I) -> Result<(&'a Hash, TimestampMillis), BlockchainError>
where
    D: DifficultyProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator
{
    trace!("find newest tip by timestamp");
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        1 => {
            let hash = tips.into_iter().next().unwrap();
            let timestamp = provider.get_timestamp_for_block_hash(hash).await?;
            Ok((hash, timestamp))
        },
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