//! Sync benchmark helpers.

use crate::tier2_integration::{TestDaemon, TestDaemonBuilder};
use anyhow::Result;

/// Build a source/target daemon pair for sync benchmarking.
pub async fn build_pair(funded_accounts: usize) -> Result<(TestDaemon, TestDaemon)> {
    let source = TestDaemonBuilder::new()
        .with_funded_accounts(funded_accounts)
        .build()
        .await?;
    let target = TestDaemonBuilder::new()
        .with_funded_accounts(funded_accounts)
        .build()
        .await?;
    Ok((source, target))
}

/// Mine a fixed number of blocks on the source daemon.
pub async fn mine_blocks(source: &TestDaemon, count: u64) -> Result<()> {
    for _ in 0..count {
        let _ = source.mine_block().await?;
    }
    Ok(())
}

/// Replay blocks from source to target by height.
pub async fn sync_blocks(source: &TestDaemon, target: &TestDaemon, count: u64) -> Result<()> {
    for height in 1..=count {
        let block = source
            .get_block_at_height(height)
            .await?
            .ok_or_else(|| anyhow::anyhow!("block not found at height {}", height))?;
        target.receive_block(block).await?;
    }
    Ok(())
}
