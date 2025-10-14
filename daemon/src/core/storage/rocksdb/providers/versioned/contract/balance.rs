use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;
use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::Column,
        RocksStorage,
        VersionedContractBalanceProvider
    }
};

#[async_trait]
impl VersionedContractBalanceProvider for RocksStorage {
    async fn delete_versioned_contract_balances_data_at_topoheight(&mut self, topoheight: TopoHeight) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned contract balances at topoheight {}", topoheight);
        }
        self.delete_versioned_at_topoheight(Column::ContractsBalances, Column::VersionedContractsBalances, topoheight)
    }

    async fn delete_versioned_contract_balances_above_topoheight(&mut self, topoheight: TopoHeight) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned contract balances above topoheight {}", topoheight);
        }
        self.delete_versioned_above_topoheight(Column::ContractsBalances, Column::VersionedContractsBalances, topoheight)
    }

    async fn delete_versioned_contract_balances_below_topoheight(&mut self, topoheight: TopoHeight, keep_last: bool) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned contract balances below topoheight {}", topoheight);
        }
        self.delete_versioned_below_topoheight(Column::ContractsBalances, Column::VersionedContractsBalances, topoheight, keep_last)
    }
}