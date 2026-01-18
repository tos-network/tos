use serde::{Deserialize, Serialize};

use crate::serializer::{Reader, ReaderError, Serializer, Writer};

/// CancelArbiterExitPayload cancels an exit request.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CancelArbiterExitPayload;

impl CancelArbiterExitPayload {
    pub fn new() -> Self {
        Self
    }
}

impl Serializer for CancelArbiterExitPayload {
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
    fn cancel_arbiter_exit_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = CancelArbiterExitPayload::new();
        let data = serde_json::to_vec(&payload)?;
        let _decoded: CancelArbiterExitPayload = serde_json::from_slice(&data)?;
        Ok(())
    }
}
