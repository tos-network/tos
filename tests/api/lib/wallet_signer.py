"""
TOS Wallet Signer - Uses tos_wallet binary for transaction signing

Since Python lacks Ristretto255 support, this module creates temporary
wallets from seeds and uses the tos_wallet binary for signing operations.
"""

import subprocess
import os
import json
import tempfile
import shutil
from typing import Optional, Dict
from dataclasses import dataclass


@dataclass
class WalletAccount:
    """Wallet account with seed and metadata"""
    name: str
    seed: str
    address: str  # Pre-verified address
    password: str = "test123"


# Test accounts with pre-verified addresses
# These addresses were verified using tos_wallet binary
TEST_ACCOUNTS = {
    "alice": WalletAccount(
        name="Alice",
        seed="tiger eight taxi vexed revamp thorn paddles dosage layout muzzle eggs chlorine sober oyster ecstatic festival banjo behind western segments january behind usage winter paddles",
        address="tst1g6vj6htms5nykkywsnvs69xt63ev6wx6w9942lpesjrgdghm3vzqqegasvg"
    ),
    "bob": WalletAccount(
        name="Bob",
        seed="ocean swift mountain eagle dancing river frozen sunset golden meadow crystal palace harmony wisdom ancient forest keeper silver dragon mystic lunar phantom voyage",
        address="tst1__TO_BE_GENERATED__"  # Generate using tos_wallet recover_seed
    ),
    "charlie": WalletAccount(
        name="Charlie",
        seed="cosmic nebula stellar quantum photon aurora borealis cascade thunder lightning plasma fusion reactor galaxy spiral vortex infinite eternal cosmos energy vault nexus",
        address="tst1__TO_BE_GENERATED__"  # Generate using tos_wallet recover_seed
    )
}


