use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tos_common::{rpc::RpcRequest, serializer::*, tokio::sync::Mutex};

// Custom serde module for [u8; 32] as hex string
mod hex_bytes32 {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| D::Error::custom("invalid length for 32-byte array"))
    }
}

// Custom serde module for [u8; 64] as hex string
mod hex_bytes64 {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| D::Error::custom("invalid length for 64-byte array"))
    }
}

// Used for context only
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct XSWDAppId(pub Arc<String>);

// Application state shared between all threads
// Built from the application data
pub struct AppState {
    // Application ID in hexadecimal format
    id: XSWDAppId,
    // Name of the app
    name: String,
    // Small description of the app
    description: String,
    // URL of the app if exists
    url: Option<String>,
    // All permissions for each method based on user config
    permissions: Mutex<IndexMap<String, Permission>>,
    // Do we have a pending request?
    is_requesting: AtomicBool,
}

impl Hash for AppState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.0.hash(state);
    }
}

impl PartialEq for AppState {
    fn eq(&self, other: &Self) -> bool {
        self.id.0.eq(&other.id.0)
    }
}

impl Eq for AppState {}

pub type AppStateShared = Arc<AppState>;

impl AppState {
    pub fn new(data: ApplicationData) -> Self {
        Self {
            id: XSWDAppId(Arc::new(data.id)),
            name: data.name,
            description: data.description,
            url: data.url,
            permissions: Mutex::new(
                data.permissions
                    .into_iter()
                    .map(|k| (k, Permission::Ask))
                    .collect(),
            ),
            is_requesting: AtomicBool::new(false),
        }
    }

    pub fn with_permissions(
        data: ApplicationData,
        permissions: IndexMap<String, Permission>,
    ) -> Self {
        Self {
            id: XSWDAppId(Arc::new(data.id)),
            name: data.name,
            description: data.description,
            url: data.url,
            permissions: Mutex::new(permissions),
            is_requesting: AtomicBool::new(false),
        }
    }

    pub fn id(&self) -> XSWDAppId {
        self.id.clone()
    }

    pub fn get_id(&self) -> &str {
        &self.id.0
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_description(&self) -> &String {
        &self.description
    }

    pub fn get_url(&self) -> &Option<String> {
        &self.url
    }

    pub fn get_permissions(&self) -> &Mutex<IndexMap<String, Permission>> {
        &self.permissions
    }

    pub fn is_requesting(&self) -> bool {
        self.is_requesting.load(Ordering::SeqCst)
    }

    pub fn set_requesting(&self, value: bool) {
        self.is_requesting.store(value, Ordering::SeqCst);
    }
}

// XSWD v2.0: Application data with Ed25519 signature verification
// This struct contains all information about an application requesting wallet access
// Security: Permissions are now cryptographically bound to the application's public key
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ApplicationData {
    // Application ID in hexadecimal format (64 chars = 32 bytes)
    pub(super) id: String,
    // Name of the app (max 32 chars)
    pub(super) name: String,
    // Small description of the app (max 255 chars)
    pub(super) description: String,
    // URL of the app if exists (max 255 chars, must start with http:// or https://)
    pub(super) url: Option<String>,
    // Permissions per RPC method
    // This is useful to request in one time all permissions
    #[serde(default)]
    pub(super) permissions: IndexSet<String>,

    // XSWD v2.0: Ed25519 signature verification fields
    // Application's Ed25519 public key (32 bytes)
    // This binds permissions to the application's cryptographic identity
    #[serde(with = "hex_bytes32")]
    pub public_key: [u8; 32],

    // Unix timestamp when this ApplicationData was created (seconds since epoch)
    // Used to prevent replay attacks (must be within 5 minutes of current time)
    pub timestamp: u64,

    // Random nonce to prevent replay attacks within the valid time window
    // Each application registration must use a unique nonce
    pub nonce: u64,

    // Ed25519 signature over all fields above
    // Signature = Ed25519Sign(private_key, serialize_for_signing(id || name || description || url || permissions || public_key || timestamp || nonce))
    // This proves the application controls the private key corresponding to public_key
    #[serde(with = "hex_bytes64")]
    pub signature: [u8; 64],
}

impl ApplicationData {
    pub fn get_id(&self) -> &String {
        &self.id
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_description(&self) -> &String {
        &self.description
    }

    pub fn get_url(&self) -> &Option<String> {
        &self.url
    }

    pub fn get_permissions(&self) -> &IndexSet<String> {
        &self.permissions
    }

    pub fn get_public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    pub fn get_timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn get_nonce(&self) -> u64 {
        self.nonce
    }

    pub fn get_signature(&self) -> &[u8; 64] {
        &self.signature
    }

    // XSWD v2.0: Serialize ApplicationData for Ed25519 signature verification
    // This function produces deterministic byte representation of all fields (except signature)
    // The signature is computed over the output of this function
    //
    // Format: id || name || description || url_present || url || permissions_len || permissions || public_key || timestamp || nonce
    // All strings are encoded as UTF-8, numbers as little-endian bytes
    pub fn serialize_for_signing(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Field 1: id (String)
        buf.extend_from_slice(self.id.as_bytes());

        // Field 2: name (String)
        buf.extend_from_slice(self.name.as_bytes());

        // Field 3: description (String)
        buf.extend_from_slice(self.description.as_bytes());

        // Field 4: url (Option<String>)
        if let Some(url) = &self.url {
            buf.push(1); // URL present
            buf.extend_from_slice(url.as_bytes());
        } else {
            buf.push(0); // URL not present
        }

        // Field 5: permissions (IndexSet<String>)
        // Format: count (u16) followed by each permission string
        buf.extend_from_slice(&(self.permissions.len() as u16).to_le_bytes());
        for perm in &self.permissions {
            buf.extend_from_slice(perm.as_bytes());
            buf.push(0); // Null terminator for each permission
        }

        // Field 6: public_key ([u8; 32])
        buf.extend_from_slice(&self.public_key);

        // Field 7: timestamp (u64)
        buf.extend_from_slice(&self.timestamp.to_le_bytes());

        // Field 8: nonce (u64)
        buf.extend_from_slice(&self.nonce.to_le_bytes());

        buf
    }
}

impl Serializer for ApplicationData {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = reader.read_string()?;
        let name = reader.read_string()?;
        let description = reader.read_string()?;
        let url = Option::read(reader)?;
        let permissions = IndexSet::read(reader)?;

