use thiserror::Error;

#[derive(Debug, Error)]
pub enum BalanceError {
    #[error("Balance overflow")]
    Overflow,

    #[error("UNO balance overflow")]
    UnoOverflow,

    #[error("Insufficient balance: need {need}, have {have}")]
    Insufficient { need: u64, have: u64 },

    #[error("Ciphertext decompression failed")]
    Decompression,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Team size calculation limit exceeded")]
    TeamSizeLimitExceeded,

    #[error("Too many events per transaction: {0}")]
    TooManyEvents(usize),

    #[error("Max referral pages exceeded")]
    MaxReferralPagesExceeded,
}
