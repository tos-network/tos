use crate::{
    account::SessionKey,
    crypto::{Hash, PublicKey},
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentAccountPayload {
    Register {
        controller: PublicKey,
        policy_hash: Hash,
        session_key_root: Option<Hash>,
    },
    UpdatePolicy {
        policy_hash: Hash,
    },
    RotateController {
        new_controller: PublicKey,
    },
    SetStatus {
        status: u8,
    },
    SetSessionKeyRoot {
        session_key_root: Option<Hash>,
    },
    AddSessionKey {
        key: SessionKey,
    },
    RevokeSessionKey {
        key_id: u64,
    },
}

impl Serializer for AgentAccountPayload {
    fn write(&self, writer: &mut Writer) {
        match self {
            Self::Register {
                controller,
                policy_hash,
                session_key_root,
            } => {
                writer.write_u8(0);
                controller.write(writer);
                policy_hash.write(writer);
                session_key_root.write(writer);
            }
            Self::UpdatePolicy { policy_hash } => {
                writer.write_u8(1);
                policy_hash.write(writer);
            }
            Self::RotateController { new_controller } => {
                writer.write_u8(2);
                new_controller.write(writer);
            }
            Self::SetStatus { status } => {
                writer.write_u8(3);
                status.write(writer);
            }
            Self::SetSessionKeyRoot { session_key_root } => {
                writer.write_u8(5);
                session_key_root.write(writer);
            }
            Self::AddSessionKey { key } => {
                writer.write_u8(6);
                key.write(writer);
            }
            Self::RevokeSessionKey { key_id } => {
                writer.write_u8(7);
                key_id.write(writer);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(match reader.read_u8()? {
            0 => Self::Register {
                controller: PublicKey::read(reader)?,
                policy_hash: Hash::read(reader)?,
                session_key_root: Option::read(reader)?,
            },
            1 => Self::UpdatePolicy {
                policy_hash: Hash::read(reader)?,
            },
            2 => Self::RotateController {
                new_controller: PublicKey::read(reader)?,
            },
            3 => Self::SetStatus {
                status: u8::read(reader)?,
            },
            5 => Self::SetSessionKeyRoot {
                session_key_root: Option::read(reader)?,
            },
            6 => Self::AddSessionKey {
                key: SessionKey::read(reader)?,
            },
            7 => Self::RevokeSessionKey {
                key_id: u64::read(reader)?,
            },
            _ => return Err(ReaderError::InvalidValue),
        })
    }

    fn size(&self) -> usize {
        1 + match self {
            Self::Register {
                controller,
                policy_hash,
                session_key_root,
            } => controller.size() + policy_hash.size() + session_key_root.size(),
            Self::UpdatePolicy { policy_hash } => policy_hash.size(),
            Self::RotateController { new_controller } => new_controller.size(),
            Self::SetStatus { status } => status.size(),
            Self::SetSessionKeyRoot { session_key_root } => session_key_root.size(),
            Self::AddSessionKey { key } => key.size(),
            Self::RevokeSessionKey { key_id } => key_id.size(),
        }
    }
}
