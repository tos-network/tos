// TNS (TOS Name Service) Transaction Payloads

use serde::{Deserialize, Serialize};

use crate::{
    crypto::Hash,
    serializer::*,
    tns::{MAX_ENCRYPTED_SIZE, MAX_NAME_LENGTH, MAX_TTL, MIN_NAME_LENGTH, MIN_TTL},
};

// ============================================================================
// RegisterNamePayload
// ============================================================================

/// Payload for registering a TNS name
/// The name is stored as-is, but will be normalized (lowercased) during verification
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisterNamePayload {
    /// Username part (without @tos.network suffix)
    name: String,
}

impl RegisterNamePayload {
    /// Create a new RegisterNamePayload
    pub fn new(name: String) -> Self {
        Self { name }
    }

    /// Get the name
    pub fn get_name(&self) -> &str {
        &self.name
    }
}

impl Serializer for RegisterNamePayload {
    fn write(&self, writer: &mut Writer) {
        // Use u8 for length (max 64 chars)
        // Debug assert to catch programming errors - name should be validated before construction
        debug_assert!(
            self.name.len() <= MAX_NAME_LENGTH,
            "Name length {} exceeds max {}",
            self.name.len(),
            MAX_NAME_LENGTH
        );
        // Saturating cast to prevent overflow/truncation in release builds
        let len = self.name.len().min(u8::MAX as usize) as u8;
        writer.write_u8(len);
        writer.write_bytes(&self.name.as_bytes()[..len as usize]);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // 1. Read length first
        let name_len = reader.read_u8()? as usize;

        // 2. Validate length before allocating memory (OOM protection)
        if !(MIN_NAME_LENGTH..=MAX_NAME_LENGTH).contains(&name_len) {
            return Err(ReaderError::InvalidSize);
        }

        // 3. Safe allocation (known length <= 64)
        let name_bytes: Vec<u8> = reader.read_bytes(name_len)?;

        // 4. UTF-8 validation
        let name = String::from_utf8(name_bytes).map_err(|_| ReaderError::InvalidValue)?;

        Ok(Self { name })
    }

    fn size(&self) -> usize {
        1 + self.name.len() // 1 byte length + name bytes
    }
}

// ============================================================================
// EphemeralMessagePayload
// ============================================================================

/// Payload for sending an ephemeral message
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EphemeralMessagePayload {
    /// Sender's name hash (blake3 of sender's registered name)
    sender_name_hash: Hash,
    /// Recipient's name hash (blake3 of recipient's registered name)
    recipient_name_hash: Hash,
    /// Message nonce for replay protection (from transaction nonce)
    message_nonce: u64,
    /// TTL in blocks (message expires after this many blocks)
    ttl_blocks: u32,
    /// Encrypted message content (using extra_data encryption scheme)
    encrypted_content: Vec<u8>,
    /// Receiver handle for decryption (r * Pk_receiver)
    receiver_handle: [u8; 32],
}

impl EphemeralMessagePayload {
    /// Create a new EphemeralMessagePayload
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sender_name_hash: Hash,
        recipient_name_hash: Hash,
        message_nonce: u64,
        ttl_blocks: u32,
        encrypted_content: Vec<u8>,
        receiver_handle: [u8; 32],
    ) -> Self {
        Self {
            sender_name_hash,
            recipient_name_hash,
            message_nonce,
            ttl_blocks,
            encrypted_content,
            receiver_handle,
        }
    }

    /// Get sender's name hash
    pub fn get_sender_name_hash(&self) -> &Hash {
        &self.sender_name_hash
    }

    /// Get recipient's name hash
    pub fn get_recipient_name_hash(&self) -> &Hash {
        &self.recipient_name_hash
    }

    /// Get message nonce
    pub fn get_message_nonce(&self) -> u64 {
        self.message_nonce
    }

    /// Get TTL in blocks
    pub fn get_ttl_blocks(&self) -> u32 {
        self.ttl_blocks
    }

    /// Get encrypted content
    pub fn get_encrypted_content(&self) -> &[u8] {
        &self.encrypted_content
    }

    /// Get receiver handle
    pub fn get_receiver_handle(&self) -> &[u8; 32] {
        &self.receiver_handle
    }
}

