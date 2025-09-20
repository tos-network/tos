use chacha20poly1305::{
    aead::Aead,
    XNonce,
    XChaCha20Poly1305,
    AeadCore,
    KeyInit
};
use tos_common::crypto::{
    HASH_SIZE,
    hash
};
use crate::{error::WalletError, config::SALT_SIZE};


pub struct Cipher {
    cipher: XChaCha20Poly1305,
    // this salt is used for keys and values
    salt: Option<[u8; SALT_SIZE]>
}

impl Cipher {
    pub const NONCE_SIZE: usize = 24;

    pub fn new(key: &[u8], salt: Option<[u8; SALT_SIZE]>) -> Result<Self, WalletError> {
        Ok(Self {
            cipher: XChaCha20Poly1305::new_from_slice(key).map_err(|_| WalletError::Cipher)?,
            salt
        })
    }

    // encrypt value passed in param and add plaintext nonce before encrypted value
    // a Nonce is generated randomly at each call
    pub fn encrypt_value(&self, value: &[u8]) -> Result<Vec<u8>, WalletError> {
        // generate unique random nonce
        let nonce = XChaCha20Poly1305::generate_nonce()
            .map_err(|_| WalletError::NonceGeneration)?;

        self.encrypt_value_with_nonce(value, &nonce.into())
    }

    // encrypt value passed in param and add plaintext nonce before encrypted value
    pub fn encrypt_value_with_nonce(&self, value: &[u8], nonce: &[u8; Self::NONCE_SIZE]) -> Result<Vec<u8>, WalletError> {
        let mut plaintext: Vec<u8> = Vec::with_capacity(SALT_SIZE + value.len());
        // add salt to the plaintext value
        if let Some(salt) = &self.salt {
            plaintext.extend_from_slice(salt);
        }
        plaintext.extend_from_slice(value);

        // encrypt data using plaintext and nonce
        let data = &self.cipher.encrypt(nonce.into(), plaintext.as_slice())
            .map_err(|e| WalletError::CryptoError(e))?;

        // append unique nonce to the encrypted data
        let mut encrypted = Vec::with_capacity(Self::NONCE_SIZE + data.len());
        encrypted.extend_from_slice(nonce);
        encrypted.extend_from_slice(data);

        Ok(encrypted)
    }

    // decrypt any value loaded from disk, with the format of above function
    pub fn decrypt_value(&self, encrypted: &[u8]) -> Result<Vec<u8>, WalletError> {
        // nonce is 24 bytes and is mandatory in encrypted slice
        if encrypted.len() < 25 {
            return Err(WalletError::InvalidEncryptedValue.into())
        }

        // read the nonce for this data 
        let nonce = XNonce::try_from(&encrypted[0..24])
            .map_err(|_| WalletError::NonceGeneration)?;

        // decrypt the value using the nonce previously decoded
        let mut decrypted = self.cipher.decrypt(&nonce, &encrypted[nonce.len()..]).map_err(|e| WalletError::CryptoError(e))?;
        // delete the salt from the decrypted slice
        if let Some(salt) = &self.salt {
            decrypted.drain(0..salt.len());
        }

        Ok(decrypted)
    }

    // hash the key with salt
    pub fn hash_key<S: AsRef<[u8]>>(&self, key: S) -> [u8; HASH_SIZE] {
        let mut data = Vec::new();
        if let Some(salt) = &self.salt {
            data.extend_from_slice(salt);
        }
        data.extend_from_slice(key.as_ref());
        hash(&data).to_bytes()
    }
}