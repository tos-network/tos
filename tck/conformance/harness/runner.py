#!/usr/bin/env python3
"""
TOS Conformance Test Runner

Main entry point for running conformance tests across multiple client implementations.
"""

import asyncio
import glob
import logging
import os
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

import aiohttp
import click
import yaml

from comparator import ResultComparator, ComparisonResult
from config import HarnessConfig, ClientConfig
from reporter import ReportGenerator, SuiteResult, TestResult, ConformanceReport

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)


class ConformanceClient:
    """HTTP client for a single implementation."""

    def __init__(self, config: ClientConfig):
        self.config = config
        self.session: Optional[aiohttp.ClientSession] = None

    async def connect(self) -> None:
        """Initialize HTTP session."""
        timeout = aiohttp.ClientTimeout(total=self.config.timeout)
        self.session = aiohttp.ClientSession(timeout=timeout)

    async def close(self) -> None:
        """Close HTTP session."""
        if self.session:
            await self.session.close()

    async def reset_state(self) -> bool:
        """Reset client to genesis state."""
        try:
            async with self.session.post(
                f"{self.config.endpoint}/state/reset"
            ) as resp:
                data = await resp.json()
                return data.get("success", False)
        except Exception as e:
            logger.error(f"[{self.config.name}] Reset failed: {e}")
            return False

    async def load_state(self, state: Dict[str, Any]) -> Optional[str]:
        """
        Load state from JSON.

        Returns state digest on success, None on failure.
        """
        try:
            async with self.session.post(
                f"{self.config.endpoint}/state/load",
                json=state,
            ) as resp:
                data = await resp.json()
                if data.get("success"):
                    return data.get("state_digest")
                return None
        except Exception as e:
            logger.error(f"[{self.config.name}] Load state failed: {e}")
            return None

    async def get_state_digest(self) -> Optional[str]:
        """Get current state digest."""
        try:
            async with self.session.get(
                f"{self.config.endpoint}/state/digest"
            ) as resp:
                data = await resp.json()
                return data.get("state_digest")
        except Exception as e:
            logger.error(f"[{self.config.name}] Get digest failed: {e}")
            return None

    async def execute_tx(self, wire_hex: str) -> Dict[str, Any]:
        """Execute a single transaction."""
        try:
            async with self.session.post(
                f"{self.config.endpoint}/tx/execute",
                json={"wire_hex": wire_hex},
            ) as resp:
                return await resp.json()
        except Exception as e:
            logger.error(f"[{self.config.name}] Execute TX failed: {e}")
            return {"success": False, "error": str(e)}

    async def execute_block(self, block_data: Dict[str, Any]) -> Dict[str, Any]:
        """Execute a block of transactions."""
        try:
            async with self.session.post(
                f"{self.config.endpoint}/block/execute",
                json=block_data,
            ) as resp:
                return await resp.json()
        except Exception as e:
            logger.error(f"[{self.config.name}] Execute block failed: {e}")
            return {"success": False, "error": str(e)}


