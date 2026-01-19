use std::env;

use once_cell::sync::OnceCell;

use tos_common::crypto::{KeyPair, PrivateKey};
use tos_common::serializer::Serializer;

use super::ArbitrationError;

const COORDINATOR_SECRET_ENV: &str = "TOS_ARBITRATION_COORDINATOR_PRIVATE_KEY";

static COORDINATOR_KEYPAIR: OnceCell<KeyPair> = OnceCell::new();

pub fn coordinator_keypair() -> Result<KeyPair, ArbitrationError> {
    COORDINATOR_KEYPAIR
        .get_or_try_init(load_coordinator_keypair)
        .map(Clone::clone)
}

fn load_coordinator_keypair() -> Result<KeyPair, ArbitrationError> {
    let secret =
        env::var(COORDINATOR_SECRET_ENV).map_err(|_| ArbitrationError::CoordinatorKeyMissing)?;
    let private = PrivateKey::from_hex(&secret)
        .map_err(|e| ArbitrationError::Transaction(format!("invalid coordinator key: {e}")))?;
    Ok(KeyPair::from_private_key(private))
}
