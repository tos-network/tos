"""
pytest configuration for TOS API tests

This file contains pytest fixtures and configuration that are shared
across all test modules.
"""

import pytest
import sys
from pathlib import Path

# Add lib directory to path
sys.path.insert(0, str(Path(__file__).parent))

from lib.rpc_client import TosRpcClient
from config import TestConfig


def pytest_addoption(parser):
    """Add custom command line options"""
    parser.addoption(
        "--run-slow",
        action="store_true",
        default=False,
        help="Run slow tests (performance, stress)",
    )
    parser.addoption(
        "--daemon-url",
        action="store",
        default=None,
        help="Override daemon RPC URL",
    )


def pytest_configure(config):
    """Configure pytest"""
    # Register custom markers
    config.addinivalue_line("markers", "tip2: TIP-2 GHOSTDAG implementation tests")
    config.addinivalue_line("markers", "slow: Slow tests (performance, stress)")
    config.addinivalue_line("markers", "unit: Unit-style API tests")
    config.addinivalue_line("markers", "integration: Integration tests")
    config.addinivalue_line("markers", "performance: Performance benchmarks")

    # Override daemon URL if specified
    daemon_url = config.getoption("--daemon-url")
    if daemon_url:
        TestConfig.DAEMON_RPC_URL = daemon_url


def pytest_collection_modifyitems(config, items):
    """Modify test collection"""
    # Skip slow tests by default
    if not config.getoption("--run-slow"):
        skip_slow = pytest.mark.skip(reason="Need --run-slow option to run")
        for item in items:
            if "slow" in item.keywords:
                item.add_marker(skip_slow)


@pytest.fixture(scope="session")
def daemon_url():
    """Daemon RPC URL fixture"""
    return TestConfig.DAEMON_RPC_URL


@pytest.fixture(scope="session")
def network():
    """Network name fixture"""
    return TestConfig.NETWORK


@pytest.fixture(scope="function")
def rpc_client():
    """
    Create RPC client for each test

    This fixture creates a new client for each test function.
    It also verifies the daemon is available before running the test.
    """
    client = TosRpcClient()

    # Check if daemon is available
    if not client.ping():
        pytest.skip(f"Daemon not available at {client.url}")

    yield client


@pytest.fixture(scope="session")
def rpc_client_session():
    """
    Create RPC client for the entire test session

    This fixture creates a single client shared across all tests in a session.
    Use this for tests that don't modify state.
    """
    client = TosRpcClient()

    if not client.ping():
        pytest.skip(f"Daemon not available at {client.url}")

    yield client


@pytest.fixture(scope="function")
def network_info(rpc_client):
    """Get current network info"""
    return rpc_client.get_info()


@pytest.fixture(scope="session")
def test_miner_address():
    """Test miner address fixture"""
    return TestConfig.TEST_MINER_ADDRESS


@pytest.fixture(scope="session", autouse=True)
def print_test_config(request):
    """Print test configuration at session start"""
    if request.config.option.verbose:
        print("\n")
        TestConfig.print_config()
        print()


@pytest.fixture
def block_at_topoheight(rpc_client):
    """Factory fixture to get block at specific topoheight"""
    def _get_block(topoheight: int):
        return rpc_client.get_block_at_topoheight(topoheight)
    return _get_block


@pytest.fixture
def balance_at_topoheight(rpc_client, test_miner_address):
    """Factory fixture to get balance at specific topoheight"""
    def _get_balance(topoheight: int, address: str = None):
        addr = address or test_miner_address
        return rpc_client.get_balance_at_topoheight(addr, topoheight)
    return _get_balance
