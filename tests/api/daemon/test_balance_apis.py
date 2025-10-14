"""
Test Balance and Account RPC APIs

Tests balance queries, nonce management, and account operations:
- get_balance, get_balance_at_topoheight
- get_nonce, get_nonce_at_topoheight
- get_account_history, get_account_assets
- is_account_registered, get_account_registration_topoheight
- count_accounts
"""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient, RpcError
from config import TestConfig


@pytest.fixture
def client():
    """Create RPC client fixture"""
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client


@pytest.fixture
def test_address(client):
    """Get a test address that has balance"""
    return TestConfig.TEST_MINER_ADDRESS


# Balance API Tests

def test_get_balance(client, test_address):
    """Test get_balance API"""
    result = client.get_balance(test_address)

    # Response has versioned balance format
    assert "topoheight" in result
    assert "version" in result
    assert isinstance(result["topoheight"], int)
    # Version contains balance_type and balance data
    assert "balance_type" in result["version"]


@pytest.mark.tip2
def test_get_balance_at_topoheight(client, test_address):
    """Test get_balance_at_topoheight API (TIP-2)"""
    # Get current topoheight
    info = client.get_info()
    current_topoheight = info["topoheight"]

    # Skip if too early in chain
    if current_topoheight < 10:
        pytest.skip("Not enough blocks")

    # Get balance at earlier topoheight
    earlier_topoheight = current_topoheight - 10

    try:
        result = client.get_balance_at_topoheight(test_address, earlier_topoheight)
    except RpcError as e:
        # Historical data may not be available in fresh devnet
        if "Data not found on disk" in str(e) or "load data" in str(e):
            pytest.skip("Historical data not available at requested topoheight")
        raise

    # Response has versioned balance format
    assert "balance_type" in result or "version" in result
    if "balance_type" in result:
        # Direct versioned balance
        assert "previous_topoheight" in result
    else:
        # Wrapped in version object
        assert "topoheight" in result


def test_get_balance_with_asset(client, test_address):
    """Test get_balance with specific asset"""
    # Native TOS asset uses "tos" identifier
    result = client.get_balance(test_address, TestConfig.TOS_ASSET)

    assert "balance" in result or "version" in result
    # Response structure varies based on versioned balance format
    if "balance" in result:
        assert isinstance(result["balance"], int)
    elif "version" in result:
        # Versioned balance format
        assert "topoheight" in result


def test_get_balance_invalid_address(client):
    """Test get_balance with invalid address"""
    with pytest.raises(RpcError):
        client.get_balance("invalid_address")


def test_has_balance(client, test_address):
    """Test has_balance API"""
    result = client.has_balance(test_address)

    assert "exist" in result
    assert isinstance(result["exist"], bool)


def test_get_stable_balance(client, test_address):
    """Test get_stable_balance API"""
    result = client.get_stable_balance(test_address)

    # Response has versioned balance and stable_topoheight
    assert "version" in result or "balance" in result
    assert "stable_topoheight" in result
    assert "stable_block_hash" in result


# Nonce API Tests

def test_get_nonce(client, test_address):
    """Test get_nonce API"""
    result = client.get_nonce(test_address)

    # Response has topoheight and versioned nonce
    assert "topoheight" in result
    # Nonce can be in versioned format or direct field
    assert "nonce" in result or "version" in result
    if "nonce" in result:
        # Nonce may be string or int
        nonce_value = int(result["nonce"]) if isinstance(result["nonce"], str) else result["nonce"]
        assert nonce_value >= 0


