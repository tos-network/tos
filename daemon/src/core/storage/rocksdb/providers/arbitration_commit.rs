use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        snapshot::Direction,
        ArbitrationCommitProvider,
    },
};
use async_trait::async_trait;
use tos_common::{
    arbitration::{ArbitrationJurorVoteKey, ArbitrationRequestKey, ArbitrationRoundKey},
    crypto::Hash,
    serializer::Serializer,
    transaction::{
        CommitArbitrationOpenPayload, CommitJurorVotePayload, CommitSelectionCommitmentPayload,
        CommitVoteRequestPayload,
    },
};

#[async_trait]
impl ArbitrationCommitProvider for RocksStorage {
    async fn get_commit_arbitration_open(
        &self,
        key: &ArbitrationRoundKey,
    ) -> Result<Option<CommitArbitrationOpenPayload>, BlockchainError> {
        let key_bytes = key.to_bytes();
        self.load_optional_from_disk(Column::ArbitrationCommitOpenByRound, &key_bytes)
    }

    async fn get_commit_arbitration_open_by_request(
        &self,
        key: &ArbitrationRequestKey,
    ) -> Result<Option<CommitArbitrationOpenPayload>, BlockchainError> {
        let key_bytes = key.to_bytes();
        self.load_optional_from_disk(Column::ArbitrationCommitOpenByRequest, &key_bytes)
    }

    async fn set_commit_arbitration_open(
        &mut self,
        round_key: &ArbitrationRoundKey,
        request_key: &ArbitrationRequestKey,
        payload: &CommitArbitrationOpenPayload,
    ) -> Result<(), BlockchainError> {
        self.insert_into_disk(
            Column::ArbitrationCommitOpenByRound,
            round_key.to_bytes(),
            payload,
        )?;
        self.insert_into_disk(
            Column::ArbitrationCommitOpenByRequest,
            request_key.to_bytes(),
            payload,
        )
    }

    async fn get_commit_vote_request(
        &self,
        key: &ArbitrationRequestKey,
    ) -> Result<Option<CommitVoteRequestPayload>, BlockchainError> {
        let key_bytes = key.to_bytes();
        self.load_optional_from_disk(Column::ArbitrationCommitVoteRequest, &key_bytes)
    }

    async fn set_commit_vote_request(
        &mut self,
        key: &ArbitrationRequestKey,
        payload: &CommitVoteRequestPayload,
    ) -> Result<(), BlockchainError> {
        self.insert_into_disk(
            Column::ArbitrationCommitVoteRequest,
            key.to_bytes(),
            payload,
        )
    }

    async fn get_commit_selection_commitment(
        &self,
        key: &ArbitrationRequestKey,
    ) -> Result<Option<CommitSelectionCommitmentPayload>, BlockchainError> {
        let key_bytes = key.to_bytes();
        self.load_optional_from_disk(Column::ArbitrationCommitSelectionCommitment, &key_bytes)
    }

    async fn set_commit_selection_commitment(
        &mut self,
        key: &ArbitrationRequestKey,
        payload: &CommitSelectionCommitmentPayload,
    ) -> Result<(), BlockchainError> {
        self.insert_into_disk(
            Column::ArbitrationCommitSelectionCommitment,
            key.to_bytes(),
            payload,
        )
    }

    async fn get_commit_juror_vote(
        &self,
        key: &ArbitrationJurorVoteKey,
    ) -> Result<Option<CommitJurorVotePayload>, BlockchainError> {
        let key_bytes = key.to_bytes();
        self.load_optional_from_disk(Column::ArbitrationCommitJurorVote, &key_bytes)
    }

    async fn set_commit_juror_vote(
        &mut self,
        key: &ArbitrationJurorVoteKey,
        payload: &CommitJurorVotePayload,
    ) -> Result<(), BlockchainError> {
        self.insert_into_disk(Column::ArbitrationCommitJurorVote, key.to_bytes(), payload)
    }

    async fn list_commit_juror_votes(
        &self,
        request_id: &Hash,
    ) -> Result<Vec<CommitJurorVotePayload>, BlockchainError> {
        let prefix = request_id.as_bytes().to_vec();
        let iter = self.iter::<ArbitrationJurorVoteKey, CommitJurorVotePayload>(
            Column::ArbitrationCommitJurorVote,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )?;
        let mut out = Vec::new();
        for item in iter {
            let (_key, value) = item?;
            out.push(value);
        }
        Ok(out)
    }

    async fn list_all_arbitration_opens(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<CommitArbitrationOpenPayload>, BlockchainError> {
        let iter = self.iter::<ArbitrationRequestKey, CommitArbitrationOpenPayload>(
            Column::ArbitrationCommitOpenByRequest,
            IteratorMode::Start,
        )?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for item in iter {
            let (_key, value) = item?;
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push(value);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}
