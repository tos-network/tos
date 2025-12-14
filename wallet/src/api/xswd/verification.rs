// XSWD v2.0: Ed25519 Signature Verification Module
// This module implements cryptographic verification of application identities
// to prevent impersonation attacks and bind permissions to public keys

use super::error::XSWDError;
use super::types::ApplicationData;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::{SystemTime, UNIX_EPOCH};

// Maximum allowed time difference between application timestamp and server time
// Applications must register within this time window to prevent replay attacks
const MAX_TIMESTAMP_DIFF_SECONDS: u64 = 300; // 5 minutes

// Verify the Ed25519 signature and timestamp of an ApplicationData
// This function performs the following security checks:
// 1. Timestamp validation (must be within 5 minutes of current time)
// 2. Public key format validation
// 3. Signature format validation
// 4. Cryptographic signature verification
//
// Returns Ok(()) if all checks pass, Err(XSWDError) otherwise
pub fn verify_application_signature(app_data: &ApplicationData) -> Result<(), XSWDError> {
    // Security Check 1: Verify timestamp is recent (within 5 minutes)
    // This prevents replay attacks using old signed ApplicationData
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| XSWDError::InvalidTimestamp)?
        .as_secs();

    let timestamp_diff = if now > app_data.get_timestamp() {
        now - app_data.get_timestamp()
    } else {
        app_data.get_timestamp() - now
    };

    if timestamp_diff > MAX_TIMESTAMP_DIFF_SECONDS {
        if log::log_enabled!(log::Level::Warn) {
            log::warn!(
                "Application {} timestamp is too old or in future: {} seconds difference (max: {})",
                app_data.get_id(),
                timestamp_diff,
                MAX_TIMESTAMP_DIFF_SECONDS
            );
        }
        return Err(XSWDError::InvalidTimestamp);
    }

    // Security Check 2: Parse and validate Ed25519 public key
    let verifying_key = VerifyingKey::from_bytes(app_data.get_public_key())
        .map_err(|_| XSWDError::InvalidPublicKey)?;

    // Security Check 3: Parse and validate Ed25519 signature
    let signature = Signature::from_bytes(app_data.get_signature());

    // Security Check 4: Verify signature over deterministic serialization
    // The message signed is: id || name || description || url || permissions || public_key || timestamp || nonce
    let message = app_data.serialize_for_signing();

    verifying_key.verify(&message, &signature).map_err(|_| {
        if log::log_enabled!(log::Level::Warn) {
            log::warn!(
                "Invalid Ed25519 signature for application {}",
                app_data.get_id()
            );
        }
        XSWDError::InvalidSignatureForApplicationData
    })?;

    // All security checks passed
    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            "Application {} signature verified successfully (public_key: {})",
            app_data.get_id(),
            hex::encode(app_data.get_public_key())
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use indexmap::IndexSet;

    // Helper function to create a valid ApplicationData for testing
    fn create_test_app_data(timestamp: u64) -> ApplicationData {
        // Generate random Ed25519 keypair for testing
        let secret_bytes = rand::random::<[u8; 32]>();
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let verifying_key = signing_key.verifying_key();

        let id = hex::encode([0u8; 32]);
        let name = "Test App".to_string();
        let description = "Test Description".to_string();
        let url = Some("https://test.com".to_string());
        let mut permissions = IndexSet::new();
        permissions.insert("get_balance".to_string());
        permissions.insert("get_address".to_string());

        let public_key = verifying_key.to_bytes();
        let nonce = 12345u64;

        // Create temporary ApplicationData for signing
        let temp_app = ApplicationData {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            url: url.clone(),
            permissions: permissions.clone(),
            public_key,
            timestamp,
            nonce,
            signature: [0u8; 64], // Placeholder
        };

        // Sign the message
        let message = temp_app.serialize_for_signing();
        let signature: Signature = signing_key.sign(&message);

        ApplicationData {
            id,
            name,
            description,
            url,
            permissions,
            public_key,
            timestamp,
            nonce,
            signature: signature.to_bytes(),
        }
    }

    #[test]
    fn test_valid_signature_verification() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let app_data = create_test_app_data(now);
        assert!(verify_application_signature(&app_data).is_ok());
    }

    #[test]
    fn test_expired_timestamp_fails() {
        // Create app data with timestamp 10 minutes in the past
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 600;

        let app_data = create_test_app_data(old_timestamp);
        assert!(matches!(
            verify_application_signature(&app_data),
            Err(XSWDError::InvalidTimestamp)
        ));
    }

    #[test]
    fn test_future_timestamp_fails() {
        // Create app data with timestamp 10 minutes in the future
        let future_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 600;

        let app_data = create_test_app_data(future_timestamp);
        assert!(matches!(
            verify_application_signature(&app_data),
            Err(XSWDError::InvalidTimestamp)
        ));
    }

    #[test]
    fn test_tampered_signature_fails() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let app_data = create_test_app_data(now);

        // Tamper with the signature
        let mut tampered_signature = *app_data.get_signature();
        tampered_signature[0] ^= 0xFF;

        let tampered_app = ApplicationData {
            id: app_data.get_id().clone(),
            name: app_data.get_name().clone(),
            description: app_data.get_description().clone(),
            url: app_data.get_url().clone(),
            permissions: app_data.get_permissions().clone(),
            public_key: *app_data.get_public_key(),
            timestamp: app_data.get_timestamp(),
            nonce: app_data.get_nonce(),
            signature: tampered_signature, // Tampered signature
        };

        assert!(matches!(
            verify_application_signature(&tampered_app),
            Err(XSWDError::InvalidSignatureForApplicationData)
        ));
    }

    #[test]
    fn test_tampered_field_fails() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let app_data = create_test_app_data(now);

        // Create new app with tampered name but same signature
        // We keep everything the same except the name
        let tampered_app = ApplicationData {
            id: app_data.get_id().clone(),
            name: "Malicious App".to_string(), // Changed
            description: app_data.get_description().clone(),
            url: app_data.get_url().clone(),
            permissions: app_data.get_permissions().clone(),
            public_key: *app_data.get_public_key(),
            timestamp: app_data.get_timestamp(),
            nonce: app_data.get_nonce(),
            signature: *app_data.get_signature(), // Same signature (invalid!)
        };

        assert!(matches!(
            verify_application_signature(&tampered_app),
            Err(XSWDError::InvalidSignatureForApplicationData)
        ));
    }

    #[test]
    fn test_invalid_public_key_fails() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let app_data = create_test_app_data(now);

        // Create app with invalid public key (not on Ed25519 curve)
        // Using bytes that are guaranteed to not be a valid Ed25519 point
        let invalid_public_key = [0xFFu8; 32]; // This is invalid for Ed25519

        let invalid_app = ApplicationData {
            id: app_data.get_id().clone(),
            name: app_data.get_name().clone(),
            description: app_data.get_description().clone(),
            url: app_data.get_url().clone(),
            permissions: app_data.get_permissions().clone(),
            public_key: invalid_public_key, // Invalid public key
            timestamp: app_data.get_timestamp(),
            nonce: app_data.get_nonce(),
            signature: *app_data.get_signature(),
        };

        let result = verify_application_signature(&invalid_app);

        // Should fail with InvalidPublicKey or InvalidSignatureForApplicationData
        // (depending on whether public key validation or signature verification fails first)
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(XSWDError::InvalidPublicKey) | Err(XSWDError::InvalidSignatureForApplicationData)
        ));
    }
}
