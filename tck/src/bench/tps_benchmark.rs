//! TPS benchmark helpers.

use crate::tier1_component::TestTransaction;
use crate::tier2_integration::{TestDaemon, TestDaemonBuilder};
use anyhow::Result;
use tos_common::crypto::Hash;

/// Deterministically derive a test public key hash from a byte seed.
pub fn test_pubkey(id: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = id;
    for (i, byte) in bytes.iter_mut().enumerate().skip(1) {
        *byte = (id.wrapping_mul(i as u8)).wrapping_add(i as u8);
    }
    Hash::new(bytes)
}

/// Build a TestDaemon for TPS benchmarks.
pub async fn build_daemon(funded_accounts: usize) -> Result<TestDaemon> {
    TestDaemonBuilder::new()
        .with_funded_accounts(funded_accounts)
        .build()
        .await
}

/// Submit a batch of simple transfers for TPS measurement.
pub async fn submit_basic_transfers(
    daemon: &TestDaemon,
    sender: Hash,
    recipient: Hash,
    count: u64,
) -> Result<()> {
    for i in 0..count {
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: sender.clone(),
            recipient: recipient.clone(),
            amount: 10,
            fee: 1,
            nonce: i + 1,
        };
        let _ = daemon.submit_transaction(tx).await?;
    }
    Ok(())
}