        // XSWD v2.0: Read Ed25519 signature fields
        let public_key = reader.read_bytes(32)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let signature = reader.read_bytes(64)?;

        Ok(Self {
            id,
            name,
            description,
            url,
            permissions,
            public_key,
            timestamp,
            nonce,
            signature,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.name.write(writer);
        self.description.write(writer);
        self.url.write(writer);
        self.permissions.write(writer);

        // XSWD v2.0: Write Ed25519 signature fields
        self.public_key.write(writer);
        self.timestamp.write(writer);
        self.nonce.write(writer);
        self.signature.write(writer);
    }
}

pub type EncryptionKey = [u8; 32];

#[derive(Serialize, Deserialize, Debug)]
pub enum EncryptionMode {
    // No encryption, just transfer the data as is (discouraged)
    None,
    // Encrypt the data using AES-GCM
    AES { key: EncryptionKey },
    // Encrypt the data using ChaCha20Poly1305 AEAD cipher
    Chacha20Poly1305 { key: EncryptionKey },
}

impl Serializer for EncryptionMode {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let mode = reader.read_u8()?;
        match mode {
            0 => Ok(Self::None),
            1 => Ok(Self::AES {
                key: reader.read_bytes(32)?,
            }),
            2 => Ok(Self::Chacha20Poly1305 {
                key: reader.read_bytes(32)?,
            }),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn write(&self, writer: &mut Writer) {
        match self {
            Self::None => writer.write_u8(0),
            Self::AES { key } => {
                writer.write_u8(1);
                key.write(writer);
            }
            Self::Chacha20Poly1305 { key } => {
                writer.write_u8(2);
                key.write(writer);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApplicationDataRelayer {
    // Actual application data
    pub inner: ApplicationData,
    // Relayer URL where we should connect
    // to communicate with the application
    pub relayer: String,
    // Encryption mode to use for the relayer
    pub encryption_mode: EncryptionMode,
}

impl Serializer for ApplicationDataRelayer {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let inner = ApplicationData::read(reader)?;
        let n = reader.read_u16()?;
        let relayer = reader.read_string_with_size(n as _)?;
        let encryption_mode = EncryptionMode::read(reader)?;
        Ok(Self {
            inner,
            relayer,
            encryption_mode,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.inner.write(writer);

        let bytes = self.relayer.as_bytes();
        writer.write_u16(bytes.len() as u16);
        bytes.write(writer);

        self.encryption_mode.write(writer);
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Allow,
    Reject,
    Ask,
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Reject => write!(f, "reject"),
            Self::Ask => write!(f, "ask"),
        }
    }
}

pub enum PermissionRequest<'a> {
    Application,
    Request(&'a RpcRequest),
}

pub enum PermissionResult {
    Accept,
    Reject,
    AlwaysAccept,
    AlwaysReject,
}

impl PermissionResult {
    pub fn is_positive(&self) -> bool {
        matches!(self, Self::Accept | Self::AlwaysAccept)
    }
}
