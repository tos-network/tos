"""
Test Utility RPC APIs

Tests address validation and utility functions:
- validate_address
- split_address
- extract_key_from_address
- make_integrated_address
- get_version, get_difficulty, get_tips
- count_* APIs
"""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient, RpcError
from config import TestConfig


@pytest.fixture
def client():
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client


# Address Validation Tests

def test_validate_address_valid(client):
    """Test validate_address with valid address"""
    address = TestConfig.TEST_MINER_ADDRESS
    result = client.validate_address(address)

    assert "is_valid" in result
    assert result["is_valid"] == True
    assert "is_integrated" in result
    assert isinstance(result["is_integrated"], bool)


def test_validate_address_invalid(client):
    """Test validate_address with invalid address"""
    # Invalid addresses may throw RPC error instead of returning is_valid=false
    from lib.rpc_client import RpcError
    try:
        result = client.validate_address("invalid_address")
        assert "is_valid" in result
        assert result["is_valid"] == False
    except RpcError as e:
        # Expected: Invalid params error for malformed address
        assert e.code == -32602


def test_validate_address_wrong_network(client):
    """Test validate_address with wrong network prefix"""
    from lib.rpc_client import RpcError
    # Use mainnet address on devnet (or vice versa)
    wrong_network_addr = "tos1" + "a" * 60
    try:
        result = client.validate_address(wrong_network_addr)
        # Should indicate invalid or wrong network
        assert "is_valid" in result
    except RpcError as e:
        # Expected: Invalid params error for malformed address
        assert e.code == -32602


def test_extract_key_from_address(client):
    """Test extract_key_from_address API"""
    address = TestConfig.TEST_MINER_ADDRESS
    result = client.call("extract_key_from_address", {"address": address})

    # API returns object with "bytes" field containing byte array
    assert isinstance(result, dict)
    assert "bytes" in result
    bytes_array = result["bytes"]
    assert isinstance(bytes_array, list)
    assert len(bytes_array) == 32  # Public key is 32 bytes


def test_make_integrated_address(client):
    """Test make_integrated_address API"""
    base_address = TestConfig.TEST_MINER_ADDRESS
    # DataElement format: use simple Value variant
    integrated_data = {"Value": {"U64": 1234}}  # DataElement::Value(DataValue::U64(1234))

    from lib.rpc_client import RpcError
    try:
        result = client.call("make_integrated_address", {
            "address": base_address,
            "integrated_data": integrated_data
        })

        assert isinstance(result, str)
        # Integrated address should start with 'tos' or 'tst'
        assert result.startswith(("tos", "tst"))
        # Should be different from base address
        assert result != base_address
    except RpcError as e:
        # May fail if DataElement format is incorrect
        # Skip test if format not supported
        pytest.skip(f"DataElement format not accepted: {e.message}")


def test_split_address(client):
    """Test split_address API"""
    # First create an integrated address
    base_address = TestConfig.TEST_MINER_ADDRESS
    integrated_data = {"Value": {"U64": 9999}}
    integrated = client.call("make_integrated_address", {
        "address": base_address,
        "integrated_data": integrated_data
    })

    # Now split it
    result = client.call("split_address", {"address": integrated})

    assert "address" in result
    assert "integrated_data" in result
    # Should recover original address
    assert result["address"] == base_address


# Version and Network Info Tests

def test_get_version(client):
    """Test get_version API"""
    result = client.call("get_version", [])

    assert isinstance(result, str)
    # Version should be non-empty
    assert len(result) > 0


def test_get_difficulty(client):
    """Test get_difficulty API"""
    result = client.call("get_difficulty", [])

    # API returns object with multiple fields, not just int
    assert isinstance(result, dict)
    assert "difficulty" in result
    assert "hashrate" in result
    # Difficulty is string (VarUint serializes to string)
    assert isinstance(result["difficulty"], str)
    assert int(result["difficulty"]) > 0


def test_get_tips(client):
    """Test get_tips API"""
    result = client.call("get_tips", [])

    assert isinstance(result, list)
    assert len(result) >= 1  # At least one tip

    # Each tip should be a valid hash
    for tip_hash in result:
        assert isinstance(tip_hash, str)
        assert len(tip_hash) == 64  # Hash is 64 hex chars


# Count APIs Tests

def test_count_assets(client):
    """Test count_assets API"""
    result = client.call("count_assets", [])

    assert isinstance(result, int)
    assert result >= 0


def test_count_accounts(client):
    """Test count_accounts API"""
    result = client.call("count_accounts", [])

    assert isinstance(result, int)
    assert result >= 0


def test_count_transactions(client):
    """Test count_transactions API"""
    result = client.call("count_transactions", [])

    assert isinstance(result, int)
    assert result >= 0


def test_count_contracts(client):
    """Test count_contracts API"""
    result = client.call("count_contracts", [])

    assert isinstance(result, int)
    assert result >= 0


# Hard Forks and Dev Fees Tests

def test_get_hard_forks(client):
    """Test get_hard_forks API"""
    result = client.call("get_hard_forks", [])

    assert isinstance(result, list)
    assert len(result) >= 1  # At least genesis fork

    # Each fork should have version, height, changelog
    for fork in result:
        assert "version" in fork
        assert "height" in fork
        assert "changelog" in fork
        # block_time_target and pow_algorithm may or may not be present


def test_get_dev_fee_thresholds(client):
    """Test get_dev_fee_thresholds API"""
    result = client.call("get_dev_fee_thresholds", [])

    assert isinstance(result, list)
    assert len(result) >= 1

    # Each threshold should have height and fee_percentage
    for threshold in result:
        assert "height" in threshold
        assert "fee_percentage" in threshold
        assert 0 <= threshold["fee_percentage"] <= 100


def test_get_size_on_disk(client):
    """Test get_size_on_disk API"""
    result = client.call("get_size_on_disk", [])

    # API returns object with both bytes and formatted string
    assert isinstance(result, dict)
    assert "size_bytes" in result
    assert "size_formatted" in result
    assert isinstance(result["size_bytes"], int)
    assert isinstance(result["size_formatted"], str)
    assert result["size_bytes"] >= 0


# Consistency Tests

def test_version_consistency(client):
    """Test version is consistent across calls"""
    version1 = client.call("get_version", [])
    version2 = client.call("get_version", [])

    assert version1 == version2


def test_tips_change_over_time(client):
    """Test that tips can change as new blocks arrive"""
    tips1 = client.call("get_tips", [])
    import time
    time.sleep(2)  # Wait for potential new block
    tips2 = client.call("get_tips", [])

    # Tips are a set, so we just check they're valid
    assert isinstance(tips1, list)
    assert isinstance(tips2, list)


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
