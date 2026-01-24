//! Block verification/mining benchmark helpers.

use crate::tier2_integration::{TestDaemon, TestDaemonBuilder};
use anyhow::Result;

/// Build a TestDaemon with a given number of funded accounts.
pub async fn build_daemon(funded_accounts: usize) -> Result<TestDaemon> {
    TestDaemonBuilder::new()
        .with_funded_accounts(funded_accounts)
        .build()
        .await
}

/// Mine a single block on the provided daemon.
pub async fn mine_one_block(daemon: &TestDaemon) -> Result<()> {
    let _ = daemon.mine_block().await?;
    Ok(())
}
