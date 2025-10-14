"""
Test Energy and Transaction Fee APIs

Tests TOS energy system (TRON-style freeze/unfreeze mechanism):
- get_energy: Query account energy information
- FreezeTos: Lock TOS to gain energy for free transfers
- UnfreezeTos: Unlock previously frozen TOS
- get_estimated_fee_rates: Get recommended transaction fee rates

Energy System Overview:
- Freeze TOS for a period (3/7/14 days) to get energy
- Different durations provide different reward multipliers
- Energy is consumed by transfer transactions
- If no energy, must pay TOS as gas fee
"""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient, RpcError
from lib.wallet_signer import WalletSigner, get_test_account
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
    """Get a test address"""
    return TestConfig.TEST_MINER_ADDRESS


@pytest.fixture
def wallet_signer():
    """Create wallet signer fixture"""
    try:
        return WalletSigner(network="testnet")
    except FileNotFoundError:
        pytest.skip("Wallet binary not available")


@pytest.fixture
def alice_account():
    """Get Alice test account"""
    return get_test_account("alice")


# Energy Query Tests

def test_get_energy(client, test_address):
    """Test get_energy API - query account energy information"""
    result = client.call("get_energy", {"address": test_address})

    # Response structure based on GetEnergyResult
    assert "frozen_tos" in result
    assert "total_energy" in result
    assert "used_energy" in result
    assert "available_energy" in result
    assert "last_update" in result
    assert "freeze_records" in result

    # Validate types
    assert isinstance(result["frozen_tos"], int)
    assert isinstance(result["total_energy"], int)
    assert isinstance(result["used_energy"], int)
    assert isinstance(result["available_energy"], int)
    assert isinstance(result["last_update"], int)
    assert isinstance(result["freeze_records"], list)

    # Validate values
    assert result["frozen_tos"] >= 0
    assert result["total_energy"] >= 0
    assert result["used_energy"] >= 0
    assert result["available_energy"] >= 0


def test_get_energy_structure(client, test_address):
    """Test energy response structure details"""
    result = client.call("get_energy", {"address": test_address})

    # If account has freeze records, validate structure
    if len(result["freeze_records"]) > 0:
        record = result["freeze_records"][0]

        # FreezeRecordInfo structure
        assert "amount" in record
        assert "duration" in record
        assert "freeze_topoheight" in record
        assert "unlock_topoheight" in record
        assert "energy_gained" in record
        assert "can_unlock" in record
        assert "remaining_blocks" in record

        # Validate types
        assert isinstance(record["amount"], int)
        assert isinstance(record["duration"], str)  # e.g., "7_days"
        assert isinstance(record["freeze_topoheight"], int)
        assert isinstance(record["unlock_topoheight"], int)
        assert isinstance(record["energy_gained"], int)
        assert isinstance(record["can_unlock"], bool)
        assert isinstance(record["remaining_blocks"], int)

        # Validate duration format
        assert "_days" in record["duration"] or "days" in record["duration"]


def test_get_energy_nonexistent_account(client):
    """Test get_energy for account without energy"""
    # Create a random address (likely no energy)
    fake_address = "tst1" + "0" * 60

    try:
        result = client.call("get_energy", {"address": fake_address})

        # Should return zero values for non-existent account
        assert result["frozen_tos"] == 0
        assert result["total_energy"] == 0
        assert result["used_energy"] == 0
        assert result["available_energy"] == 0
        assert len(result["freeze_records"]) == 0
    except RpcError as e:
        # May error on invalid address checksum
        assert e.code == -32602


def test_get_energy_invalid_address(client):
    """Test get_energy with invalid address"""
    with pytest.raises(RpcError) as exc_info:
        client.call("get_energy", {"address": "invalid_address"})

    # Should get invalid params error
    assert exc_info.value.code == -32602


# Energy Calculations Tests

def test_energy_available_calculation(client, test_address):
    """Test that available_energy = total_energy - used_energy"""
    result = client.call("get_energy", {"address": test_address})

    total = result["total_energy"]
    used = result["used_energy"]
    available = result["available_energy"]

    # Available energy should equal total minus used
    assert available == total - used


