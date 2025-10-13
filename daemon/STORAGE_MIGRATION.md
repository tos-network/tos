# Storage Migration Guide

## Overview

This document describes the storage migration system in TOS daemon and provides guidance for database administrators and developers.

## Storage Versions

### Version 2 (Legacy)
- Stored `cumulative_difficulty` separately in both Sled and RocksDB
- **Sled**: Dedicated `cumulative_difficulty` tree
- **RocksDB**: `cumulative_difficulty` field in `BlockDifficulty` struct
- Used for chain comparison and P2P synchronization

### Version 3 (Current - TIP-2 Phase 2)
- **Removed** `cumulative_difficulty` storage
- Uses GHOSTDAG `blue_work` instead for equivalent functionality
- Reduces storage overhead and simplifies codebase
- Cumulative difficulty calculated on-demand from `blue_work` when needed (e.g., RPC)

## What Changed in V3

### Sled Storage
1. **Dropped**: `cumulative_difficulty` tree completely removed
2. **Cleared**: In-memory cache for cumulative difficulty
3. **Result**: Immediate disk space savings

### RocksDB Storage
1. **Modified**: `BlockDifficulty` struct no longer contains `cumulative_difficulty` field
2. **Backward Compatible**: Can still read V2 format data (migration on read)
3. **Forward Only**: New blocks written in V3 format without cumulative difficulty
4. **Result**: Gradual disk space savings as data turns over

### Code Changes
1. **DifficultyProvider** trait: Removed `get_cumulative_difficulty_for_block_hash()` method
2. **Block Storage**: `save_block()` no longer accepts `cumulative_difficulty` parameter
3. **RPC Layer**: Computes cumulative difficulty on-the-fly from blue_work for backward compatibility
4. **P2P Layer**: Uses blue_work directly for chain comparison

## Data Loss Analysis

### What is Lost
- **Stored cumulative difficulty values** are deleted during migration

### Why This is Safe
1. **Redundant Data**: Cumulative difficulty can be recalculated from GHOSTDAG blue_work
2. **No Functional Impact**: All functionality moved to use blue_work directly
3. **RPC Compatibility**: RPC endpoints still return cumulative_difficulty by computing it from blue_work
4. **P2P Compatibility**: Chain comparison now uses blue_work which is more accurate

### Recovery
If you need to recover cumulative difficulty for a specific block after migration:
```rust
// Old way (V2)
let cumulative_diff = storage.get_cumulative_difficulty_for_block_hash(&hash).await?;

// New way (V3)
let blue_work = ghostdag_provider.get_ghostdag_blue_work(&hash).await?;
let cumulative_diff = blue_work; // They're equivalent in TIP-2 Phase 2
```

## Migration Process

### Automatic Migration
The daemon automatically detects when migration is needed and performs it on startup:

1. **Detection**: Checks storage version on startup
2. **Warning**: Displays migration information and waits 5 seconds
3. **Execution**: Runs migration scripts
4. **Verification**: Updates version marker
5. **Completion**: Proceeds with normal startup

### Manual Control
To skip automatic migration (advanced users only):
```bash
# Not implemented yet - migrations are automatic
# Future: --skip-migration flag
```

## Backup Recommendations

### Before Migration

**IMPORTANT**: Always backup your database before migration!

#### Sled Backup
```bash
# Stop the daemon first
pkill tos-daemon

# Copy the entire database directory
cp -r ~/.tos/mainnet ~/.tos/mainnet.backup.$(date +%Y%m%d)

# Or create a tarball
tar -czf tos-db-backup-$(date +%Y%m%d).tar.gz ~/.tos/mainnet
```

#### RocksDB Backup
```bash
# Stop the daemon first
pkill tos-daemon

# Copy the entire database directory
cp -r ~/.tos/mainnet ~/.tos/mainnet.backup.$(date +%Y%m%d)

# Or create a tarball
tar -czf tos-db-backup-$(date +%Y%m%d).tar.gz ~/.tos/mainnet
```

### Backup Verification
```bash
# Check backup size
du -sh ~/.tos/mainnet.backup.*

# Verify backup integrity (for tarball)
tar -tzf tos-db-backup-*.tar.gz > /dev/null && echo "Backup OK"
```

## Rollback Process

If you need to rollback after migration:

### Option 1: Restore from Backup
```bash
# Stop daemon
pkill tos-daemon

# Remove current database
rm -rf ~/.tos/mainnet

# Restore from backup
cp -r ~/.tos/mainnet.backup.YYYYMMDD ~/.tos/mainnet
# OR
tar -xzf tos-db-backup-YYYYMMDD.tar.gz -C ~/.tos/

# Start daemon with older version
./tos-daemon
```

