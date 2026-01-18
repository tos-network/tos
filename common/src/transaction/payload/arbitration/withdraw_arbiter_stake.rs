use serde::{Deserialize, Serialize};

use crate::serializer::{Reader, ReaderError, Serializer, Writer};

/// WithdrawArbiterStakePayload withdraws stake after cooldown.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WithdrawArbiterStakePayload {
    /// Amount to withdraw (0 = withdraw all available).
    pub amount: u64,
}

impl WithdrawArbiterStakePayload {
    pub fn new(amount: u64) -> Self {
        Self { amount }
    }
}

impl Serializer for WithdrawArbiterStakePayload {
    fn write(&self, writer: &mut Writer) {
        self.amount.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            amount: u64::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.amount.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn withdraw_arbiter_stake_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = WithdrawArbiterStakePayload::new(42);
        let data = serde_json::to_vec(&payload)?;
        let decoded: WithdrawArbiterStakePayload = serde_json::from_slice(&data)?;
        assert_eq!(payload.amount, decoded.amount);
        Ok(())
    }
}
