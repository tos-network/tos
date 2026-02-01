//! Discv6-based peer discovery protocol for TOS nodes.
//!
//! This module implements a discovery protocol similar to Ethereum's discv5,
//! enabling nodes to find peers in a decentralized manner using Kademlia-style
//! routing.
//!
//! ## Features
//!
//! - **UDP-based discovery**: Low-overhead peer discovery using UDP packets
//! - **Ed25519 node identity**: Cryptographic node authentication
//! - **Kademlia routing**: Efficient peer lookup using XOR distance
//! - **Bootnode support**: Discovery-only mode for dedicated bootstrap nodes
//!
//! ## Message Types
//!
//! | Type | ID | Description |
//! |------|-----|-------------|
//! | PING | 0x01 | Liveness check and node info exchange |
//! | PONG | 0x02 | Response to PING |
//! | FINDNODE | 0x03 | Request nodes close to a target ID |
//! | NEIGHBORS | 0x04 | Response with node list |
//!
//! ## Node URL Format
//!
//! ```text
//! tosnode://<node_id_hex>@<ip>:<port>
//! ```
//!
//! Example: `tosnode://1a2b3c4d5e6f...@192.168.1.1:2126`
//!
//! ## Usage
//!
//! ### Running a Bootnode
//!
//! ```bash
//! tos_daemon --p2p-discovery-only --discovery-port 2126
//! ```
//!
//! ### Connecting to a Bootnode
//!
//! ```bash
//! tos_daemon --discovery-bootstrap "tosnode://...@127.0.0.1:2126"
//! ```
//!
//! ## Constants
//!
//! - Default discovery port: 2126
//! - Default P2P port: 2125
//! - K-bucket size: 16 nodes
//! - Alpha (parallel lookups): 3
//! - Max packet size: 1280 bytes
//! - Message expiration: 20 seconds

pub mod config;
pub mod error;
pub mod identity;
pub mod messages;
pub mod routing_table;
pub mod server;
pub mod url;

pub use config::DiscoveryConfig;
pub use error::{DiscoveryError, DiscoveryResult};
pub use identity::{NodeId, NodeIdentity};
pub use messages::{Message, NodeInfo, SignedPacket};
pub use routing_table::RoutingTable;
pub use server::DiscoveryServer;
pub use url::TosNodeUrl;
