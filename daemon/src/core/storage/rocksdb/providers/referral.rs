// ReferralProvider implementation for RocksDB storage

use crate::core::{
    error::BlockchainError,
    storage::{
        providers::NetworkProvider,
        rocksdb::{Column, RocksStorage},
        ReferralProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    referral::{
        DirectReferralsResult, DistributionResult, ReferralRecord, ReferralRewardRatios,
        TeamVolumeRecord, UplineResult, ZoneVolumesResult, MAX_UPLINE_LEVELS,
    },
};

/// Page size for storing direct referrals
const DIRECT_REFERRALS_PAGE_SIZE: u32 = 1000;

#[async_trait]
impl ReferralProvider for RocksStorage {
    async fn has_referrer(&self, user: &PublicKey) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "checking if user {} has referrer",
                user.as_address(self.is_mainnet())
            );
        }
        // Check if user has actually bound a referrer (not just has a stub record)
        let record: Option<ReferralRecord> =
            self.load_optional_from_disk(Column::Referrals, user.as_bytes())?;
        Ok(record.map(|r| r.referrer.is_some()).unwrap_or(false))
    }

    async fn get_referrer(&self, user: &PublicKey) -> Result<Option<PublicKey>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting referrer for user {}",
                user.as_address(self.is_mainnet())
            );
        }
        let record: Option<ReferralRecord> =
            self.load_optional_from_disk(Column::Referrals, user.as_bytes())?;
        Ok(record.and_then(|r| r.referrer))
    }

    async fn bind_referrer(
        &mut self,
        user: &PublicKey,
        referrer: &PublicKey,
        topoheight: TopoHeight,
        tx_hash: Hash,
        timestamp: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "binding referrer {} for user {} at topoheight {}",
                referrer.as_address(self.is_mainnet()),
                user.as_address(self.is_mainnet()),
                topoheight
            );
        }

        // Check if user already has a referrer
        if self.has_referrer(user).await? {
            return Err(BlockchainError::ReferralAlreadyBound);
        }

        // Prevent self-referral
        if user == referrer {
            return Err(BlockchainError::ReferralSelfReferral);
        }

        // Check for circular reference: referrer cannot be in user's downline
        if self.is_downline(user, referrer, MAX_UPLINE_LEVELS).await? {
            return Err(BlockchainError::ReferralCircularReference);
        }

        // Create the referral record (preserve cached counts if a stub exists)
        let mut record = ReferralRecord::new(
            user.clone(),
            Some(referrer.clone()),
            topoheight,
            tx_hash,
            timestamp,
        );
        if let Some(existing) =
            self.load_optional_from_disk::<_, ReferralRecord>(Column::Referrals, user.as_bytes())?
        {
            record.direct_referrals_count = existing.direct_referrals_count;
            record.team_size = existing.team_size;
        }

        // Store the record
        self.insert_into_disk(Column::Referrals, user.as_bytes(), &record)?;

        // Add to referrer's direct referrals list
        self.add_to_direct_referrals(referrer, user).await?;

        // Update referrer's direct count (create stub record if referrer doesn't have one)
        let mut referrer_record = self
            .load_optional_from_disk::<_, ReferralRecord>(Column::Referrals, referrer.as_bytes())?
            .unwrap_or_else(|| {
                // Create stub record for referrer (no referrer of their own)
                // Use zeros for tx_hash since the referrer didn't actually bind
                ReferralRecord::new(
                    referrer.clone(),
                    None, // Unknown referrer
                    topoheight,
                    Hash::zero(),
                    timestamp,
                )
            });
        referrer_record.increment_direct_count();
        self.insert_into_disk(Column::Referrals, referrer.as_bytes(), &referrer_record)?;

        Ok(())
    }

    async fn get_referral_record(
        &self,
        user: &PublicKey,
    ) -> Result<Option<ReferralRecord>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting referral record for user {}",
                user.as_address(self.is_mainnet())
            );
        }
        self.load_optional_from_disk(Column::Referrals, user.as_bytes())
    }

    async fn get_uplines(
        &self,
        user: &PublicKey,
        levels: u8,
    ) -> Result<UplineResult, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting {} uplines for user {}",
                levels,
                user.as_address(self.is_mainnet())
            );
        }

        let levels = levels.min(MAX_UPLINE_LEVELS);
        let mut uplines = Vec::with_capacity(levels as usize);
        let mut current = user.clone();

        for _ in 0..levels {
            match self.get_referrer(&current).await? {
                Some(referrer) => {
                    uplines.push(referrer.clone());
                    current = referrer;
                }
                None => break,
            }
        }

        Ok(UplineResult::new(uplines))
    }

    async fn get_level(&self, user: &PublicKey) -> Result<u8, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting level for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        let mut level = 0u8;
        let mut current = user.clone();

        while level < MAX_UPLINE_LEVELS {
            match self.get_referrer(&current).await? {
                Some(referrer) => {
                    level = level.saturating_add(1);
                    current = referrer;
                }
                None => break,
            }
        }

        Ok(level)
    }

    async fn is_downline(
        &self,
        ancestor: &PublicKey,
        descendant: &PublicKey,
        max_depth: u8,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "checking if {} is downline of {} (max_depth: {})",
                descendant.as_address(self.is_mainnet()),
                ancestor.as_address(self.is_mainnet()),
                max_depth
            );
        }

        let mut current = descendant.clone();
        let max_depth = max_depth.min(MAX_UPLINE_LEVELS);

        for _ in 0..max_depth {
            match self.get_referrer(&current).await? {
                Some(referrer) => {
                    if &referrer == ancestor {
                        return Ok(true);
                    }
                    current = referrer;
                }
                None => break,
            }
        }

        Ok(false)
    }

    async fn get_direct_referrals(
        &self,
        user: &PublicKey,
        offset: u32,
        limit: u32,
    ) -> Result<DirectReferralsResult, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting direct referrals for user {} (offset: {}, limit: {})",
                user.as_address(self.is_mainnet()),
                offset,
                limit
            );
        }

        let total_count = self.get_direct_referrals_count(user).await?;

        if offset >= total_count {
            return Ok(DirectReferralsResult::new(vec![], total_count, offset));
        }

        let mut referrals = Vec::new();
        let start_page = offset / DIRECT_REFERRALS_PAGE_SIZE;
        let mut collected = 0u32;
        let mut skipped = 0u32;

        // Iterate through pages
        for page in start_page.. {
            let key = Self::get_direct_referrals_page_key(user, page);
            let page_data: Option<Vec<PublicKey>> =
                self.load_optional_from_disk(Column::ReferralDirects, &key)?;

            match page_data {
                Some(keys) => {
                    for key in keys {
                        if skipped < (offset % DIRECT_REFERRALS_PAGE_SIZE) {
                            skipped += 1;
                            continue;
                        }

                        referrals.push(key);
                        collected += 1;

                        if collected >= limit {
                            break;
                        }
                    }

                    if collected >= limit {
                        break;
                    }
                }
                None => break,
            }
        }

        Ok(DirectReferralsResult::new(referrals, total_count, offset))
    }

    async fn get_direct_referrals_count(&self, user: &PublicKey) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting direct referrals count for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        match self.get_referral_record(user).await? {
            Some(record) => Ok(record.direct_referrals_count),
            None => Ok(0),
        }
    }

    async fn get_team_size(
        &self,
        user: &PublicKey,
        use_cache: bool,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting team size for user {} (use_cache: {})",
                user.as_address(self.is_mainnet()),
                use_cache
            );
        }

        if use_cache {
            match self.get_referral_record(user).await? {
                Some(record) => Ok(record.team_size),
                None => Ok(0),
            }
        } else {
            // Real-time calculation - recursively count all descendants
            self.calculate_team_size(user).await
        }
    }

    async fn update_team_size_cache(
        &mut self,
        user: &PublicKey,
        size: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "updating team size cache for user {} to {}",
                user.as_address(self.is_mainnet()),
                size
            );
        }

        if let Some(mut record) =
            self.load_optional_from_disk::<_, ReferralRecord>(Column::Referrals, user.as_bytes())?
        {
            record.set_team_size(size);
            self.insert_into_disk(Column::Referrals, user.as_bytes(), &record)?;
        }

        Ok(())
    }

    async fn distribute_to_uplines(
        &mut self,
        from_user: &PublicKey,
        _asset: Hash,
        total_amount: u64,
        ratios: &ReferralRewardRatios,
    ) -> Result<DistributionResult, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "distributing {} to uplines of user {}",
                total_amount,
                from_user.as_address(self.is_mainnet())
            );
        }

        // Validate ratios
        if !ratios.is_valid() {
            return Err(BlockchainError::ReferralRatiosTooHigh);
        }

        let levels = ratios.levels();
        let uplines = self.get_uplines(from_user, levels).await?;

        let mut distributions = Vec::new();

        for (i, upline) in uplines.uplines.iter().enumerate() {
            if let Some(ratio) = ratios.get_ratio(i) {
                let amount = (total_amount as u128 * ratio as u128 / 10000) as u64;
                if amount > 0 {
                    distributions.push(tos_common::referral::RewardDistribution {
                        recipient: upline.clone(),
                        amount,
                        level: (i + 1) as u8,
                    });
                }
            }
        }

        Ok(DistributionResult::new(distributions))
    }

    async fn delete_referral_record(&mut self, user: &PublicKey) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "deleting referral record for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        // Get the record first to update referrer's count
        if let Some(record) = self.get_referral_record(user).await? {
            if let Some(referrer) = &record.referrer {
                // Remove from referrer's direct referrals
                self.remove_from_direct_referrals(referrer, user).await?;

                // Decrement referrer's direct count
                if let Some(mut referrer_record) = self
                    .load_optional_from_disk::<_, ReferralRecord>(
                        Column::Referrals,
                        referrer.as_bytes(),
                    )?
                {
                    referrer_record.decrement_direct_count();
                    self.insert_into_disk(
                        Column::Referrals,
                        referrer.as_bytes(),
                        &referrer_record,
                    )?;
                }
            }
        }

        self.remove_from_disk(Column::Referrals, user.as_bytes())
    }

    async fn add_to_direct_referrals(
        &mut self,
        referrer: &PublicKey,
        user: &PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "adding {} to direct referrals of {}",
                user.as_address(self.is_mainnet()),
                referrer.as_address(self.is_mainnet())
            );
        }

        // Get current count to determine page
        let count = self.get_direct_referrals_count(referrer).await?;
        let page = count / DIRECT_REFERRALS_PAGE_SIZE;
        let key = Self::get_direct_referrals_page_key(referrer, page);

        // Load existing page or create new
        let mut page_data: Vec<PublicKey> = self
            .load_optional_from_disk(Column::ReferralDirects, &key)?
            .unwrap_or_default();

        page_data.push(user.clone());
        self.insert_into_disk(Column::ReferralDirects, &key, &page_data)
    }

    async fn remove_from_direct_referrals(
        &mut self,
        referrer: &PublicKey,
        user: &PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "removing {} from direct referrals of {}",
                user.as_address(self.is_mainnet()),
                referrer.as_address(self.is_mainnet())
            );
        }

        // Search through pages to find and remove the user
        for page in 0.. {
            let key = Self::get_direct_referrals_page_key(referrer, page);
            let page_data: Option<Vec<PublicKey>> =
                self.load_optional_from_disk(Column::ReferralDirects, &key)?;

            match page_data {
                Some(mut keys) => {
                    if let Some(pos) = keys.iter().position(|k| k == user) {
                        keys.remove(pos);
                        if keys.is_empty() {
                            self.remove_from_disk(Column::ReferralDirects, &key)?;
                        } else {
                            self.insert_into_disk(Column::ReferralDirects, &key, &keys)?;
                        }
                        return Ok(());
                    }
                }
                None => break,
            }
        }

        Ok(())
    }

    // ===== Team Volume Operations =====

    async fn add_team_volume(
        &mut self,
        user: &PublicKey,
        asset: &Hash,
        amount: u64,
        propagate_levels: u8,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "adding team volume {} for user {} asset {} to {} levels",
                amount,
                user.as_address(self.is_mainnet()),
                asset,
                propagate_levels
            );
        }

        let levels = propagate_levels.min(MAX_UPLINE_LEVELS);
        let uplines = self.get_uplines(user, levels).await?;

        // First upline (immediate referrer): add to both direct_volume and team_volume
        if let Some(first) = uplines.uplines.first() {
            let key = Self::make_team_volume_key(first, asset);
            let mut record: TeamVolumeRecord = self
                .load_optional_from_disk(Column::TeamVolumes, &key)?
                .unwrap_or_default();
            record.add_direct_volume(amount);
            record.add_team_volume(amount);
            record.set_last_update(topoheight);
            self.insert_into_disk(Column::TeamVolumes, &key, &record)?;
        }

        // Remaining uplines (levels 2+): add to team_volume only
        for upline in uplines.uplines.iter().skip(1) {
            let key = Self::make_team_volume_key(upline, asset);
            let mut record: TeamVolumeRecord = self
                .load_optional_from_disk(Column::TeamVolumes, &key)?
                .unwrap_or_default();
            record.add_team_volume(amount);
            record.set_last_update(topoheight);
            self.insert_into_disk(Column::TeamVolumes, &key, &record)?;
        }

        Ok(())
    }

    async fn get_team_volume(
        &self,
        user: &PublicKey,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting team volume for user {} asset {}",
                user.as_address(self.is_mainnet()),
                asset
            );
        }

        let key = Self::make_team_volume_key(user, asset);
        let record: Option<TeamVolumeRecord> =
            self.load_optional_from_disk(Column::TeamVolumes, &key)?;
        Ok(record.map(|r| r.team_volume).unwrap_or(0))
    }

    async fn get_direct_volume(
        &self,
        user: &PublicKey,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting direct volume for user {} asset {}",
                user.as_address(self.is_mainnet()),
                asset
            );
        }

        let key = Self::make_team_volume_key(user, asset);
        let record: Option<TeamVolumeRecord> =
            self.load_optional_from_disk(Column::TeamVolumes, &key)?;
        Ok(record.map(|r| r.direct_volume).unwrap_or(0))
    }

    async fn get_zone_volumes(
        &self,
        user: &PublicKey,
        asset: &Hash,
        limit: u32,
    ) -> Result<ZoneVolumesResult, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting zone volumes for user {} asset {} limit {}",
                user.as_address(self.is_mainnet()),
                asset,
                limit
            );
        }

        // Get direct referrals
        let direct_result = self.get_direct_referrals(user, 0, limit).await?;
        let total_count = direct_result.total_count;

        // For each direct referral, get their team volume
        let mut zones = Vec::with_capacity(direct_result.referrals.len());
        for referral in direct_result.referrals {
            let team_vol = self.get_team_volume(&referral, asset).await?;
            zones.push((referral, team_vol));
        }

        Ok(ZoneVolumesResult::new(zones, total_count))
    }

    async fn get_team_volume_record(
        &self,
        user: &PublicKey,
        asset: &Hash,
    ) -> Result<Option<TeamVolumeRecord>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting team volume record for user {} asset {}",
                user.as_address(self.is_mainnet()),
                asset
            );
        }

        let key = Self::make_team_volume_key(user, asset);
        self.load_optional_from_disk(Column::TeamVolumes, &key)
    }
}

