use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    arbitration::{ArbitrationJurorVoteKey, ArbitrationRequestKey, ArbitrationRoundKey},
    crypto::Hash,
    transaction::{
        CommitArbitrationOpenPayload, CommitJurorVotePayload, CommitSelectionCommitmentPayload,
        CommitVoteRequestPayload,
    },
};

#[async_trait]
pub trait ArbitrationCommitProvider: Send + Sync {
    async fn get_commit_arbitration_open(
        &self,
        key: &ArbitrationRoundKey,
    ) -> Result<Option<CommitArbitrationOpenPayload>, BlockchainError>;

    async fn get_commit_arbitration_open_by_request(
        &self,
        key: &ArbitrationRequestKey,
    ) -> Result<Option<CommitArbitrationOpenPayload>, BlockchainError>;

    async fn set_commit_arbitration_open(
        &mut self,
        round_key: &ArbitrationRoundKey,
        request_key: &ArbitrationRequestKey,
        payload: &CommitArbitrationOpenPayload,
    ) -> Result<(), BlockchainError>;

    async fn get_commit_vote_request(
        &self,
        key: &ArbitrationRequestKey,
    ) -> Result<Option<CommitVoteRequestPayload>, BlockchainError>;

    async fn set_commit_vote_request(
        &mut self,
        key: &ArbitrationRequestKey,
        payload: &CommitVoteRequestPayload,
    ) -> Result<(), BlockchainError>;

    async fn get_commit_selection_commitment(
        &self,
        key: &ArbitrationRequestKey,
    ) -> Result<Option<CommitSelectionCommitmentPayload>, BlockchainError>;

    async fn set_commit_selection_commitment(
        &mut self,
        key: &ArbitrationRequestKey,
        payload: &CommitSelectionCommitmentPayload,
    ) -> Result<(), BlockchainError>;

    async fn get_commit_juror_vote(
        &self,
        key: &ArbitrationJurorVoteKey,
    ) -> Result<Option<CommitJurorVotePayload>, BlockchainError>;

    async fn set_commit_juror_vote(
        &mut self,
        key: &ArbitrationJurorVoteKey,
        payload: &CommitJurorVotePayload,
    ) -> Result<(), BlockchainError>;

    async fn list_commit_juror_votes(
        &self,
        request_id: &Hash,
    ) -> Result<Vec<CommitJurorVotePayload>, BlockchainError>;
}
