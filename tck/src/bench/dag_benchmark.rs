//! DAG ordering benchmark helpers.

use tos_common::crypto::Hash;
use tos_common::difficulty::CumulativeDifficulty;
use tos_common::varuint::VarUint;

/// Build deterministic (hash, difficulty) pairs for ordering benchmarks.
pub fn make_scores(count: usize) -> Vec<(Hash, CumulativeDifficulty)> {
    let mut scores = Vec::with_capacity(count);
    for i in 0..count {
        let mut bytes = [0u8; 32];
        bytes[0] = (i & 0xFF) as u8;
        bytes[1] = ((i >> 8) & 0xFF) as u8;
        let hash = Hash::new(bytes);
        let difficulty = VarUint::from((i as u64 + 1) * 10);
        scores.push((hash, difficulty));
    }
    scores
}