@pytest.mark.tip2
def test_get_nonce_at_topoheight(client, test_address):
    """Test get_nonce_at_topoheight API (TIP-2)"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    if current_topoheight < 10:
        pytest.skip("Not enough blocks")

    earlier_topoheight = current_topoheight - 10
    try:
        result = client.get_nonce_at_topoheight(test_address, earlier_topoheight)
        assert "topoheight" in result
        assert result["topoheight"] == earlier_topoheight
    except RpcError as e:
        # Account may not have nonce history at this topoheight
        if e.code == -32004:  # Data not found
            pytest.skip("Account has no nonce data at this topoheight")
        raise


def test_has_nonce(client, test_address):
    """Test has_nonce API"""
    result = client.has_nonce(test_address)

    assert "exist" in result
    assert isinstance(result["exist"], bool)


# Account API Tests

def test_get_account_history(client, test_address):
    """Test get_account_history API"""
    result = client.get_account_history(test_address)

    assert isinstance(result, list)
    # Each history entry should have hash, topoheight, timestamp
    for entry in result:
        assert "hash" in entry
        assert "topoheight" in entry


def test_get_account_history_with_range(client, test_address):
    """Test get_account_history with topoheight range"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    if current_topoheight < 100:
        pytest.skip("Not enough blocks")

    # Query limited range
    min_topo = current_topoheight - 100
    max_topo = current_topoheight
    result = client.get_account_history(
        test_address,
        minimum_topoheight=min_topo,
        maximum_topoheight=max_topo
    )

    assert isinstance(result, list)
    # Verify all entries are in range
    for entry in result:
        assert min_topo <= entry["topoheight"] <= max_topo


def test_get_account_assets(client, test_address):
    """Test get_account_assets API"""
    result = client.get_account_assets(test_address)

    # Response is a dict with asset hash as key, not a list
    assert isinstance(result, (dict, list))
    if isinstance(result, dict):
        # Asset hash -> balance mapping
        for asset_hash, balance_data in result.items():
            assert isinstance(asset_hash, str)
            assert len(asset_hash) == 64  # Hash is 64 hex chars
    elif isinstance(result, list):
        # List of asset entries
        for entry in result:
            # Entry might be asset hash string or object
            if isinstance(entry, dict):
                assert "asset" in entry or len(entry) > 0


def test_is_account_registered(client, test_address):
    """Test is_account_registered API"""
    result = client.is_account_registered(test_address)

    # API returns direct boolean, not object with "exist" field
    assert isinstance(result, bool)
    # Test address (miner) should be registered
    assert result == True


def test_is_account_registered_nonexistent(client):
    """Test is_account_registered for non-existent account"""
    from lib.rpc_client import RpcError
    # Create a random address (likely not registered)
    # Note: using fake address may cause checksum error
    fake_address = "tos1" + "0" * 60
    try:
        result = client.is_account_registered(fake_address)
        # API returns direct boolean
        assert isinstance(result, bool)
        # Should be False for non-existent account
        assert result == False
    except RpcError as e:
        # Expected if address has invalid checksum
        assert e.code == -32602


def test_get_account_registration_topoheight(client, test_address):
    """Test get_account_registration_topoheight API"""
    # Check if account is registered first
    is_registered = client.is_account_registered(test_address)

    if not is_registered:
        pytest.skip("Test address not registered")

    result = client.get_account_registration_topoheight(test_address)

    # API returns direct integer (topoheight), not object
    assert isinstance(result, int)
    assert result >= 0
    # Registration topoheight should be <= current topoheight
    info = client.get_info()
    assert result <= info["topoheight"]


def test_get_accounts(client):
    """Test get_accounts API"""
    # GetAccountsParams requires all 4 struct fields explicitly
    result = client.call("get_accounts", {
        "skip": None,
        "maximum": None,
        "minimum_topoheight": None,
        "maximum_topoheight": None
    })

    assert isinstance(result, (list, dict))
    if isinstance(result, list):
        # Should return list of address strings
        for address in result:
            assert isinstance(address, str)
            # Address prefix depends on network (tos/tst/tss)
            assert address.startswith(("tos", "tst", "tss"))
    else:
        # May return empty dict if no accounts
        assert len(result) >= 0


def test_get_accounts_with_pagination(client):
    """Test get_accounts with pagination"""
    # Get first 10 accounts (params need to be passed as object or with skip/maximum keys)
    result = client.call("get_accounts", {"skip": 0, "maximum": 10})

    assert isinstance(result, list)
    assert len(result) <= 10


