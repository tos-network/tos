"""
TOS API Test Configuration

Environment variables:
    TOS_DAEMON_RPC_URL: Daemon RPC endpoint (default: http://127.0.0.1:8080/json_rpc)
    TOS_NETWORK: Network type (default: devnet)
    TOS_RPC_TIMEOUT: RPC timeout in milliseconds (default: 30000)
    TOS_TEST_MINER_ADDRESS: Test miner address for mining tests
    TOS_DEBUG: Enable debug logging (default: false)
"""

import os
from typing import Optional
from pathlib import Path


class TestConfig:
    """Test configuration with environment variable overrides"""

    # RPC Endpoints
    DAEMON_RPC_URL: str = os.getenv(
        "TOS_DAEMON_RPC_URL",
        "http://127.0.0.1:8080/json_rpc"
    )

    # Network Configuration
    NETWORK: str = os.getenv("TOS_NETWORK", "devnet")

    # Timeout Settings (milliseconds)
    RPC_TIMEOUT: int = int(os.getenv("TOS_RPC_TIMEOUT", "30000"))
    BLOCK_TIMEOUT: int = int(os.getenv("TOS_BLOCK_TIMEOUT", "60000"))
    SYNC_TIMEOUT: int = int(os.getenv("TOS_SYNC_TIMEOUT", "300000"))

    # Test Parameters
    TEST_MINER_ADDRESS: str = os.getenv(
        "TOS_TEST_MINER_ADDRESS",
        "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
    )

    # Debugging
    DEBUG: bool = os.getenv("TOS_DEBUG", "").lower() in ("1", "true", "yes")
    VERBOSE: bool = os.getenv("TOS_VERBOSE", "").lower() in ("1", "true", "yes")

    # Test Data
    TEST_DATA_DIR: Path = Path(__file__).parent / "test_data"
    FIXTURES_DIR: Path = Path(__file__).parent / "fixtures"

    # Performance Thresholds (milliseconds)
    PERF_GET_INFO_MAX_MS: int = 100
    PERF_GET_BLOCK_MAX_MS: int = 200
    PERF_GET_BALANCE_MAX_MS: int = 300
    PERF_SUBMIT_TX_MAX_MS: int = 500

    # TIP-2 Specific
    TIP2_ACTIVATION_HEIGHT: Optional[int] = None  # Auto-detect from daemon

    # Asset Identifiers
    # Native TOS asset is the zero hash (Hash::zero() in Rust code)
    TOS_ASSET: str = "0000000000000000000000000000000000000000000000000000000000000000"
    # Same constant with clear naming
    TOS_ASSET_ZERO_HASH: str = "0000000000000000000000000000000000000000000000000000000000000000"

    @classmethod
    def validate(cls) -> None:
        """Validate configuration"""
        if not cls.DAEMON_RPC_URL:
            raise ValueError("TOS_DAEMON_RPC_URL must be set")

        if cls.NETWORK not in ["mainnet", "testnet", "devnet"]:
            raise ValueError(f"Invalid network: {cls.NETWORK}")

    @classmethod
    def print_config(cls) -> None:
        """Print current configuration"""
        print("=" * 60)
        print("TOS API Test Configuration")
        print("=" * 60)
        print(f"Daemon RPC URL:     {cls.DAEMON_RPC_URL}")
        print(f"Network:            {cls.NETWORK}")
        print(f"RPC Timeout:        {cls.RPC_TIMEOUT}ms")
        print(f"Test Miner Address: {cls.TEST_MINER_ADDRESS[:20]}...")
        print(f"Debug Mode:         {cls.DEBUG}")
        print("=" * 60)


# Validate configuration on import
TestConfig.validate()


# Network-specific configurations
class NetworkConfig:
    """Network-specific parameters"""

    MAINNET = {
        "expected_bps": 1.0,
        "block_time_target": 1000,
        "k": 10,
    }

    TESTNET = {
        "expected_bps": 1.0,
        "block_time_target": 1000,
        "k": 10,
    }

    DEVNET = {
        "expected_bps": 1.0,
        "block_time_target": 1000,
        "k": 10,
    }

    @classmethod
    def get_config(cls, network: str = None) -> dict:
        """Get configuration for specific network"""
        network = network or TestConfig.NETWORK
        return getattr(cls, network.upper(), cls.DEVNET)


if __name__ == "__main__":
    # Print configuration when run as script
    TestConfig.print_config()
    print("\nNetwork Parameters:")
    config = NetworkConfig.get_config()
    for key, value in config.items():
        print(f"  {key}: {value}")
