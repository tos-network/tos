"""
Test get_info RPC API

Tests the get_info endpoint with focus on TIP-2 changes:
- New fields: bps, actual_bps
- GHOSTDAG fields: blue_score, topoheight
- Network metrics validation
"""

import pytest
import sys
from pathlib import Path

# Add parent directory to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient, RpcError
from config import TestConfig, NetworkConfig


@pytest.fixture
def client():
    """Create RPC client fixture"""
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client


@pytest.mark.tip2
def test_get_info_basic(client):
    """Test get_info returns expected fields"""
    result = client.get_info()

    # Essential fields
    assert "blue_score" in result, "Missing blue_score field"
    assert "topoheight" in result, "Missing topoheight field"
    assert "network" in result, "Missing network field"
    assert "version" in result, "Missing version field"

    # Validate types
    assert isinstance(result["blue_score"], int), "blue_score should be integer"
    assert isinstance(result["topoheight"], int), "topoheight should be integer"
    assert isinstance(result["network"], str), "network should be string"


@pytest.mark.tip2
def test_get_info_bps_fields(client):
    """Test get_info returns bps and actual_bps fields (TIP-2)"""
    result = client.get_info()

    # TIP-2: New BPS fields
    assert "bps" in result, "Missing bps field (TIP-2)"
    assert "actual_bps" in result, "Missing actual_bps field (TIP-2)"
    assert "block_time_target" in result, "Missing block_time_target field"
    assert "average_block_time" in result, "Missing average_block_time field"

    # Validate types
    assert isinstance(result["bps"], float), "bps should be float"
    assert isinstance(result["actual_bps"], float), "actual_bps should be float"
    assert isinstance(result["block_time_target"], int), "block_time_target should be int"
    assert isinstance(result["average_block_time"], int), "average_block_time should be int"


@pytest.mark.tip2
def test_bps_calculation(client):
    """Test that bps is correctly calculated from block_time_target"""
    result = client.get_info()

    block_time_target = result["block_time_target"]
    bps = result["bps"]

    # BPS = 1000 / block_time_target
    expected_bps = 1000.0 / block_time_target

    assert abs(bps - expected_bps) < 0.001, (
        f"BPS calculation incorrect: "
        f"got {bps}, expected {expected_bps} "
        f"(block_time_target={block_time_target})"
    )


@pytest.mark.tip2
def test_actual_bps_calculation(client):
    """Test that actual_bps is correctly calculated from average_block_time"""
    result = client.get_info()

    average_block_time = result["average_block_time"]
    actual_bps = result["actual_bps"]

    if average_block_time > 0:
        # actual_bps = 1000 / average_block_time
        expected_actual_bps = 1000.0 / average_block_time

        assert abs(actual_bps - expected_actual_bps) < 0.001, (
            f"Actual BPS calculation incorrect: "
            f"got {actual_bps}, expected {expected_actual_bps} "
            f"(average_block_time={average_block_time})"
        )
    else:
        # If average_block_time is 0, actual_bps should be 0
        assert actual_bps == 0.0, "actual_bps should be 0 when average_block_time is 0"


def test_network_field(client):
    """Test network field matches configuration"""
    result = client.get_info()

    network = result["network"]

    # Network enum Display returns abbreviated names:
    # "Mainnet", "Testnet", "Stagenet", "Dev" (not "devnet")
    network_map = {
        "mainnet": "Mainnet",
        "testnet": "Testnet",
        "stagenet": "Stagenet",
        "devnet": "Dev"
    }
    expected_network = network_map.get(TestConfig.NETWORK.lower(), TestConfig.NETWORK)

    assert network == expected_network, (
        f"Network mismatch: got {network}, expected {expected_network}"
    )


@pytest.mark.tip2
def test_ghostdag_fields(client):
    """Test GHOSTDAG-related fields (TIP-2)"""
    result = client.get_info()

    # GHOSTDAG fields
    assert "blue_score" in result
    assert "topoheight" in result
    assert "stable_blue_score" in result

    # Validate values
    assert result["blue_score"] >= 0
    assert result["topoheight"] >= 0
    assert result["stable_blue_score"] >= 0

    # stable_blue_score should be <= blue_score
    assert result["stable_blue_score"] <= result["blue_score"], (
        "stable_blue_score should not exceed blue_score"
    )