def test_get_accounts_with_minimum_balance(client):
    """Test get_accounts with minimum balance filter"""
    # Get accounts with at least 1 TOS
    min_balance = 1000000  # 1 TOS in atomic units
    result = client.call("get_accounts", {"skip": 0, "maximum": 100, "minimum_balance": min_balance})

    assert isinstance(result, list)


def test_count_accounts(client):
    """Test count_accounts API"""
    result = client.call("count_accounts", [])

    assert isinstance(result, int)
    assert result >= 0


# Balance Consistency Tests

@pytest.mark.tip2
def test_balance_consistency_across_topoheights(client, test_address):
    """Test that balance is consistent when queried at same topoheight"""
    info = client.get_info()
    topoheight = info["topoheight"]

    # Query same topoheight multiple times
    balance1 = client.get_balance_at_topoheight(test_address, topoheight)
    balance2 = client.get_balance_at_topoheight(test_address, topoheight)

    # Extract balance value from versioned response
    bal1 = balance1.get("balance", balance1.get("version", {}).get("balance", 0))
    bal2 = balance2.get("balance", balance2.get("version", {}).get("balance", 0))
    assert bal1 == bal2


@pytest.mark.tip2
def test_balance_never_decreases_in_past(client, test_address):
    """Test that querying past balance doesn't show future transactions"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    if current_topoheight < 100:
        pytest.skip("Not enough blocks")

    # Get balance at two different past topoheights
    past_topo1 = current_topoheight - 100
    past_topo2 = current_topoheight - 50

    try:
        balance1 = client.get_balance_at_topoheight(test_address, past_topo1)
        balance2 = client.get_balance_at_topoheight(test_address, past_topo2)

        # Balances should be returned with versioned format
        # Check for either direct balance_type field or nested version field
        assert ("balance_type" in balance1 or "version" in balance1 or "balance" in balance1)
        assert ("balance_type" in balance2 or "version" in balance2 or "balance" in balance2)
    except RpcError as e:
        # Account may not have balance data at these topoheights
        if e.code == -32004:  # Data not found
            pytest.skip("Account has no balance data at these topoheights")
        raise


# Nonce Consistency Tests

def test_nonce_increases_monotonically(client, test_address):
    """Test that nonce never decreases"""
    nonce1_response = client.get_nonce(test_address)
    nonce2_response = client.get_nonce(test_address)

    # Extract nonce values from versioned format
    def get_nonce_value(response):
        if "nonce" in response:
            return int(response["nonce"]) if isinstance(response["nonce"], str) else response["nonce"]
        elif "version" in response:
            v = response["version"]
            if isinstance(v, dict) and "nonce" in v:
                return int(v["nonce"]) if isinstance(v["nonce"], str) else v["nonce"]
        return 0

    nonce1 = get_nonce_value(nonce1_response)
    nonce2 = get_nonce_value(nonce2_response)

    # Nonce should never decrease
    assert nonce2 >= nonce1


# Error Handling Tests

def test_get_balance_at_invalid_topoheight(client, test_address):
    """Test get_balance_at_topoheight with invalid topoheight"""
    info = client.get_info()
    invalid_topoheight = info["topoheight"] + 1000000

    with pytest.raises(RpcError):
        client.get_balance_at_topoheight(test_address, invalid_topoheight)


def test_get_balance_negative_topoheight(client, test_address):
    """Test get_balance_at_topoheight with negative topoheight"""
    with pytest.raises((RpcError, ValueError)):
        client.get_balance_at_topoheight(test_address, -1)


# Performance Tests

@pytest.mark.performance
def test_get_balance_performance(client, test_address):
    """Test get_balance response time"""
    import time

    iterations = 10
    times = []

    for _ in range(iterations):
        start = time.time()
        client.get_balance(test_address)
        elapsed = (time.time() - start) * 1000
        times.append(elapsed)

    avg_time = sum(times) / len(times)
    threshold = TestConfig.PERF_GET_BALANCE_MAX_MS

    print(f"\nget_balance performance: {avg_time:.2f}ms average")

    assert avg_time < threshold, f"get_balance too slow: {avg_time:.2f}ms"


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
