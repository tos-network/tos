"""
Test Network and P2P RPC APIs

Tests P2P network status, peer management, and mempool:
- p2p_status, get_peers
- get_mempool, get_mempool_summary, get_mempool_cache
- get_estimated_fee_rates
"""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient


@pytest.fixture
def client():
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client


# P2P Status Tests

def test_p2p_status(client):
    """Test p2p_status API"""
    result = client.call("p2p_status", [])

    assert "peer_count" in result
    assert "max_peers" in result
    assert isinstance(result["peer_count"], int)
    assert isinstance(result["max_peers"], int)
    assert result["peer_count"] >= 0
    assert result["peer_count"] <= result["max_peers"]


def test_get_peers(client):
    """Test get_peers API"""
    result = client.call("get_peers", [])

    # API returns object with peers list, not direct list
    assert isinstance(result, dict)
    assert "peers" in result
    assert "total_peers" in result
    assert "hidden_peers" in result
    assert isinstance(result["peers"], list)

    # If there are peers, validate structure
    for peer in result["peers"]:
        assert "addr" in peer
        assert "id" in peer  # "id" not "peer_id"
        assert "blue_work" in peer
        assert "topoheight" in peer
        assert "connected_on" in peer
        assert isinstance(peer["id"], int)


def test_get_peers_structure(client):
    """Test peer entry structure"""
    result = client.call("get_peers", [])

    if len(result["peers"]) == 0:
        pytest.skip("No peers connected")

    peer = result["peers"][0]

    # Required fields based on PeerEntry struct
    required_fields = [
        "id", "addr", "local_port", "tag", "version",
        "top_block_hash", "topoheight", "height", "last_ping",
        "blue_work", "connected_on", "bytes_sent", "bytes_recv"
    ]

    for field in required_fields:
        assert field in peer, f"Missing field: {field}"


# Mempool Tests

def test_get_mempool(client):
    """Test get_mempool API"""
    # GetMempoolParams requires explicit struct fields
    result = client.call("get_mempool", {"maximum": None, "skip": None})

    # Response may be list or object
    assert isinstance(result, (list, dict))
    if isinstance(result, list):
        # Each transaction should have hash
        for tx in result:
            assert "hash" in tx
            assert isinstance(tx["hash"], str)
    else:
        # Object with transactions list
        assert "transactions" in result


def test_get_mempool_summary(client):
    """Test get_mempool_summary API"""
    # Uses same params as get_mempool - requires explicit struct fields
    result = client.call("get_mempool_summary", {"maximum": None, "skip": None})

    # Response has "total" field, not "count"
    assert "total" in result or "count" in result
    if "total" in result:
        assert isinstance(result["total"], int)
        assert result["total"] >= 0
    else:
        assert isinstance(result["count"], int)
        assert result["count"] >= 0


def test_get_mempool_cache(client):
    """Test get_mempool_cache API"""
    # GetMempoolCacheParams requires an address
    # Use test miner address - but account may not have mempool cache
    from config import TestConfig
    from lib.rpc_client import RpcError
    try:
        result = client.call("get_mempool_cache", {
            "address": TestConfig.TEST_MINER_ADDRESS
        })
        assert isinstance(result, (list, dict))
    except RpcError as e:
        # Expected if account not found in mempool
        assert e.code == -32004


def test_get_estimated_fee_rates(client):
    """Test get_estimated_fee_rates API"""
    result = client.call("get_estimated_fee_rates", [])

    # Should return fee rate recommendations
    assert isinstance(result, dict)


# Mempool Consistency Tests

def test_mempool_count_consistency(client):
    """Test mempool count matches actual transactions"""
    params = {"maximum": None, "skip": None}
    mempool = client.call("get_mempool", params)
    summary = client.call("get_mempool_summary", params)

    # Get count from appropriate field
    summary_count = summary.get("total", summary.get("count", 0))

    # Get mempool size
    if isinstance(mempool, list):
        mempool_size = len(mempool)
    else:
        mempool_size = len(mempool.get("transactions", []))

    # Summary count should match actual mempool size
    assert summary_count == mempool_size


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
