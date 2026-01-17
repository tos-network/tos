use super::{
    blockchain::{Blockchain, BroadcastOption},
    storage::Storage,
};
use log::{error, info};
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tos_common::{block::Block, config::TIPS_LIMIT, crypto::KeyPair, tokio::time::interval};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Simulator {
    // Mine only one block every BLOCK_TIME
    Blockchain,
    // Mine random 1-5 blocks every BLOCK_TIME to enable BlockDAG
    BlockDag,
    // Same as blockDAG but generates much more blocks and TXs for stress test
    Stress,
}

impl FromStr for Simulator {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "blockchain" | "0" => Self::Blockchain,
            "blockdag" | "1" => Self::BlockDag,
            "stress" | "2" => Self::Stress,
            _ => return Err("Invalid simulator type".into()),
        })
    }
}

impl Serialize for Simulator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'a> Deserialize<'a> for Simulator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;
        Simulator::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Display for Simulator {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match &self {
            Self::Blockchain => "blockchain",
            Self::BlockDag => "blockdag",
            Self::Stress => "stress",
        };
        write!(f, "{}", str)
    }
}

impl Simulator {
    // Start the Simulator mode to generate new blocks automatically
    // It generates random miner keys and mine blocks with them
    pub async fn start<S: Storage>(&self, blockchain: Arc<Blockchain<S>>) {
        let millis_interval = match self {
            Self::Stress => 300,
            _ => 5000,
        };

        let mut interval = interval(Duration::from_millis(millis_interval));
        let mut rng = OsRng;
        let mut keys: Vec<KeyPair> = Vec::new();

        // Generate 100 random keys for mining
        for _ in 0..100 {
            keys.push(KeyPair::new());
        }

        loop {
            interval.tick().await;
            if log::log_enabled!(log::Level::Info) {
                info!("Adding new simulated block...");
            }
            // Number of blocks to generate
            let blocks_count = match self {
                Self::BlockDag => rng.gen_range(1..=TIPS_LIMIT),
                Self::Stress => rng.gen_range(1..=10),
                _ => 1,
            };

            // Generate blocks
            let blocks = self
                .generate_blocks(blocks_count, &mut rng, &keys, &blockchain)
                .await;

            // Add all blocks to the chain
            for block in blocks {
                match blockchain
                    .add_new_block(block, None, BroadcastOption::None, false)
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        if log::log_enabled!(log::Level::Error) {
                            error!("Error while adding block: {}", e);
                        }
                    }
                }
            }
        }
    }

    async fn generate_blocks(
        &self,
        max_blocks: usize,
        rng: &mut OsRng,
        keys: &Vec<KeyPair>,
        blockchain: &Arc<Blockchain<impl Storage>>,
    ) -> Vec<Block> {
        if log::log_enabled!(log::Level::Info) {
            info!("Adding simulated blocks");
        }
        let n = rng.gen_range(1..=max_blocks);
        let mut blocks = Vec::with_capacity(n);
        for _ in 0..n {
            let index = rng.gen_range(0..keys.len());
            let selected_key = keys[index].get_public_key();
            match blockchain.mine_block(&selected_key.compress()).await {
                Ok(block) => {
                    blocks.push(block);
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Error while mining block: {}", e);
                    }
                }
            }
        }

        blocks
    }
}
