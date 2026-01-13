//! Native Asset Role System
//!
//! Role-based access control for native assets.

use serde::{Deserialize, Serialize};

use crate::crypto::Hash;
use crate::serializer::{Reader, ReaderError, Serializer, Writer};

/// Role identifier (32 bytes for flexibility)
pub type RoleId = [u8; 32];

/// Create a RoleId from a string name (hash of the name)
pub fn role_id_from_name(name: &str) -> RoleId {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(b"NATIVE_ASSET_ROLE:");
    hasher.update(name.as_bytes());
    let result = hasher.finalize();
    let mut id = [0u8; 32];
    id.copy_from_slice(result.as_bytes());
    id
}

// Predefined roles - use hash of role name for consistency

/// Default admin role - can manage all other roles
pub const DEFAULT_ADMIN_ROLE: RoleId = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Minter role - can mint new tokens
pub const MINTER_ROLE: RoleId = [
    0x4d, 0x49, 0x4e, 0x54, 0x45, 0x52, 0x5f, 0x52, // MINTER_R
    0x4f, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00, // OLE
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

/// Burner role - can burn tokens
pub const BURNER_ROLE: RoleId = [
    0x42, 0x55, 0x52, 0x4e, 0x45, 0x52, 0x5f, 0x52, // BURNER_R
    0x4f, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00, // OLE
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
];

/// Pauser role - can pause/unpause transfers
pub const PAUSER_ROLE: RoleId = [
    0x50, 0x41, 0x55, 0x53, 0x45, 0x52, 0x5f, 0x52, // PAUSER_R
    0x4f, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00, // OLE
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03,
];

/// Freezer role - can freeze/unfreeze accounts
pub const FREEZER_ROLE: RoleId = [
    0x46, 0x52, 0x45, 0x45, 0x5a, 0x45, 0x52, 0x5f, // FREEZER_
    0x52, 0x4f, 0x4c, 0x45, 0x00, 0x00, 0x00, 0x00, // ROLE
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
];

/// Role configuration for an asset
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoleConfig {
    /// The admin role that can grant/revoke this role
    pub admin_role: RoleId,
    /// Number of members with this role
    pub member_count: u32,
}

impl Default for RoleConfig {
    fn default() -> Self {
        Self {
            admin_role: DEFAULT_ADMIN_ROLE,
            member_count: 0,
        }
    }
}

impl Serializer for RoleConfig {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.admin_role);
        self.member_count.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let admin_role = reader.read_bytes_32()?;
        let member_count = reader.read()?;
        Ok(Self {
            admin_role,
            member_count,
        })
    }

    fn size(&self) -> usize {
        32 + self.member_count.size()
    }
}

/// Role member entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoleMember {
    /// The asset hash
    pub asset: Hash,
    /// The role ID
    pub role: RoleId,
    /// The account holding the role
    pub account: [u8; 32],
    /// When the role was granted (block height)
    pub granted_at: u64,
}

impl Serializer for RoleMember {
    fn write(&self, writer: &mut Writer) {
        self.asset.write(writer);
        writer.write_bytes(&self.role);
        writer.write_bytes(&self.account);
        self.granted_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let asset = reader.read()?;
        let role = reader.read_bytes_32()?;
        let account = reader.read_bytes_32()?;
        let granted_at = reader.read()?;
        Ok(Self {
            asset,
            role,
            account,
            granted_at,
        })
    }

    fn size(&self) -> usize {
        self.asset.size() + 32 + 32 + self.granted_at.size()
    }
}

/// Check if a role is a predefined role
pub fn is_predefined_role(role: &RoleId) -> bool {
    *role == DEFAULT_ADMIN_ROLE
        || *role == MINTER_ROLE
        || *role == BURNER_ROLE
        || *role == PAUSER_ROLE
        || *role == FREEZER_ROLE
}

/// Get the name of a predefined role
pub fn predefined_role_name(role: &RoleId) -> Option<&'static str> {
    if *role == DEFAULT_ADMIN_ROLE {
        Some("DEFAULT_ADMIN")
    } else if *role == MINTER_ROLE {
        Some("MINTER")
    } else if *role == BURNER_ROLE {
        Some("BURNER")
    } else if *role == PAUSER_ROLE {
        Some("PAUSER")
    } else if *role == FREEZER_ROLE {
        Some("FREEZER")
    } else {
        None
    }
}