class ConformanceHarness:
    """Main test harness for conformance testing."""

    def __init__(self, config: HarnessConfig):
        self.config = config
        self.clients: Dict[str, ConformanceClient] = {}
        self.comparator = ResultComparator(reference_client="tos-rust")
        self.reporter = ReportGenerator(config.result_dir)

    async def setup(self) -> None:
        """Initialize all clients."""
        for name, client_config in self.config.get_enabled_clients().items():
            client = ConformanceClient(client_config)
            await client.connect()
            self.clients[name] = client
            logger.info(f"Connected to {client_config.name} at {client_config.endpoint}")

    async def teardown(self) -> None:
        """Close all client connections."""
        for client in self.clients.values():
            await client.close()

    async def reset_all(self) -> bool:
        """Reset all clients to genesis state."""
        results = await asyncio.gather(*[
            client.reset_state()
            for client in self.clients.values()
        ])
        return all(results)

    async def load_state_all(self, state: Dict[str, Any]) -> ComparisonResult:
        """Load identical state into all clients and verify digests match."""
        digests = {}
        for name, client in self.clients.items():
            digest = await client.load_state(state)
            if digest:
                digests[name] = digest
            else:
                logger.error(f"Failed to load state in {name}")

        return self.comparator.compare_state_digests(
            digests, "state_load"
        )

    async def execute_tx_all(self, wire_hex: str) -> Dict[str, Dict[str, Any]]:
        """Execute transaction on all clients."""
        tasks = {
            name: client.execute_tx(wire_hex)
            for name, client in self.clients.items()
        }

        results = {}
        for name, task in tasks.items():
            results[name] = await task

        return results

    async def run_vector(self, vector: Dict[str, Any]) -> TestResult:
        """Run a single test vector."""
        vector_name = vector.get("name", "unknown")
        start_time = time.time()

        try:
            # Reset all clients
            if not await self.reset_all():
                return TestResult(
                    vector_name=vector_name,
                    suite_name="",
                    passed=False,
                    execution_time_ms=(time.time() - start_time) * 1000,
                    error="Failed to reset clients",
                )

            # Load pre-state if specified
            if "pre_state" in vector:
                result = await self.load_state_all(vector["pre_state"])
                if result.has_divergences:
                    return TestResult(
                        vector_name=vector_name,
                        suite_name="",
                        passed=False,
                        execution_time_ms=(time.time() - start_time) * 1000,
                        comparison=result,
                        error="State load divergence",
                    )

            # Execute transaction
            tx_data = vector.get("transaction", {})
            wire_hex = tx_data.get("wire_hex", "")

            if wire_hex:
                results = await self.execute_tx_all(wire_hex)
                comparison = self.comparator.compare_results(results, vector_name)

                return TestResult(
                    vector_name=vector_name,
                    suite_name="",
                    passed=not comparison.has_divergences,
                    execution_time_ms=(time.time() - start_time) * 1000,
                    comparison=comparison,
                )

            # No transaction to execute - just a state load test
            return TestResult(
                vector_name=vector_name,
                suite_name="",
                passed=True,
                execution_time_ms=(time.time() - start_time) * 1000,
            )

        except Exception as e:
            logger.exception(f"Error running vector {vector_name}")
            return TestResult(
                vector_name=vector_name,
                suite_name="",
                passed=False,
                execution_time_ms=(time.time() - start_time) * 1000,
                error=str(e),
            )

    async def run_suite(self, suite_path: str) -> SuiteResult:
        """Run a test suite from a YAML file."""
        suite_name = Path(suite_path).stem
        logger.info(f"Running suite: {suite_name}")

        start_time = time.time()

        with open(suite_path) as f:
            suite = yaml.safe_load(f)

        vectors = suite.get("test_vectors", [])
        test_results = []

        for vector in vectors:
            result = await self.run_vector(vector)
            result.suite_name = suite_name
            test_results.append(result)

            status = "PASS" if result.passed else "FAIL"
            logger.info(f"  [{status}] {result.vector_name}")

            if not result.passed and self.config.stop_on_first_failure:
                break

        passed = sum(1 for r in test_results if r.passed)
        failed = sum(1 for r in test_results if not r.passed)

        return SuiteResult(
            suite_name=suite_name,
            total_tests=len(test_results),
            passed_tests=passed,
            failed_tests=failed,
            skipped_tests=len(vectors) - len(test_results),
            execution_time_ms=(time.time() - start_time) * 1000,
            test_results=test_results,
        )

    async def run_all(self, vector_paths: List[str]) -> ConformanceReport:
        """Run all test suites."""
        start_time = time.time()

        suite_results = []
        for path in vector_paths:
            result = await self.run_suite(path)
            suite_results.append(result)

        report = self.reporter.generate_report(
            suite_results=suite_results,
            clients=list(self.clients.keys()),
            reference_client=self.comparator.reference_client,
            execution_time_ms=(time.time() - start_time) * 1000,
        )

        return report


def find_vector_files(vector_dir: str) -> List[str]:
    """Find all vector YAML files in directory."""
    patterns = [
        os.path.join(vector_dir, "**", "*.yaml"),
        os.path.join(vector_dir, "**", "*.yml"),
    ]

    files = []
    for pattern in patterns:
        files.extend(glob.glob(pattern, recursive=True))

    return sorted(files)


@click.command()
@click.option(
    "--vectors",
    default=None,
    help="Path to vectors directory or specific YAML file",
)
@click.option(
    "--rust-endpoint",
    default=None,
    help="TOS Rust endpoint URL",
)
@click.option(
    "--c-endpoint",
    default=None,
    help="Avatar C endpoint URL",
)
@click.option(
    "--result-dir",
    default=None,
    help="Directory to write results",
)
@click.option(
    "--verbose",
    is_flag=True,
    help="Enable verbose output",
)
@click.option(
    "--stop-on-failure",
    is_flag=True,
    help="Stop on first test failure",
)
def main(
    vectors: Optional[str],
    rust_endpoint: Optional[str],
    c_endpoint: Optional[str],
    result_dir: Optional[str],
    verbose: bool,
    stop_on_failure: bool,
) -> None:
    """Run TOS conformance tests."""

    # Load config from environment, then override with CLI args
    config = HarnessConfig.from_env()

    if rust_endpoint:
        config.clients["tos-rust"].endpoint = rust_endpoint
    if c_endpoint:
        config.clients["avatar-c"].endpoint = c_endpoint
    if result_dir:
        config.result_dir = result_dir
    if verbose:
        config.verbose = True
        logging.getLogger().setLevel(logging.DEBUG)
    if stop_on_failure:
        config.stop_on_first_failure = True

    # Find vector files
    vector_dir = vectors or config.vector_dir
    if os.path.isfile(vector_dir):
        vector_files = [vector_dir]
    else:
        vector_files = find_vector_files(vector_dir)

    if not vector_files:
        logger.error(f"No vector files found in {vector_dir}")
        sys.exit(1)

    logger.info(f"Found {len(vector_files)} vector files")

    # Run tests
    async def run() -> int:
        harness = ConformanceHarness(config)

        try:
            await harness.setup()
            report = await harness.run_all(vector_files)

            # Generate reports
            harness.reporter.write_json_report(report)
            harness.reporter.write_summary(report)
            harness.reporter.print_summary(report)

            return 0 if report.total_failed == 0 else 1

        finally:
            await harness.teardown()

    exit_code = asyncio.run(run())
    sys.exit(exit_code)


if __name__ == "__main__":
    main()
