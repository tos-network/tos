#!/usr/bin/env python3
"""
TOS Log Optimization Checker

Detects log statements with format arguments that are not wrapped with
`if log::log_enabled!()` as required by TIP-6.

Usage:
    python3 scripts/check_log_optimization.py [--json] [--fix-preview]
"""

import os
import re
import sys
import json
from pathlib import Path
from dataclasses import dataclass, field
from typing import List, Dict, Tuple, Optional
from collections import defaultdict
from datetime import datetime, date

# Log levels and their priorities
LOG_LEVELS = {
    'trace': ('Trace', 1, 'critical'),    # Always disabled in production
    'debug': ('Debug', 2, 'critical'),    # Usually disabled in production
    'info': ('Info', 3, 'medium'),        # Sometimes disabled
    'warn': ('Warn', 4, 'low'),           # Usually enabled
    'error': ('Error', 5, 'low'),         # Always enabled
}

# Hot path patterns - files that are performance critical
HOT_PATH_PATTERNS = [
    'daemon/src/core/blockchain.rs',
    'daemon/src/core/state/',
    'daemon/src/core/mempool.rs',
    'daemon/src/p2p/mod.rs',
    'daemon/src/p2p/connection.rs',
    'daemon/src/p2p/chain_sync/',
    'daemon/src/p2p/peer_list/',
    'daemon/src/core/storage/',
    'common/src/transaction/verify/',
]

# Patterns to skip (test code, etc.)
SKIP_PATTERNS = [
    '/tests/',
    '/test_',
    '_test.rs',
    '/examples/',
    '/benches/',
]


@dataclass
class UnoptimizedLog:
    """Represents an unoptimized log statement."""
    file_path: str
    line_number: int
    log_level: str
    content: str
    is_hot_path: bool
    is_test: bool

    def to_dict(self) -> dict:
        return {
            'file': self.file_path,
            'line': self.line_number,
            'level': self.log_level,
            'content': self.content.strip(),
            'hot_path': self.is_hot_path,
            'test': self.is_test,
        }


@dataclass
class FileStats:
    """Statistics for a single file."""
    path: str
    logs: List[UnoptimizedLog] = field(default_factory=list)

    @property
    def count(self) -> int:
        return len(self.logs)

    @property
    def by_level(self) -> Dict[str, int]:
        result = defaultdict(int)
        for log in self.logs:
            result[log.log_level] += 1
        return dict(result)