impl RocksStorage {
    /// Get the key for a direct referrals page
    fn get_direct_referrals_page_key(referrer: &PublicKey, page: u32) -> Vec<u8> {
        let mut key = Vec::with_capacity(36); // 32 bytes pubkey + 4 bytes page
        key.extend_from_slice(referrer.as_bytes());
        key.extend_from_slice(&page.to_be_bytes());
        key
    }

    /// Create team volume storage key: {user_pubkey (32 bytes)}{asset_hash (32 bytes)}
    fn make_team_volume_key(user: &PublicKey, asset: &Hash) -> Vec<u8> {
        let mut key = Vec::with_capacity(64);
        key.extend_from_slice(user.as_bytes());
        key.extend_from_slice(asset.as_bytes());
        key
    }

    /// Calculate team size recursively (real-time, expensive)
    async fn calculate_team_size(&self, user: &PublicKey) -> Result<u64, BlockchainError> {
        // Add visited set to prevent infinite loops on cyclic referral graphs
        // Even though bind_referrer checks for cycles up to MAX_UPLINE_LEVELS (20),
        // a longer cycle could theoretically exist from legacy data or corruption.
        // This visited set ensures we never process the same node twice.
        let mut total = 0u64;
        let mut stack = vec![user.clone()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(user.clone());

        // Also add a maximum iteration limit for safety
        const MAX_TEAM_SIZE_ITERATIONS: u64 = 1_000_000;
        let mut iterations = 0u64;

        while let Some(current) = stack.pop() {
            // Safety limit to prevent excessive CPU usage
            iterations = iterations.saturating_add(1);
            if iterations > MAX_TEAM_SIZE_ITERATIONS {
                if log::log_enabled!(log::Level::Warn) {
                    log::warn!(
                        "calculate_team_size exceeded max iterations ({}) for user {:?}",
                        MAX_TEAM_SIZE_ITERATIONS,
                        user
                    );
                }
                break;
            }

            // Get direct referrals for current user
            let mut offset = 0;
            loop {
                let result = self
                    .get_direct_referrals(&current, offset, DIRECT_REFERRALS_PAGE_SIZE)
                    .await?;

                for referral in &result.referrals {
                    // Skip already visited nodes (cycle protection)
                    if visited.insert(referral.clone()) {
                        total = total.saturating_add(1);
                        stack.push(referral.clone());
                    }
                }

                if !result.has_more {
                    break;
                }
                offset += DIRECT_REFERRALS_PAGE_SIZE;
            }
        }

        Ok(total)
    }
}
