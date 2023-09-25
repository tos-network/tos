// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use super::base_types::*;
use std::collections::BTreeMap;

#[derive(Eq, PartialEq, Clone, Hash, Debug)]
pub struct Validators {
    pub voting_rights: BTreeMap<ValidatorName, usize>,
    pub total_votes: usize,
}

impl Validators {
    pub fn new(voting_rights: BTreeMap<ValidatorName, usize>) -> Self {
        let total_votes = voting_rights.iter().fold(0, |sum, (_, votes)| sum + *votes);
        Validators {
            voting_rights,
            total_votes,
        }
    }

    pub fn weight(&self, author: &ValidatorName) -> usize {
        *self.voting_rights.get(author).unwrap_or(&0)
    }

    pub fn quorum_threshold(&self) -> usize {
        // If N = 3f + 1 + k (0 <= k < 3)
        // then (2 N + 3) / 3 = 2f + 1 + (2k + 2)/3 = 2f + 1 + k = N - f
        2 * self.total_votes / 3 + 1
    }

    pub fn validity_threshold(&self) -> usize {
        // If N = 3f + 1 + k (0 <= k < 3)
        // then (N + 2) / 3 = f + 1 + k/3 = f + 1
        (self.total_votes + 2) / 3
    }

    /// Find the highest value than is supported by a quorum of validators.
    pub fn get_strong_majority_lower_bound<V>(&self, mut values: Vec<(ValidatorName, V)>) -> V
    where
        V: Default + std::cmp::Ord,
    {
        values.sort_by(|(_, x), (_, y)| V::cmp(y, x));
        // Browse values by decreasing tx, while tracking how many votes they have.
        let mut score = 0;
        for (name, value) in values {
            score += self.weight(&name);
            if score >= self.quorum_threshold() {
                return value;
            }
        }
        V::default()
    }
}
