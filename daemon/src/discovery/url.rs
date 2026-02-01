//! tosnode:// URL parser for discovery protocol.
//!
//! Format: `tosnode://<node_id_hex>@<ip>:<port>`
//!
//! Example: `tosnode://1a2b3c4d5e6f...@192.168.1.1:2126`

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

use super::error::{DiscoveryError, DiscoveryResult};
use super::identity::NodeId;
use tos_common::crypto::Hash;

/// URL scheme for TOS discovery nodes.
pub const TOSNODE_URL_SCHEME: &str = "tosnode://";

/// Parsed tosnode:// URL containing node ID and socket address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TosNodeUrl {
    /// The node ID (SHA3-256 hash of public key, 32 bytes).
    pub node_id: NodeId,
    /// The socket address (IP:port).
    pub address: SocketAddr,
}

impl TosNodeUrl {
    /// Create a new TosNodeUrl.
    pub fn new(node_id: NodeId, address: SocketAddr) -> Self {
        Self { node_id, address }
    }

    /// Parse a tosnode:// URL string.
    ///
    /// Format: `tosnode://<node_id_hex>@<ip>:<port>`
    ///
    /// # Arguments
    /// * `s` - The URL string to parse
    ///
    /// # Returns
    /// * `Ok(TosNodeUrl)` if parsing succeeds
    /// * `Err(DiscoveryError::InvalidUrl)` if the format is invalid
    pub fn parse(s: &str) -> DiscoveryResult<Self> {
        // Check scheme
        let rest = s.strip_prefix(TOSNODE_URL_SCHEME).ok_or_else(|| {
            DiscoveryError::InvalidUrl(format!(
                "URL must start with '{}', got: {}",
                TOSNODE_URL_SCHEME, s
            ))
        })?;

        // Split node_id@address
        let parts: Vec<&str> = rest.splitn(2, '@').collect();
        if parts.len() != 2 {
            return Err(DiscoveryError::InvalidUrl(format!(
                "URL must contain '@' separator between node_id and address: {}",
                s
            )));
        }

        let node_id_hex = parts[0];
        let address_str = parts[1];

        // Validate and parse node ID (64 hex chars = 32 bytes)
        if node_id_hex.len() != 64 {
            return Err(DiscoveryError::InvalidUrl(format!(
                "Node ID must be 64 hex characters (32 bytes), got {} characters",
                node_id_hex.len()
            )));
        }

        let node_id_bytes = hex::decode(node_id_hex)
            .map_err(|e| DiscoveryError::InvalidUrl(format!("Invalid node ID hex: {}", e)))?;

        let mut node_id_array = [0u8; 32];
        node_id_array.copy_from_slice(&node_id_bytes);
        let node_id = Hash::new(node_id_array);

        // Parse socket address
        let address: SocketAddr = address_str.parse().map_err(|e| {
            DiscoveryError::InvalidUrl(format!("Invalid socket address '{}': {}", address_str, e))
        })?;

        Ok(Self { node_id, address })
    }

    /// Convert to URL string.
    pub fn to_string_url(&self) -> String {
        format!(
            "{}{}@{}",
            TOSNODE_URL_SCHEME,
            hex::encode(self.node_id.as_bytes()),
            self.address
        )
    }
}

impl fmt::Display for TosNodeUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_url())
    }
}

impl FromStr for TosNodeUrl {
    type Err = DiscoveryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn sample_node_id() -> NodeId {
        // 32 bytes of predictable data for testing
        Hash::new([
            0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70, 0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7,
            0xf8, 0x09, 0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb,
            0xdc, 0xed, 0xfe, 0x0f,
        ])
    }

    #[test]
    fn test_parse_valid_ipv4() {
        let node_id = sample_node_id();
        let url_str = format!(
            "tosnode://{}@192.168.1.1:2126",
            hex::encode(node_id.as_bytes())
        );

        let parsed = TosNodeUrl::parse(&url_str).unwrap();
        assert_eq!(parsed.node_id, node_id);
        assert_eq!(
            parsed.address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 2126)
        );
    }

    #[test]
    fn test_parse_valid_ipv6() {
        let node_id = sample_node_id();
        let url_str = format!("tosnode://{}@[::1]:2126", hex::encode(node_id.as_bytes()));

        let parsed = TosNodeUrl::parse(&url_str).unwrap();
        assert_eq!(parsed.node_id, node_id);
        assert_eq!(
            parsed.address,
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 2126)
        );
    }

    #[test]
    fn test_parse_missing_scheme() {
        let result = TosNodeUrl::parse("1a2b3c@127.0.0.1:2126");
        assert!(result.is_err());
        if let Err(DiscoveryError::InvalidUrl(msg)) = result {
            assert!(msg.contains("tosnode://"));
        }
    }

    #[test]
    fn test_parse_missing_separator() {
        let node_id = sample_node_id();
        let url_str = format!(
            "tosnode://{}192.168.1.1:2126",
            hex::encode(node_id.as_bytes())
        );

        let result = TosNodeUrl::parse(&url_str);
        assert!(result.is_err());
        if let Err(DiscoveryError::InvalidUrl(msg)) = result {
            assert!(msg.contains("@"));
        }
    }

    #[test]
    fn test_parse_invalid_node_id_length() {
        let url_str = "tosnode://1a2b3c@192.168.1.1:2126";
        let result = TosNodeUrl::parse(url_str);
        assert!(result.is_err());
        if let Err(DiscoveryError::InvalidUrl(msg)) = result {
            assert!(msg.contains("64 hex characters"));
        }
    }

    #[test]
    fn test_parse_invalid_node_id_hex() {
        // 64 characters but not valid hex
        let url_str =
            "tosnode://gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg@192.168.1.1:2126";
        let result = TosNodeUrl::parse(url_str);
        assert!(result.is_err());
        if let Err(DiscoveryError::InvalidUrl(msg)) = result {
            assert!(msg.contains("Invalid node ID hex"));
        }
    }

    #[test]
    fn test_parse_invalid_address() {
        let node_id = sample_node_id();
        let url_str = format!(
            "tosnode://{}@not-an-address",
            hex::encode(node_id.as_bytes())
        );

        let result = TosNodeUrl::parse(&url_str);
        assert!(result.is_err());
        if let Err(DiscoveryError::InvalidUrl(msg)) = result {
            assert!(msg.contains("Invalid socket address"));
        }
    }

    #[test]
    fn test_roundtrip() {
        let node_id = sample_node_id();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 2126);
        let url = TosNodeUrl::new(node_id, address);

        let url_str = url.to_string_url();
        let parsed = TosNodeUrl::parse(&url_str).unwrap();

        assert_eq!(parsed, url);
    }

    #[test]
    fn test_display() {
        let node_id = sample_node_id();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126);
        let url = TosNodeUrl::new(node_id, address);

        let displayed = format!("{}", url);
        assert!(displayed.starts_with(TOSNODE_URL_SCHEME));
        assert!(displayed.contains("@127.0.0.1:2126"));
    }

    #[test]
    fn test_from_str() {
        let node_id = sample_node_id();
        let url_str = format!(
            "tosnode://{}@192.168.1.1:2126",
            hex::encode(node_id.as_bytes())
        );

        let parsed: TosNodeUrl = url_str.parse().unwrap();
        assert_eq!(parsed.node_id, node_id);
    }
}
