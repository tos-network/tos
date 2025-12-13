use super::{SledStorage, DB_VERSION};
use crate::core::error::BlockchainError;
use log::debug;
use tos_common::config::VERSION;

impl SledStorage {
    pub(super) fn handle_migrations(&mut self) -> Result<(), BlockchainError> {
        debug!("set DB version to {}", VERSION);
        self.extra.insert(DB_VERSION, VERSION)?;

        Ok(())
    }
}
