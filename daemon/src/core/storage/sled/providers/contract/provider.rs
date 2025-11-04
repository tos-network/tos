use crate::core::storage::{
    AccountProvider, AssetProvider, BalanceProvider, ContractBalanceProvider, ContractDataProvider,
    ContractProvider as _, NetworkProvider, SledStorage, SupplyProvider,
};
use log::trace;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
    tokio::try_block_on,
};
use tos_vm::ValueCell;

impl ContractStorage for SledStorage {
    fn load_data(
        &self,
        contract: &Hash,
        key: &ValueCell,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "load contract {} key {} data at topoheight {}",
                contract,
                key,
                topoheight
            );
        }
        let res = try_block_on(
            self.get_contract_data_at_maximum_topoheight_for(contract, &key, topoheight),
        )??;

        match res {
            Some((topoheight, data)) => match data.take() {
                Some(data) => Ok(Some((topoheight, Some(data)))),
                None => Ok(Some((topoheight, None))),
            },
            None => Ok(None),
        }
    }

    fn has_data(
        &self,
        contract: &Hash,
        key: &ValueCell,
        topoheight: TopoHeight,
    ) -> Result<bool, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "check if contract {} key {} data exists at topoheight {}",
                contract,
                key,
                topoheight
            );
        }
        let contains = try_block_on(
            self.has_contract_data_at_maximum_topoheight(contract, &key, topoheight),
        )??;
        Ok(contains)
    }

    fn load_data_latest_topoheight(
        &self,
        contract: &Hash,
        key: &ValueCell,
        topoheight: TopoHeight,
    ) -> Result<Option<TopoHeight>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "load data latest topoheight for contract {} key {} at topoheight {}",
                contract,
                key,
                topoheight
            );
        }
        let res = try_block_on(
            self.get_contract_data_topoheight_at_maximum_topoheight_for(contract, &key, topoheight),
        )??;
        Ok(res)
    }

    fn has_contract(&self, contract: &Hash, topoheight: TopoHeight) -> Result<bool, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has contract {} at topoheight {}", contract, topoheight);
        }
        let res = try_block_on(self.has_contract_at_maximum_topoheight(contract, topoheight))??;
        Ok(res)
    }
}

impl ContractProvider for SledStorage {
    fn get_contract_balance_for_asset(
        &self,
        contract: &Hash,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract balance for contract {} asset {}",
                contract,
                asset
            );
        }
        let res = try_block_on(
            self.get_contract_balance_at_maximum_topoheight(contract, asset, topoheight),
        )??;
        Ok(res.map(|(topoheight, balance)| (topoheight, balance.take())))
    }

    fn asset_exists(&self, asset: &Hash, topoheight: TopoHeight) -> Result<bool, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "check if asset {} exists at topoheight {}",
                asset,
                topoheight
            );
        }
        let contains =
            try_block_on(self.is_asset_registered_at_maximum_topoheight(asset, topoheight))??;
        Ok(contains)
    }

    fn account_exists(
        &self,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<bool, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "check if account {} exists at topoheight {}",
                key.as_address(self.is_mainnet()),
                topoheight
            );
        }

        let contains = try_block_on(self.is_account_registered_for_topoheight(key, topoheight))??;
        Ok(contains)
    }

    // Load the asset data from the storage
    fn load_asset_data(
        &self,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, AssetData)>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "load asset data for asset {} at topoheight {}",
                asset,
                topoheight
            );
        }
        let res = try_block_on(self.get_asset_at_maximum_topoheight(asset, topoheight))??;
        Ok(res.map(|(topo, v)| (topo, v.take())))
    }

    fn load_asset_supply(
        &self,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "load asset supply for asset {} at topoheight {}",
                asset,
                topoheight
            );
        }
        let res = try_block_on(self.get_asset_supply_at_maximum_topoheight(asset, topoheight))??;
        Ok(res.map(|(topoheight, supply)| (topoheight, supply.take())))
    }

    fn get_account_balance_for_asset(
        &self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get account {} balance for asset {} at topoheight {}",
                key.as_address(self.is_mainnet()),
                asset,
                topoheight
            );
        }
        let res = try_block_on(self.get_balance_at_maximum_topoheight(key, asset, topoheight))??;
        Ok(res.map(|(topoheight, balance)| (topoheight, balance.get_balance())))
    }

    fn load_contract_module(
        &self,
        contract: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<Vec<u8>>, anyhow::Error> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "load contract Module bytecode for contract {} at topoheight {}",
                contract,
                topoheight
            );
        }

        // Get the VersionedContract from storage using the daemon's async ContractProvider
        use crate::core::storage::ContractProvider as DaemonContractProvider;
        let versioned_contract_opt =
            try_block_on(self.get_contract_at_maximum_topoheight_for(contract, topoheight))??;

        let Some((found_topo, versioned)) = versioned_contract_opt else {
            return Ok(None);
        };

        // Extract Module from VersionedContract
        // VersionedContract = Versioned<Option<Cow<'a, Module>>>
        let module_option_cow = versioned.get();

        let module_cow = module_option_cow.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Contract {} at topoheight {} has been deleted",
                contract,
                found_topo
            )
        })?;

        let module = module_cow.as_ref();

        // Get bytecode from Module
        let bytecode_opt = module.get_bytecode();

        let bytecode = bytecode_opt.ok_or_else(|| {
            anyhow::anyhow!(
                "Contract {} at topoheight {} has no bytecode",
                contract,
                found_topo
            )
        })?;

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Loaded contract Module {} at topoheight {}: {} bytes",
                contract,
                found_topo,
                bytecode.len()
            );
        }

        Ok(Some(bytecode.to_vec()))
    }
}