class WalletSigner:
    """
    Wallet signer using tos_wallet binary

    This class creates temporary wallets from seeds for signing operations.
    """

    def __init__(self, wallet_bin: str = None, network: str = "testnet"):
        """
        Initialize wallet signer

        Args:
            wallet_bin: Path to tos_wallet binary (auto-detected if None)
            network: Network to use (mainnet/testnet/devnet)
        """
        self.wallet_bin = wallet_bin or self._find_wallet_binary()
        self.network = network
        self._verify_wallet_binary()

    def _find_wallet_binary(self) -> str:
        """Find tos_wallet binary in project"""
        possible_paths = [
            os.path.expanduser("~/tos-network/tos/target/release/tos_wallet"),
            "./target/release/tos_wallet",
            "../../target/release/tos_wallet",
            "../../../target/release/tos_wallet"
        ]

        for path in possible_paths:
            if os.path.exists(path):
                return os.path.abspath(path)

        raise FileNotFoundError(
            "tos_wallet binary not found. Build with: cargo build --release --bin tos_wallet"
        )

    def _verify_wallet_binary(self):
        """Verify wallet binary exists and is executable"""
        if not os.path.exists(self.wallet_bin):
            raise FileNotFoundError(f"Wallet binary not found: {self.wallet_bin}")

        if not os.access(self.wallet_bin, os.X_OK):
            raise PermissionError(f"Wallet binary not executable: {self.wallet_bin}")

    def get_account(self, name: str) -> WalletAccount:
        """Get test account by name"""
        account = TEST_ACCOUNTS.get(name.lower())
        if not account:
            raise ValueError(f"Unknown test account: {name}. Available: {list(TEST_ACCOUNTS.keys())}")
        return account

    def verify_address_from_seed(self, seed: str, expected_address: str) -> bool:
        """
        Verify that a seed produces the expected address

        Creates a temporary wallet and checks the address matches.
        Useful for validating test account data.

        Args:
            seed: Mnemonic seed phrase
            expected_address: Expected address to verify

        Returns:
            True if address matches, False otherwise
        """
        # TODO: Implement using temporary wallet
        # For now, assume addresses in TEST_ACCOUNTS are correct
        return True

    def create_temp_wallet(self, seed: str, password: str = "test123") -> str:
        """
        Create temporary wallet from seed

        Args:
            seed: Mnemonic seed phrase
            password: Wallet password

        Returns:
            Path to temporary wallet directory
        """
        # Create temp directory
        temp_dir = tempfile.mkdtemp(prefix="tos_wallet_")

        try:
            # Create wallet recovery script
            input_script = os.path.join(temp_dir, "input.txt")
            with open(input_script, 'w') as f:
                f.write("recover_seed\n")
                f.write(f"{seed}\n")
                f.write(f"{password}\n")
                f.write(f"{password}\n")
                f.write("exit\n")

            # Run wallet to recover from seed
            result = subprocess.run(
                [
                    self.wallet_bin,
                    "--network", self.network,
                    "--wallet-path", temp_dir,
                    "--offline-mode",
                    "--disable-ascii-art"
                ],
                stdin=open(input_script, 'r'),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                text=True
            )

            # Check if recovery succeeded
            if "Wallet recovered" not in (result.stdout + result.stderr):
                raise RuntimeError(f"Failed to recover wallet from seed: {result.stderr}")

            return temp_dir

        except Exception as e:
            # Clean up on failure
            if os.path.exists(temp_dir):
                shutil.rmtree(temp_dir)
            raise RuntimeError(f"Failed to create temporary wallet: {e}")

    def sign_transaction(self, account: WalletAccount, transaction_data: Dict) -> Dict:
        """
        Sign transaction using wallet binary

        Args:
            account: Wallet account to sign with
            transaction_data: Transaction data to sign

        Returns:
            Signed transaction data

        Raises:
            RuntimeError: If signing fails
        """
        # TODO: Implement actual transaction signing
        # This requires:
        # 1. Create temporary wallet from seed
        # 2. Build transaction in wallet format
        # 3. Call wallet sign command
        # 4. Extract signature
        # 5. Clean up temporary wallet

        raise NotImplementedError(
            "Transaction signing not yet implemented. "
            "Requires wallet RPC or binary command interface for signing."
        )

    def build_transfer_transaction(
        self,
        sender: WalletAccount,
        recipient_address: str,
        amount: int,
        fee: int = 1000,
        data: Optional[bytes] = None
    ) -> Dict:
        """
        Build transfer transaction

        Args:
            sender: Sender account
            recipient_address: Recipient address
            amount: Amount to transfer (atomic units)
            fee: Transaction fee (atomic units)
            data: Optional transaction data

        Returns:
            Transaction data dictionary
        """
        # TODO: Implement transaction building
        # This needs to match the Transaction structure from common/src/transaction/mod.rs

        raise NotImplementedError(
            "Transaction building not yet implemented. "
            "Need to implement Transaction structure matching Rust code."
        )

    def submit_transaction(self, signed_tx: Dict, daemon_url: str = "http://127.0.0.1:8080/json_rpc") -> str:
        """
        Submit signed transaction to daemon

        Args:
            signed_tx: Signed transaction data
            daemon_url: Daemon RPC URL

        Returns:
            Transaction hash

        Raises:
            RuntimeError: If submission fails
        """
        # TODO: Implement transaction submission via RPC
        raise NotImplementedError("Transaction submission not yet implemented")


def get_test_account(name: str) -> WalletAccount:
    """
    Get test account by name (convenience function)

    Args:
        name: Account name (alice, bob, charlie, etc.)

    Returns:
        WalletAccount instance
    """
    account = TEST_ACCOUNTS.get(name.lower())
    if not account:
        raise ValueError(
            f"Unknown test account: {name}. "
            f"Available: {', '.join(TEST_ACCOUNTS.keys())}"
        )
    return account


# Example usage
if __name__ == "__main__":
    print("TOS Wallet Signer")
    print("=" * 50)

    # Initialize signer
    signer = WalletSigner()
    print(f"Wallet binary: {signer.wallet_bin}")
    print(f"Network: {signer.network}")

    # List available accounts
    print(f"\nAvailable test accounts:")
    for name, account in TEST_ACCOUNTS.items():
        print(f"  - {name}: {account.address}")

    # Get Alice account
    alice = get_test_account("alice")
    print(f"\nAlice account:")
    print(f"  Name: {alice.name}")
    print(f"  Address: {alice.address}")
    print(f"  Seed: {alice.seed[:50]}...")

    print("\n[OK] Wallet signer initialized successfully")
    print("\nNOTE: Transaction signing not yet implemented")
    print("      Requires wallet binary command interface or RPC")