### Option 2: Resync from Network
```bash
# Stop daemon
pkill tos-daemon

# Remove database
rm -rf ~/.tos/mainnet

# Start daemon - will resync from network
./tos-daemon
```

**Note**: Resync can take several hours depending on chain size and network speed.

## Migration Timeline

### For Node Operators
- **Before Migration**: V2 nodes continue working normally
- **During Migration**: 5-second warning, then automatic migration (typically < 1 minute)
- **After Migration**: Node operates on V3 storage

### For Developers
- **Code Update**: Update code to use `blue_work` instead of `cumulative_difficulty`
- **Testing**: Verify RPC endpoints still work correctly
- **Deployment**: Migration happens automatically on first run

## Troubleshooting

### Migration Fails
```
Error: Failed to drop cumulative_difficulty tree
```
**Solution**: Check disk space and permissions. Tree might already be dropped.

### Disk Space Issues
```
Error: No space left on device
```
**Solution**: Free up disk space before migration. Migration itself reduces storage but needs temporary space.

### Version Mismatch
```
Error: Storage version mismatch
```
**Solution**: Don't mix V2 and V3 daemon versions on same database.

### Performance After Migration

#### Sled
- Expect immediate performance improvement (less data to manage)
- Disk space freed immediately

#### RocksDB
- Performance neutral initially
- Gradual improvement as V3 format propagates
- Use `compact_block_difficulty_to_v3()` for immediate compaction (optional)

## Frequently Asked Questions

### Q: Can I skip this migration?
**A**: No, migration is required for TIP-2 Phase 2 compatibility.

### Q: Will this break my RPC client?
**A**: No, RPC endpoints compute cumulative_difficulty on-the-fly for backward compatibility.

### Q: How long does migration take?
**A**: Typically under 1 minute for most databases. Sled is instant, RocksDB is version marker update only.

### Q: Can I run V2 and V3 daemons on the same database?
**A**: No, once migrated to V3, you cannot use V2 daemon without restoring from backup.

### Q: What if migration is interrupted (power failure, kill -9)?
**A**:
- **Sled**: Tree drop is atomic - database is consistent
- **RocksDB**: Version marker update is last step - safe to retry
- In worst case, restore from backup and retry

### Q: Does this affect my wallet?
**A**: No, wallet data is not affected by this migration.

### Q: How much disk space will I save?
**A**: Depends on chain size. Roughly 16 bytes per block (for cumulative_difficulty) plus tree overhead.

## Developer Notes

### Adding New Migrations

To add a new migration (e.g., V3 -> V4):

1. **Update version constants** in `daemon/src/core/storage/versioning.rs`:
```rust
pub const STORAGE_VERSION_CURRENT: u32 = 4;
pub const STORAGE_VERSION_V4: u32 = 4;
```

2. **Add migration description**:
```rust
fn get_migration_description(from: u32, to: u32) -> String {
    match (from, to) {
        (3, 4) => "Your migration description here".to_string(),
        // ... existing cases
    }
}
```

3. **Implement migration** in Sled:
```rust
impl SledStorage {
    pub async fn migrate_v3_to_v4(&mut self) -> Result<(), BlockchainError> {
        // Your migration logic
        self.set_storage_version(4).await?;
        Ok(())
    }
}
```

4. **Implement migration** in RocksDB:
```rust
impl RocksStorage {
    pub async fn migrate_v3_to_v4(&mut self) -> Result<(), BlockchainError> {
        // Your migration logic
        self.set_storage_version(4).await?;
        Ok(())
    }
}
```

5. **Update migration trigger**:
```rust
match target_version {
    3 => self.migrate_v2_to_v3().await?,
    4 => self.migrate_v3_to_v4().await?,
    // ... existing cases
}
```

6. **Add tests** in `migration.rs` test modules

### Testing Migrations

```rust
#[tokio::test]
async fn test_v3_to_v4_migration() {
    let storage = create_test_storage();
    storage.set_storage_version(3).await.unwrap();

    assert!(storage.needs_migration().await.unwrap());
    storage.migrate_to_latest().await.unwrap();

    let version = storage.get_storage_version().await.unwrap();
    assert_eq!(version, 4);
}
```

## Support

For issues or questions:
- GitHub Issues: https://github.com/TOS-NETWORK/tos/issues
- Discord: [TOS Community]
- Documentation: https://docs.tos.network

## References

- [TIP-2 Phase 2 Specification](../CLAUDE.md)
- [GHOSTDAG Implementation](../consensus/)
- [Storage Architecture](./src/core/storage/README.md)