impl Serializer for EphemeralMessagePayload {
    fn write(&self, writer: &mut Writer) {
        self.sender_name_hash.write(writer);
        self.recipient_name_hash.write(writer);
        writer.write_u64(&self.message_nonce);
        writer.write_u32(&self.ttl_blocks);
        // Use u16 for content length (max 188 bytes = 140 + 48 overhead)
        // Debug assert to catch programming errors - content should be validated before construction
        debug_assert!(
            self.encrypted_content.len() <= MAX_ENCRYPTED_SIZE,
            "Encrypted content length {} exceeds max {}",
            self.encrypted_content.len(),
            MAX_ENCRYPTED_SIZE
        );
        // Saturating cast to prevent overflow/truncation in release builds
        let len = self.encrypted_content.len().min(u16::MAX as usize) as u16;
        writer.write_u16(len);
        writer.write_bytes(&self.encrypted_content[..len as usize]);
        writer.write_bytes(&self.receiver_handle);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // 1. sender_name_hash: fixed 32 bytes
        let sender_name_hash = Hash::read(reader)?;

        // 2. recipient_name_hash: fixed 32 bytes
        let recipient_name_hash = Hash::read(reader)?;

        // 3. message_nonce: fixed 8 bytes (replay protection)
        let message_nonce = reader.read_u64()?;

        // 4. ttl_blocks: fixed 4 bytes + range check (defense in depth)
        let ttl_blocks = reader.read_u32()?;
        if !(MIN_TTL..=MAX_TTL).contains(&ttl_blocks) {
            return Err(ReaderError::InvalidValue);
        }

        // 5. encrypted_content: variable length, must be limited
        let content_len = reader.read_u16()? as usize;
        if content_len == 0 || content_len > MAX_ENCRYPTED_SIZE {
            return Err(ReaderError::InvalidSize);
        }

        let encrypted_content: Vec<u8> = reader.read_bytes(content_len)?;

        // 6. receiver_handle: fixed 32 bytes
        let receiver_handle: [u8; 32] = reader.read_bytes_32()?;

        Ok(Self {
            sender_name_hash,
            recipient_name_hash,
            message_nonce,
            ttl_blocks,
            encrypted_content,
            receiver_handle,
        })
    }

    fn size(&self) -> usize {
        32 + 32 + 8 + 4 + 2 + self.encrypted_content.len() + 32 // 110 + content_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_name_payload_serialization() {
        let payload = RegisterNamePayload::new("alice".to_string());

        // Serialize
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        payload.write(&mut writer);

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let decoded = RegisterNamePayload::read(&mut reader).unwrap();

        assert_eq!(decoded.get_name(), "alice");
    }

    #[test]
    fn test_register_name_payload_size_limits() {
        // Too short (2 chars)
        let short_bytes = vec![2, b'a', b'b'];
        let mut reader = Reader::new(&short_bytes);
        assert!(RegisterNamePayload::read(&mut reader).is_err());

        // Too long (65 chars)
        let mut long_bytes = vec![65];
        long_bytes.extend(vec![b'a'; 65]);
        let mut reader = Reader::new(&long_bytes);
        assert!(RegisterNamePayload::read(&mut reader).is_err());
    }

    #[test]
    fn test_ephemeral_message_payload_serialization() {
        let sender_hash = Hash::zero();
        let recipient_hash = Hash::zero();
        let payload = EphemeralMessagePayload::new(
            sender_hash,
            recipient_hash,
            42,
            1000,
            vec![1, 2, 3, 4],
            [0u8; 32],
        );

        // Serialize
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        payload.write(&mut writer);

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let decoded = EphemeralMessagePayload::read(&mut reader).unwrap();

        assert_eq!(decoded.get_message_nonce(), 42);
        assert_eq!(decoded.get_ttl_blocks(), 1000);
        assert_eq!(decoded.get_encrypted_content(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_ephemeral_message_ttl_limits() {
        let sender_hash = Hash::zero();
        let recipient_hash = Hash::zero();

        // Valid TTL
        let payload =
            EphemeralMessagePayload::new(sender_hash, recipient_hash, 1, 1000, vec![1], [0u8; 32]);
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        payload.write(&mut writer);
        let mut reader = Reader::new(&bytes);
        assert!(EphemeralMessagePayload::read(&mut reader).is_ok());
    }
}