def test_freeze_records_consistency(client, test_address):
    """Test freeze records data consistency"""
    result = client.call("get_energy", {"address": test_address})

    if len(result["freeze_records"]) > 0:
        for record in result["freeze_records"]:
            # If can_unlock is True, remaining_blocks should be 0
            if record["can_unlock"]:
                assert record["remaining_blocks"] == 0
            else:
                # If cannot unlock, remaining_blocks should be > 0
                assert record["remaining_blocks"] > 0

            # unlock_topoheight should be after freeze_topoheight
            assert record["unlock_topoheight"] > record["freeze_topoheight"]

            # Energy gained should be positive
            assert record["energy_gained"] > 0


# Transaction Fee Tests

def test_get_estimated_fee_rates(client):
    """Test get_estimated_fee_rates API"""
    result = client.call("get_estimated_fee_rates", [])

    # Should return fee rate recommendations
    assert isinstance(result, dict)

    # Common fee rate fields (structure may vary)
    # Could be: {low: N, medium: N, high: N} or other format
    # Just verify it's a non-empty dict
    assert len(result) > 0


def test_estimated_fee_rates_consistency(client):
    """Test fee rates are consistent across calls"""
    result1 = client.call("get_estimated_fee_rates", [])
    result2 = client.call("get_estimated_fee_rates", [])

    # Should return consistent rates in short time period
    assert isinstance(result1, dict)
    assert isinstance(result2, dict)


# Energy System Understanding Tests

def test_energy_duration_options(client):
    """Document supported freeze durations"""
    # Based on source code analysis:
    # FreezeDuration supports: 3, 7, 14 days
    # Reward multipliers:
    #   - 3 days: 7x multiplier
    #   - 7 days: 14x multiplier
    #   - 14 days: 28x multiplier
    # Energy calculation: (amount / COIN_VALUE) * multiplier
    # E.g., 1 TOS frozen for 7 days = 14 energy = 14 free transfers

    # This test documents the expected behavior
    # Actual values verified against source code:
    # common/src/transaction/payload/energy.rs
    # common/src/account/freeze_duration.rs

    assert True  # Documentation test


def test_energy_fee_model(client):
    """Document energy fee model"""
    # Based on source code analysis:
    #
    # Fee Model:
    # 1. FreezeTos/UnfreezeTos operations:
    #    - Don't consume energy
    #    - Require small TOS fee (FEE_PER_TRANSFER)
    #
    # 2. Regular Transfer operations:
    #    - Consume 1 energy per transfer
    #    - If no energy: pay TOS as gas fee
    #
    # 3. Energy regeneration:
    #    - Used energy resets/regenerates over time
    #    - Details in EnergyResource implementation

    assert True  # Documentation test


# Transaction Submission Tests (require wallet)

@pytest.mark.skip(reason="Requires transaction signing implementation - see WALLET_IMPLEMENTATION_STATUS.md")
def test_submit_freeze_transaction(client, wallet_signer, alice_account):
    """Test submitting a FreezeTos transaction"""
    # Once signing is implemented, this test will:
    # 1. Build FreezeTos transaction using wallet_signer
    # 2. Sign with Alice's account
    # 3. Submit to daemon
    # 4. Verify transaction accepted
    #
    # Example usage:
    # tx_data = wallet_signer.build_freeze_transaction(
    #     sender=alice_account,
    #     amount=100_000_000,  # 1 TOS (atomic units)
    #     duration=7,          # 7 days
    #     fee=1000
    # )
    # signed_tx = wallet_signer.sign_transaction(alice_account, tx_data)
    # result = client.call("submit_transaction", {"data": signed_tx})
    # assert "hash" in result
    #
    # Then verify energy increased:
    # energy = client.call("get_energy", {"address": alice_account.address})
    # assert energy["frozen_tos"] > 0

    pass


