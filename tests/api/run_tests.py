#!/usr/bin/env python3
"""
TOS API Test Runner

Usage:
    python run_tests.py                    # Run all tests
    python run_tests.py --daemon           # Run daemon tests only
    python run_tests.py --tip2             # Run TIP-2 related tests only
    python run_tests.py --performance      # Run performance tests
    python run_tests.py -v                 # Verbose output
    python run_tests.py --help             # Show help
"""

import sys
import argparse
from pathlib import Path
import subprocess

# Add current directory to path
sys.path.insert(0, str(Path(__file__).parent))

from config import TestConfig


def run_pytest(args):
    """Run pytest with given arguments"""
    cmd = ["pytest"] + args

    print("=" * 70)
    print("TOS API Test Runner")
    print("=" * 70)
    TestConfig.print_config()
    print()
    print(f"Command: {' '.join(cmd)}")
    print("=" * 70)
    print()

    # Run pytest
    result = subprocess.run(cmd, cwd=Path(__file__).parent)
    return result.returncode


def main():
    parser = argparse.ArgumentParser(
        description="TOS API Test Runner",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    # Test selection
    parser.add_argument(
        "--daemon",
        action="store_true",
        help="Run daemon API tests only",
    )
    parser.add_argument(
        "--ai-mining",
        action="store_true",
        help="Run AI mining API tests only",
    )
    parser.add_argument(
        "--integration",
        action="store_true",
        help="Run integration tests only",
    )
    parser.add_argument(
        "--performance",
        action="store_true",
        help="Run performance tests",
    )

    # Test markers
    parser.add_argument(
        "--tip2",
        action="store_true",
        help="Run TIP-2 related tests only",
    )
    parser.add_argument(
        "--slow",
        action="store_true",
        help="Include slow tests",
    )

    # Output options
    parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Verbose output",
    )
    parser.add_argument(
        "-s",
        action="store_true",
        help="Show print statements",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Enable debug mode",
    )

    # Coverage
    parser.add_argument(
        "--cov",
        action="store_true",
        help="Generate coverage report",
    )

    # Parallel execution
    parser.add_argument(
        "-n",
        type=int,
        metavar="NUM",
        help="Run tests in parallel (pytest-xdist)",
    )

    # Additional pytest args
    parser.add_argument(
        "pytest_args",
        nargs="*",
        help="Additional arguments to pass to pytest",
    )

    args = parser.parse_args()

    # Build pytest arguments
    pytest_args = []

    # Test selection
    if args.daemon:
        pytest_args.append("daemon/")
    elif args.ai_mining:
        pytest_args.append("ai_mining/")
    elif args.integration:
        pytest_args.append("integration/")
    elif args.performance:
        pytest_args.append("performance/")

    # Markers
    marker_exprs = []
    if args.tip2:
        marker_exprs.append("tip2")
    if args.performance:
        marker_exprs.append("performance")

    if marker_exprs:
        pytest_args.extend(["-m", " or ".join(marker_exprs)])

    if args.slow:
        pytest_args.append("--run-slow")
    else:
        pytest_args.extend(["-m", "not slow"])

    # Output options
    if args.verbose:
        pytest_args.append("-v")
    if args.s:
        pytest_args.append("-s")

    # Debug mode
    if args.debug:
        import os
        os.environ["TOS_DEBUG"] = "1"

    # Coverage
    if args.cov:
        pytest_args.extend(["--cov=lib", "--cov-report=html", "--cov-report=term"])

    # Parallel execution
    if args.n:
        pytest_args.extend(["-n", str(args.n)])

    # Additional args
    pytest_args.extend(args.pytest_args)

    # Run tests
    exit_code = run_pytest(pytest_args)

    # Print summary
    print()
    print("=" * 70)
    if exit_code == 0:
        print("✓ All tests passed!")
    else:
        print(f"✗ Tests failed with exit code {exit_code}")
    print("=" * 70)

    sys.exit(exit_code)


if __name__ == "__main__":
    main()
