// Source of gas for scheduled execution tracking
// Tracks whether gas was provided by a contract or an account

use serde::{Deserialize, Serialize};

use crate::{
    crypto::{Hash, PublicKey},
    serializer::*,
};

/// Source of gas funding for scheduled executions.
/// Used to track gas contributions for refund calculations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum Source {
    /// Gas funded by a contract's balance
    Contract(Hash),
    /// Gas funded by a user account
    Account(PublicKey),
}

impl Serializer for Source {
    fn write(&self, writer: &mut Writer) {
        match self {
            Source::Contract(hash) => {
                writer.write_u8(0);
                hash.write(writer);
            }
            Source::Account(account) => {
                writer.write_u8(1);
                account.write(writer);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let tag = reader.read_u8()?;
        match tag {
            0 => {
                let hash = Hash::read(reader)?;
                Ok(Source::Contract(hash))
            }
            1 => {
                let account = PublicKey::read(reader)?;
                Ok(Source::Account(account))
            }
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        match self {
            Source::Contract(hash) => 1 + hash.size(),
            Source::Account(account) => 1 + account.size(),
        }
    }
}
