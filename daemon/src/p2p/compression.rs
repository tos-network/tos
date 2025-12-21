use chacha20poly1305::aead::Buffer;
use human_bytes::human_bytes;
use log::trace;
use metrics::histogram;
use snap::raw::{Decoder, Encoder};
use std::time::Instant;
use thiserror::Error;
use tos_common::tokio::sync::Mutex;

use crate::config::PEER_MAX_PACKET_SIZE;

/// Compression threshold in bytes - packets smaller than this won't be compressed
pub const COMPRESSION_THRESHOLD: usize = 1024; // 1 KiB

#[derive(Debug, Error)]
pub enum CompressionError {
    #[error("Compression error")]
    Compression,
    #[error("Decompression error")]
    Decompression,
    #[error("Buffer error")]
    Buffer,
    #[error("Already initialized")]
    Initialized,
}

/// Compression handler for P2P packets using Snappy algorithm
pub struct Compression {
    // Encoder & Decoder for compressing/decompressing packets
    // they both have their own buffer to avoid reallocating all the time
    // Both are in their own Mutex to allow read & write at same time
    encoder: Option<Mutex<(Encoder, Vec<u8>)>>,
    decoder: Option<Mutex<(Decoder, Vec<u8>)>>,
}

impl Compression {
    pub fn new() -> Self {
        Self {
            encoder: None,
            decoder: None,
        }
    }

    /// Check if compression is enabled
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.encoder.is_some()
    }

    /// Setup the encoder & decoder with their buffers
    pub fn enable(&mut self) -> Result<(), CompressionError> {
        if self.encoder.is_some() || self.decoder.is_some() {
            return Err(CompressionError::Initialized);
        }

        let buffer = vec![0; snap::raw::max_compress_len(PEER_MAX_PACKET_SIZE as usize)];
        self.encoder = Some(Mutex::new((Encoder::new(), buffer)));

        let buffer = vec![0; PEER_MAX_PACKET_SIZE as usize];
        self.decoder = Some(Mutex::new((Decoder::new(), buffer)));

        Ok(())
    }

    /// Compress the input buffer if its size is greater than COMPRESSION_THRESHOLD
    /// If compression was not enabled, this is a no-op
    pub async fn compress(&self, input: &mut impl Buffer) -> Result<(), CompressionError> {
        if let Some(mutex) = self.encoder.as_ref() {
            let mut should_compress = input.len() > COMPRESSION_THRESHOLD;
            if should_compress {
                let start = Instant::now();

                let mut lock = mutex.lock().await;
                let (encoder, buffer) = &mut *lock;

                let len = input.len();
                let mut n = encoder
                    .compress(input.as_ref(), buffer)
                    .map_err(|_| CompressionError::Compression)?;

                let use_compressed = n < len && n + 1 <= PEER_MAX_PACKET_SIZE as usize;
                if use_compressed {
                    if len < n {
                        input
                            .extend_from_slice(&buffer[len..n])
                            .map_err(|_| CompressionError::Buffer)?;
                        n = input.len();
                    } else {
                        input.truncate(n);
                    }

                    // Reinject the compressed data in our input buffer
                    input.as_mut().copy_from_slice(&buffer[..n]);

                    let elapsed = start.elapsed();
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Packet compressed from {} to {} in {:?}",
                            human_bytes(len as f64),
                            human_bytes(n as f64),
                            elapsed
                        );
                    }
                    histogram!("tos_p2p_compress").record(elapsed.as_millis() as f64);
                } else {
                    should_compress = false;
                }
            }

            if input.len() + 1 > PEER_MAX_PACKET_SIZE as usize {
                return Err(CompressionError::Buffer);
            }

            // Add compression flag byte at the end
            input
                .extend_from_slice(&[should_compress as u8])
                .map_err(|_| CompressionError::Buffer)?;
        }

        Ok(())
    }

    /// Decompress the input buffer if the last byte indicates it was compressed
    /// If compression was not enabled, this is a no-op
    pub async fn decompress(&self, buf: &mut impl Buffer) -> Result<(), CompressionError> {
        if let Some(mutex) = self.decoder.as_ref() {
            if buf.len() < 1 {
                return Err(CompressionError::Buffer);
            }

            // Check the compression flag at the end
            let compressed = buf.as_ref()[buf.len() - 1] == 1;
            buf.truncate(buf.len() - 1);

            if compressed {
                let start = Instant::now();
                let mut lock = mutex.lock().await;
                let (decoder, buffer) = &mut *lock;

                let mut n = decoder
                    .decompress(buf.as_ref(), buffer)
                    .map_err(|_| CompressionError::Decompression)?;

                let len = buf.len();
                // Assemble the buffer by calculating the new length
                if n > len {
                    buf.extend_from_slice(&buffer[len..n])
                        .map_err(|_| CompressionError::Buffer)?;
                    n = buf.len();
                } else {
                    buf.truncate(n);
                }

                // Reinject in our buffer the decompressed data
                buf.as_mut().copy_from_slice(&buffer[..n]);

                let elapsed = start.elapsed();
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Packet decompressed from {} to {} in {:?}",
                        human_bytes(len as f64),
                        human_bytes(n as f64),
                        elapsed
                    );
                }
                histogram!("tos_p2p_decompress").record(elapsed.as_millis() as f64);
            }
        }

        Ok(())
    }
}

impl Default for Compression {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compression() {
        let mut compression = Compression::new();
        compression.enable().unwrap();

        // Create test data larger than threshold
        let data = vec![0u8; 2048];
        let mut buffer = data.clone();

        compression.compress(&mut buffer).await.unwrap();
        // Compressed data + 1 byte flag should be smaller than original
        assert!(buffer.len() < data.len() + 1);

        compression.decompress(&mut buffer).await.unwrap();
        assert_eq!(buffer, data);
    }

    #[tokio::test]
    async fn test_compression_small_data() {
        let mut compression = Compression::new();
        compression.enable().unwrap();

        // Create test data smaller than threshold - should not be compressed
        let data = vec![42u8; 100];
        let mut buffer = data.clone();

        compression.compress(&mut buffer).await.unwrap();
        // Should just have added the flag byte (0 = not compressed)
        assert_eq!(buffer.len(), data.len() + 1);
        assert_eq!(buffer.last(), Some(&0u8));

        compression.decompress(&mut buffer).await.unwrap();
        assert_eq!(buffer, data);
    }

    #[tokio::test]
    async fn test_compression_disabled() {
        let compression = Compression::new();

        let data = vec![0u8; 2048];
        let mut buffer = data.clone();

        // Should be a no-op when not enabled
        compression.compress(&mut buffer).await.unwrap();
        assert_eq!(buffer, data);

        compression.decompress(&mut buffer).await.unwrap();
        assert_eq!(buffer, data);
    }
}