class LogOptimizationChecker:
    """Checks for unoptimized log statements in Rust code."""

    def __init__(self, root_dir: str):
        self.root_dir = Path(root_dir)
        self.files: Dict[str, FileStats] = {}
        self.total_logs = 0

    def is_hot_path(self, file_path: str) -> bool:
        """Check if file is in a hot path."""
        for pattern in HOT_PATH_PATTERNS:
            if pattern in file_path:
                return True
        return False

    def is_test_file(self, file_path: str) -> bool:
        """Check if file is test code."""
        for pattern in SKIP_PATTERNS:
            if pattern in file_path:
                return True
        return False

    def check_file(self, file_path: Path) -> Optional[FileStats]:
        """Check a single file for unoptimized logs."""
        rel_path = str(file_path.relative_to(self.root_dir))
        is_hot = self.is_hot_path(rel_path)
        is_test = self.is_test_file(rel_path)

        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                lines = f.readlines()
        except Exception as e:
            print(f"Warning: Could not read {file_path}: {e}", file=sys.stderr)
            return None

        stats = FileStats(path=rel_path)

        # Track if we're inside a log_enabled block
        in_log_enabled_block = False
        brace_depth = 0
        log_enabled_level = None

        # Pattern to match log macros with format arguments
        log_pattern = re.compile(r'^\s*(trace|debug|info|warn|error)!\s*\(')
        # Pattern to detect format arguments
        format_arg_pattern = re.compile(r'\{[^}]*\}')
        # Pattern to detect log_enabled (matches both log::log_enabled! and log_enabled!)
        log_enabled_pattern = re.compile(r'if\s+(log::)?log_enabled!\s*\(\s*log::Level::(\w+)\s*\)')

        i = 0
        while i < len(lines):
            line = lines[i]
            line_num = i + 1

            # Check for log_enabled block start
            enabled_match = log_enabled_pattern.search(line)
            if enabled_match:
                in_log_enabled_block = True
                log_enabled_level = enabled_match.group(2).lower()  # group(2) because group(1) is optional 'log::'
                # Count braces to track block
                brace_depth = line.count('{') - line.count('}')
                i += 1
                continue

            # Track brace depth if in log_enabled block
            if in_log_enabled_block:
                brace_depth += line.count('{') - line.count('}')
                if brace_depth <= 0:
                    in_log_enabled_block = False
                    log_enabled_level = None

            # Check for log macro
            log_match = log_pattern.match(line)
            if log_match:
                level = log_match.group(1)

                # Get the full log statement (may span multiple lines)
                full_statement = line
                paren_depth = line.count('(') - line.count(')')
                j = i + 1
                while paren_depth > 0 and j < len(lines):
                    full_statement += lines[j]
                    paren_depth += lines[j].count('(') - lines[j].count(')')
                    j += 1

                # Check if it has format arguments
                if format_arg_pattern.search(full_statement):
                    # Check if it's already wrapped
                    is_wrapped = (in_log_enabled_block and
                                  log_enabled_level and
                                  log_enabled_level == level)

                    if not is_wrapped:
                        log_entry = UnoptimizedLog(
                            file_path=rel_path,
                            line_number=line_num,
                            log_level=level,
                            content=full_statement[:200],  # Truncate long statements
                            is_hot_path=is_hot,
                            is_test=is_test,
                        )
                        stats.logs.append(log_entry)

            i += 1

        if stats.logs:
            return stats
        return None

    def scan_directory(self) -> None:
        """Scan all Rust files in the directory."""
        rust_files = list(self.root_dir.rglob('*.rs'))

        for file_path in rust_files:
            # Skip target directory
            if '/target/' in str(file_path):
                continue

            stats = self.check_file(file_path)
            if stats:
                self.files[stats.path] = stats
                self.total_logs += stats.count

    def get_summary(self) -> dict:
        """Generate summary statistics."""
        by_level = defaultdict(int)
        by_module = defaultdict(int)
        hot_path_count = 0
        test_count = 0

        for stats in self.files.values():
            for log in stats.logs:
                by_level[log.log_level] += 1

                # Extract module
                parts = log.file_path.split('/')
                if len(parts) >= 2:
                    module = '/'.join(parts[:2])
                else:
                    module = parts[0]
                by_module[module] += 1

                if log.is_hot_path:
                    hot_path_count += 1
                if log.is_test:
                    test_count += 1

        return {
            'total': self.total_logs,
            'files': len(self.files),
            'by_level': dict(by_level),
            'by_module': dict(by_module),
            'hot_path': hot_path_count,
            'test': test_count,
            'production': self.total_logs - test_count,
        }

    def generate_markdown_report(self) -> str:
        """Generate a detailed Markdown report."""
        summary = self.get_summary()

        lines = []
        lines.append("# TOS Log Optimization Check Report")
        lines.append("")
        lines.append(f"**Date**: {date.today().isoformat()}")
        lines.append(f"**Reference**: TIP-6 Zero-Overhead Logging Performance Optimization")
        lines.append(f"**Status**: {'⚠️ **' + str(summary['total']) + ' unoptimized log statements found**' if summary['total'] > 0 else '✅ All logs optimized'}")
        lines.append("")
        lines.append("---")
        lines.append("")

        # Executive Summary
        lines.append("## Executive Summary")
        lines.append("")
        lines.append("| Metric | Value |")
        lines.append("|--------|-------|")
        lines.append(f"| **Total Unoptimized Logs** | {summary['total']} |")
        lines.append(f"| **Files Affected** | {summary['files']} |")
        lines.append(f"| **Hot Path Logs** | {summary['hot_path']} |")
        lines.append(f"| **Test Code (can skip)** | {summary['test']} |")
        lines.append(f"| **Production Code** | {summary['production']} |")
        lines.append("")

        # By Log Level
        lines.append("### By Log Level (Priority)")
        lines.append("")
        lines.append("| Level | Count | Priority | Reason |")
        lines.append("|-------|-------|----------|--------|")
        for level in ['trace', 'debug', 'info', 'warn', 'error']:
            count = summary['by_level'].get(level, 0)
            _, _, priority = LOG_LEVELS[level]
            priority_icon = {'critical': '🔴 **Critical**', 'medium': '🟡 Medium', 'low': '🟢 Low'}[priority]
            reasons = {
                'trace': 'Always disabled in production',
                'debug': 'Usually disabled in production',
                'info': 'Sometimes disabled',
                'warn': 'Usually enabled',
                'error': 'Always enabled',
            }
            lines.append(f"| `{level}!` | {count} | {priority_icon} | {reasons[level]} |")
        lines.append("")

        # By Module
        lines.append("---")
        lines.append("")
        lines.append("## By Module")
        lines.append("")
        lines.append("| Module | Count | Hot Path? | Priority |")
        lines.append("|--------|-------|-----------|----------|")

        sorted_modules = sorted(summary['by_module'].items(), key=lambda x: -x[1])
        for module, count in sorted_modules:
            is_hot = any(hp in module for hp in ['daemon/src/core', 'daemon/src/p2p', 'common/src/transaction'])
            hot_str = "✅ Yes" if is_hot else "No"
            priority = "🔴 Critical" if is_hot else "🟢 Low"
            lines.append(f"| `{module}` | {count} | {hot_str} | {priority} |")
        lines.append("")

        # Critical Hot Path Files
        lines.append("---")
        lines.append("")
        lines.append("## Critical Hot Path Files")
        lines.append("")
        lines.append("These files are in consensus/network hot paths and should be prioritized:")
        lines.append("")

        # Sort files by count, filter hot paths
        hot_files = [(path, stats) for path, stats in self.files.items()
                     if stats.logs and any(log.is_hot_path for log in stats.logs)]
        hot_files.sort(key=lambda x: -x[1].count)

        if hot_files:
            lines.append("| File | Unoptimized | trace | debug | info | warn | error |")
            lines.append("|------|-------------|-------|-------|------|------|-------|")
            for path, stats in hot_files[:20]:
                by_level = stats.by_level
                lines.append(f"| `{path}` | {stats.count} | {by_level.get('trace', 0)} | {by_level.get('debug', 0)} | {by_level.get('info', 0)} | {by_level.get('warn', 0)} | {by_level.get('error', 0)} |")
            lines.append("")

        # All Files (Top 40)
        lines.append("---")
        lines.append("")
        lines.append("## All Files (Sorted by Count)")
        lines.append("")
        lines.append("```")

        sorted_files = sorted(self.files.items(), key=lambda x: -x[1].count)
        for path, stats in sorted_files[:50]:
            lines.append(f"{path:70} {stats.count:4}")
        lines.append("```")
        lines.append("")

        # Detailed Locations
        lines.append("---")
        lines.append("")
        lines.append("## Detailed Locations (Hot Path Files)")
        lines.append("")
        lines.append("### Format: `file:line` - level - content")
        lines.append("")

        for path, stats in hot_files[:10]:
            lines.append(f"### `{path}` ({stats.count} logs)")
            lines.append("")
            lines.append("```rust")
            for log in stats.logs[:15]:  # Limit to 15 per file
                content = log.content.strip().replace('\n', ' ')[:100]
                lines.append(f"// {path}:{log.line_number}")
                lines.append(f"// {log.log_level}! - {content}")
            if len(stats.logs) > 15:
                lines.append(f"// ... and {len(stats.logs) - 15} more")
            lines.append("```")
            lines.append("")

        # Fix Examples
        lines.append("---")
        lines.append("")
        lines.append("## How to Fix")
        lines.append("")
        lines.append("### Pattern")
        lines.append("")
        lines.append("```rust")
        lines.append("// BEFORE (unoptimized):")
        lines.append('trace!("Processing block {} at height {}", hash, height);')
        lines.append("")
        lines.append("// AFTER (optimized per TIP-6):")
        lines.append("if log::log_enabled!(log::Level::Trace) {")
        lines.append('    trace!("Processing block {} at height {}", hash, height);')
        lines.append("}")
        lines.append("```")
        lines.append("")

        # Commands
        lines.append("---")
        lines.append("")
        lines.append("## Verification Commands")
        lines.append("")
        lines.append("```bash")
        lines.append("# Run this checker")
        lines.append("python3 scripts/check_log_optimization.py")
        lines.append("")
        lines.append("# Find all unoptimized logs")
        lines.append("rg '^\\s*(error|warn|info|debug|trace)!\\([^)]*\\{' --type rust -c | sort -t: -k2 -rn")
        lines.append("")
        lines.append("# Count already optimized")
        lines.append("rg 'if log::log_enabled!\\(log::Level::' --type rust | wc -l")
        lines.append("```")
        lines.append("")

        # Action Items
        lines.append("---")
        lines.append("")
        lines.append("## Action Items")
        lines.append("")

        for path, stats in sorted_files[:15]:
            test_marker = " (test - skip)" if all(log.is_test for log in stats.logs) else ""
            lines.append(f"- [ ] `{path}` ({stats.count} logs){test_marker}")
        lines.append("")

        lines.append("---")
        lines.append("")
        lines.append(f"**Report Generated**: {date.today().isoformat()} by `check_log_optimization.py`")
        lines.append("")

        return '\n'.join(lines)

    def generate_json_report(self) -> str:
        """Generate JSON report."""
        summary = self.get_summary()

        all_logs = []
        for stats in self.files.values():
            for log in stats.logs:
                all_logs.append(log.to_dict())

        report = {
            'summary': summary,
            'files': {path: {'count': stats.count, 'by_level': stats.by_level}
                      for path, stats in self.files.items()},
            'logs': all_logs,
        }

        return json.dumps(report, indent=2)


def main():
    import argparse

    parser = argparse.ArgumentParser(description='Check for unoptimized log statements')
    parser.add_argument('--json', action='store_true', help='Output JSON format')
    parser.add_argument('--dir', default='.', help='Directory to scan')
    args = parser.parse_args()

    checker = LogOptimizationChecker(args.dir)
    checker.scan_directory()

    if args.json:
        print(checker.generate_json_report())
    else:
        print(checker.generate_markdown_report())


if __name__ == '__main__':
    main()
