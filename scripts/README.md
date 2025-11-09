# TOS Scripts Directory

This directory contains utility scripts for the TOS blockchain project.

## Available Scripts

### check_migration_progress.sh

**Purpose**: Track RocksDB migration progress from SledStorage

**Usage**:
```bash
# Run from project root
cd ~/tos-network/tos
./scripts/check_migration_progress.sh

# Or with absolute path
~/tos-network/tos/scripts/check_migration_progress.sh
```

**Features**:
- Counts total ignored tests (need migration)
- Counts migrated tests (RocksDB-based)
- Categorizes tests by type
- Shows progress percentage
- Estimates time savings
- Provides migration priorities
- Shows next steps

**Output Example**:
```
╔════════════════════════════════════════════════════════════════════╗
║  TOS RocksDB Migration Progress Tracker                          ║
╚════════════════════════════════════════════════════════════════════╝

Progress Summary:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Total ignored tests:      64
  Migrated tests (RocksDB): 4
  Remaining to migrate:     64
  Progress:                 6%

[... detailed analysis ...]
```

**Related Documentation**:
- `../MIGRATION_TRACKING_SYSTEM.md` - Complete tracking system overview
- `../MIGRATION_PROGRESS.md` - Current migration status
- `../ROCKSDB_MIGRATION_SUMMARY.md` - Technical guide
- `../MIGRATION_QUICK_REFERENCE.md` - Quick reference

---

## Script Requirements

### check_migration_progress.sh

**Dependencies**:
- bash (any modern version)
- grep
- wc
- find
- sort
- uniq

**Tested On**:
- macOS (Darwin 25.0.0)
- Linux (should work on any standard Linux distribution)

**Performance**:
- Execution time: <1 second
- No external dependencies
- Safe to run frequently

---

## Adding New Scripts

When adding a new script to this directory:

1. **Make it executable**: `chmod +x script_name.sh`
2. **Add shebang**: Start with `#!/bin/bash`
3. **Add header comments**: Describe purpose and usage
4. **Update this README**: Document the new script
5. **Follow TOS coding standards**: See `../CLAUDE.md`
6. **Test thoroughly**: Verify on macOS and Linux if possible

---

## Maintenance

**Script Owner**: TOS Development Team
**Last Updated**: 2025-10-30
**Review Frequency**: Monthly or as needed

---

## Support

For issues or questions about scripts:
1. Check script header comments for usage
2. Read related documentation files
3. Consult TOS development team

---

**Related Files**:
- `~/tos-network/tos/MIGRATION_TRACKING_SYSTEM.md`
- `~/tos-network/tos/MIGRATION_PROGRESS.md`
- `~/tos-network/tos/ROCKSDB_MIGRATION_SUMMARY.md`
- `~/tos-network/tos/MIGRATION_QUICK_REFERENCE.md`