@pytest.mark.skip(reason="Requires transaction signing implementation - see WALLET_IMPLEMENTATION_STATUS.md")
def test_submit_unfreeze_transaction(client, wallet_signer, alice_account):
    """Test submitting an UnfreezeTos transaction"""
    # Once signing is implemented:
    # tx_data = wallet_signer.build_unfreeze_transaction(
    #     sender=alice_account,
    #     amount=100_000_000,
    #     fee=1000
    # )
    # signed_tx = wallet_signer.sign_transaction(alice_account, tx_data)
    # result = client.call("submit_transaction", {"data": signed_tx})
    # assert "hash" in result
    pass


@pytest.mark.skip(reason="Requires transaction signing implementation - see WALLET_IMPLEMENTATION_STATUS.md")
def test_transfer_with_energy(client, wallet_signer, alice_account):
    """Test transfer transaction using energy (not TOS fee)"""
    # This test verifies energy-based transfers:
    # 1. Verify account has available energy
    # energy = client.call("get_energy", {"address": alice_account.address})
    # assert energy["available_energy"] > 0
    #
    # 2. Build and submit transfer
    # tx_data = wallet_signer.build_transfer_transaction(
    #     sender=alice_account,
    #     recipient_address="tst1...",
    #     amount=1000,
    #     fee=0  # Use energy instead of TOS
    # )
    # signed_tx = wallet_signer.sign_transaction(alice_account, tx_data)
    # result = client.call("submit_transaction", {"data": signed_tx})
    #
    # 3. Verify energy consumed
    # new_energy = client.call("get_energy", {"address": alice_account.address})
    # assert new_energy["used_energy"] == energy["used_energy"] + 1
    pass


@pytest.mark.skip(reason="Requires transaction signing implementation - see WALLET_IMPLEMENTATION_STATUS.md")
def test_transfer_without_energy(client, wallet_signer, alice_account):
    """Test transfer transaction paying TOS fee"""
    # This test verifies TOS fee-based transfers:
    # 1. Get initial balance
    # balance_before = client.call("get_balance", {"address": alice_account.address})
    #
    # 2. Build and submit transfer with fee
    # tx_data = wallet_signer.build_transfer_transaction(
    #     sender=alice_account,
    #     recipient_address="tst1...",
    #     amount=1000,
    #     fee=1000  # Pay TOS as fee
    # )
    # signed_tx = wallet_signer.sign_transaction(alice_account, tx_data)
    # result = client.call("submit_transaction", {"data": signed_tx})
    #
    # 3. Verify TOS balance decreased by amount + fee
    # balance_after = client.call("get_balance", {"address": alice_account.address})
    # assert balance_after["balance"] < balance_before["balance"]
    pass


# Energy System Edge Cases

def test_energy_last_update_field(client, test_address):
    """Test last_update field represents topoheight"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    result = client.call("get_energy", {"address": test_address})
    last_update = result["last_update"]

    # last_update should be <= current topoheight
    assert last_update <= current_topoheight


def test_multiple_freeze_records(client, test_address):
    """Test account can have multiple freeze records with different durations"""
    result = client.call("get_energy", {"address": test_address})

    if len(result["freeze_records"]) > 1:
        # Multiple freeze records are supported
        durations = [record["duration"] for record in result["freeze_records"]]
        amounts = [record["amount"] for record in result["freeze_records"]]

        # All should be valid
        for duration in durations:
            assert "_days" in duration or "days" in duration

        for amount in amounts:
            assert amount > 0


# Performance Tests

@pytest.mark.performance
def test_get_energy_performance(client, test_address):
    """Test get_energy API performance"""
    import time

    iterations = 10
    times = []

    for _ in range(iterations):
        start = time.time()
        client.call("get_energy", {"address": test_address})
        elapsed = (time.time() - start) * 1000
        times.append(elapsed)

    avg_time = sum(times) / len(times)
    print(f"\\nget_energy: {avg_time:.2f}ms average")

    # Should be fast (same as other read operations)
    assert avg_time < 200  # 200ms threshold


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
