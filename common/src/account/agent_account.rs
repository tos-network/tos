use crate::{
    crypto::{Hash, PublicKey},
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAccountMeta {
    pub owner: PublicKey,
    pub controller: PublicKey,
    // Off-chain policy reference; consensus only enforces session-key constraints.
    pub policy_hash: Hash,
    pub status: u8,
    pub energy_pool: Option<PublicKey>,
    pub session_key_root: Option<Hash>,
}

impl Serializer for AgentAccountMeta {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            owner: PublicKey::read(reader)?,
            controller: PublicKey::read(reader)?,
            policy_hash: Hash::read(reader)?,
            status: u8::read(reader)?,
            energy_pool: Option::read(reader)?,
            session_key_root: Option::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.owner.write(writer);
        self.controller.write(writer);
        self.policy_hash.write(writer);
        self.status.write(writer);
        self.energy_pool.write(writer);
        self.session_key_root.write(writer);
    }

    fn size(&self) -> usize {
        self.owner.size()
            + self.controller.size()
            + self.policy_hash.size()
            + self.status.size()
            + self.energy_pool.size()
            + self.session_key_root.size()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionKey {
    pub key_id: u64,
    pub public_key: PublicKey,
    pub expiry_topoheight: u64,
    // Per-transaction spending cap for session key enforcement.
    pub max_value_per_window: u64,
    pub allowed_targets: Vec<PublicKey>,
    pub allowed_assets: Vec<Hash>,
}

impl Serializer for SessionKey {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            key_id: u64::read(reader)?,
            public_key: PublicKey::read(reader)?,
            expiry_topoheight: u64::read(reader)?,
            max_value_per_window: u64::read(reader)?,
            allowed_targets: Vec::read(reader)?,
            allowed_assets: Vec::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.key_id.write(writer);
        self.public_key.write(writer);
        self.expiry_topoheight.write(writer);
        self.max_value_per_window.write(writer);
        self.allowed_targets.write(writer);
        self.allowed_assets.write(writer);
    }

    fn size(&self) -> usize {
        self.key_id.size()
            + self.public_key.size()
            + self.expiry_topoheight.size()
            + self.max_value_per_window.size()
            + self.allowed_targets.size()
            + self.allowed_assets.size()
    }
}
