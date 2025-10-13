use indexmap::IndexSet;
use log::trace;
use tos_common::{
    difficulty::CumulativeDifficulty,
    time::TimestampMillis,
    crypto::Hash,
};
use super::{
    storage::{
        Storage,
        DifficultyProvider,
        GhostdagDataProvider
    },
    error::BlockchainError,
    ghostdag::BlueWorkType,
};

// sort the scores by cumulative difficulty and, if equals, by hash value
pub fn sort_descending_by_cumulative_difficulty<T>(scores: &mut Vec<(T, CumulativeDifficulty)>)
where
    T: AsRef<Hash>,
{
    trace!("sort descending by cumulative difficulty");
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
    trace!("sort ascending by cumulative difficulty");
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

// Sort the TIPS by cumulative difficulty
// If the cumulative difficulty is the same, the hash value is used to sort
// Hashes are sorted in descending order
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
            let mut scores: Vec<(Hash, CumulativeDifficulty)> = Vec::with_capacity(tips_len);
            for hash in tips {
                let cumulative_difficulty = storage.get_cumulative_difficulty_for_block_hash(&hash).await?;
                scores.push((hash, cumulative_difficulty));
            }

            sort_descending_by_cumulative_difficulty(&mut scores);
            Ok(scores.into_iter().map(|(hash, _)| hash).collect())
        }
    }
}

// LEGACY: determine the lowest height possible based on tips and do N+1
// NOTE: This uses legacy height calculation. For GHOSTDAG DAG, use calculate_blue_score_at_tips instead.
// This function is deprecated and should only be used for backward compatibility with chain-based logic.
#[deprecated(note = "Use calculate_blue_score_at_tips for GHOSTDAG DAG support")]
pub async fn calculate_height_at_tips<'a, D, I>(provider: &D, tips: I) -> Result<u64, BlockchainError>
where
    D: DifficultyProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator
{
    trace!("calculate height at tips (LEGACY)");
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

// GHOSTDAG: Calculate the expected blue score for a block with given tips
// This is the GHOSTDAG-aware version of calculate_height_at_tips.
//
// In GHOSTDAG, blue_score is calculated as max(parent.blue_score) + 1,
// where parent is selected based on blue_work (not cumulative difficulty).
//
// For a DAG with multiple parents, this finds the parent with highest blue_score
// and returns blue_score + 1. This matches Kaspa's header processing pipeline.
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
        blue_score += 1;
    }

    Ok(blue_score)
}

// LEGACY: find the best tip based on cumulative difficulty of the blocks
// NOTE: This uses legacy cumulative difficulty. For GHOSTDAG DAG, use find_best_tip_by_blue_work instead.
// This function is deprecated and should only be used for backward compatibility with chain-based logic.
#[deprecated(note = "Use find_best_tip_by_blue_work for GHOSTDAG DAG support")]
pub async fn find_best_tip_by_cumulative_difficulty<'a, D, I>(provider: &D, tips: I) -> Result<&'a Hash, BlockchainError>
where
    D: DifficultyProvider,
    I: Iterator<Item = &'a Hash> + ExactSizeIterator
{
    trace!("find best tip by cumulative difficulty (LEGACY)");
    let tips_len = tips.len();
    match tips_len {
        0 => Err(BlockchainError::ExpectedTips),
        1 => Ok(tips.into_iter().next().unwrap()),
        _ => {
            let mut highest_cumulative_difficulty = CumulativeDifficulty::zero();
            let mut selected_tip = None;
            for hash in tips {
                let cumulative_difficulty = provider.get_cumulative_difficulty_for_block_hash(hash).await?;
                if highest_cumulative_difficulty < cumulative_difficulty {
                    highest_cumulative_difficulty = cumulative_difficulty;
                    selected_tip = Some(hash);
                }
            }

            selected_tip.ok_or(BlockchainError::ExpectedTips)
        }
    }
}

// GHOSTDAG: Find the best tip based on blue work
// This is the GHOSTDAG-aware version of find_best_tip_by_cumulative_difficulty.
//
// In GHOSTDAG, the "heaviest" chain is determined by blue_work (cumulative difficulty of blue blocks)
// rather than cumulative difficulty of all blocks. This implements the core GHOSTDAG selection rule.
//
// For multiple parents, this selects the parent with highest blue_work,
// which becomes the "selected parent" in GHOSTDAG terminology.
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