def test_supply_fields(client):
    """Test supply-related fields"""
    result = client.get_info()

    # Supply fields
    assert "circulating_supply" in result
    assert "emitted_supply" in result
    assert "maximum_supply" in result

    # Validate types
    assert isinstance(result["circulating_supply"], int)
    assert isinstance(result["emitted_supply"], int)
    assert isinstance(result["maximum_supply"], int)

    # Validate relationships
    assert result["circulating_supply"] >= 0
    assert result["emitted_supply"] >= result["circulating_supply"]
    assert result["maximum_supply"] >= result["emitted_supply"]


def test_difficulty_field(client):
    """Test difficulty field"""
    result = client.get_info()

    assert "difficulty" in result
    # Difficulty is VarUint (U256) which serializes as string for large number support
    assert isinstance(result["difficulty"], str)
    # Verify it's a valid numeric string
    difficulty_value = int(result["difficulty"])
    assert difficulty_value > 0, "Difficulty should be positive"


def test_reward_fields(client):
    """Test reward-related fields"""
    result = client.get_info()

    # Reward fields
    assert "block_reward" in result
    assert "dev_reward" in result
    assert "miner_reward" in result

    # Validate types
    assert isinstance(result["block_reward"], int)
    assert isinstance(result["dev_reward"], int)
    assert isinstance(result["miner_reward"], int)

    # Validate values
    assert result["block_reward"] >= 0
    assert result["dev_reward"] >= 0
    assert result["miner_reward"] >= 0

    # miner_reward + dev_reward should equal block_reward
    total = result["miner_reward"] + result["dev_reward"]
    assert total == result["block_reward"], (
        f"Reward split incorrect: "
        f"miner({result['miner_reward']}) + dev({result['dev_reward']}) "
        f"!= block_reward({result['block_reward']})"
    )


def test_mempool_size(client):
    """Test mempool_size field"""
    result = client.get_info()

    assert "mempool_size" in result
    assert isinstance(result["mempool_size"], int)
    assert result["mempool_size"] >= 0, "Mempool size cannot be negative"


def test_top_block_hash(client):
    """Test top_block_hash field"""
    result = client.get_info()

    assert "top_block_hash" in result
    assert isinstance(result["top_block_hash"], str)

    # Validate hash format (64 hex characters)
    top_hash = result["top_block_hash"]
    assert len(top_hash) == 64, f"Invalid hash length: {len(top_hash)}"
    assert all(c in "0123456789abcdef" for c in top_hash.lower()), (
        "Hash contains invalid characters"
    )


@pytest.mark.performance
def test_get_info_performance(client):
    """Test get_info response time"""
    import time

    iterations = 10
    times = []

    for _ in range(iterations):
        start = time.time()
        client.get_info()
        elapsed = (time.time() - start) * 1000  # Convert to ms
        times.append(elapsed)

    avg_time = sum(times) / len(times)
    max_time = max(times)

    print(f"\nget_info performance:")
    print(f"  Average: {avg_time:.2f}ms")
    print(f"  Max: {max_time:.2f}ms")

    # Performance threshold from config
    threshold = TestConfig.PERF_GET_INFO_MAX_MS
    assert avg_time < threshold, (
        f"get_info too slow: {avg_time:.2f}ms (threshold: {threshold}ms)"
    )


def test_bps_target_matches_network_config(client):
    """Test that target BPS matches network configuration"""
    result = client.get_info()
    network_config = NetworkConfig.get_config()

    expected_bps = network_config["expected_bps"]
    actual_bps = result["bps"]

    assert abs(actual_bps - expected_bps) < 0.01, (
        f"BPS target mismatch: got {actual_bps}, expected {expected_bps}"
    )


@pytest.mark.tip2
def test_block_time_target_matches_network_config(client):
    """Test that block_time_target matches network configuration"""
    result = client.get_info()
    network_config = NetworkConfig.get_config()

    expected_block_time = network_config["block_time_target"]
    actual_block_time = result["block_time_target"]

    assert actual_block_time == expected_block_time, (
        f"block_time_target mismatch: "
        f"got {actual_block_time}, expected {expected_block_time}"
    )


if __name__ == "__main__":
    # Run tests directly
    pytest.main([__file__, "-v", "-s"])
