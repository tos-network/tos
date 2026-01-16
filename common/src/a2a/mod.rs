pub type Value = serde_json::Value;

pub mod constants;
pub mod errors;
pub mod limits;
pub mod types;

pub use constants::*;
pub use errors::*;
pub use limits::*;
pub use types::*;

#[cfg(feature = "tokio")]
pub mod service;

#[cfg(feature = "tokio")]
pub use service::*;

pub type TaskId = String;
pub type MessageId = String;
pub type ArtifactId = String;
pub type ContextId = String;

#[cfg(test)]
mod tests;
