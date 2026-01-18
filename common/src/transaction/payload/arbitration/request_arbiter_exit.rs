use serde::{Deserialize, Serialize};

use crate::serializer::{Reader, ReaderError, Serializer, Writer};

/// RequestArbiterExitPayload initiates arbiter exit and cooldown.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RequestArbiterExitPayload;

impl RequestArbiterExitPayload {
    pub fn new() -> Self {
        Self
    }
}

impl Serializer for RequestArbiterExitPayload {
    fn write(&self, _writer: &mut Writer) {}

    fn read(_reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self)
    }

    fn size(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_arbiter_exit_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = RequestArbiterExitPayload::new();
        let data = serde_json::to_vec(&payload)?;
        let _decoded: RequestArbiterExitPayload = serde_json::from_slice(&data)?;
        Ok(())
    }
}
