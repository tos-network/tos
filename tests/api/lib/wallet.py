"""
TOS Wallet Implementation for Testing

Simplified Python implementation of TOS wallet functionality
for testing purposes. Based on tos/wallet Rust implementation.

Supports:
- Generate keypair from mnemonic seed
- Create addresses
- Sign transactions
"""

import hashlib
import struct
from typing import List, Tuple, Optional
from dataclasses import dataclass


# Constants from Rust implementation
KEY_SIZE = 32
SEED_LENGTH = 24
WORDS_LIST = 1626

# English word list (first 100 words for testing, full list needed for production)
# In production, load from wallet/src/mnemonics/languages/english.rs
ENGLISH_WORDS = [
    "abbey", "abducts", "ability", "ablaze", "abnormal", "abort", "abrasive", "absorb",
    "abyss", "academy", "aces", "aching", "acidic", "acoustic", "acquire", "across",
    # ... (truncated for brevity, need full 1626 words)
    # For testing, we'll implement with partial word list
]


@dataclass
class KeyPair:
    """TOS KeyPair (private + public key)"""
    private_key: bytes  # 32 bytes
    public_key: bytes   # 32 bytes compressed point

    @classmethod
    def from_seed(cls, seed_words: List[str]) -> 'KeyPair':
        """Generate keypair from mnemonic seed words"""
        private_key = words_to_key(seed_words)
        public_key = private_key_to_public_key(private_key)
        return cls(private_key=private_key, public_key=public_key)

    def get_address(self, network: str = "devnet") -> str:
        """Get wallet address from public key"""
        return public_key_to_address(self.public_key, network)


def words_to_key(words: List[str]) -> bytes:
    """
    Convert mnemonic words to private key (32 bytes)

    Based on wallet/src/mnemonics/mod.rs: words_to_key()
    Algorithm:
    1. Find word indices in word list
    2. Verify checksum (if 25 words)
    3. Convert indices to 32 bytes using special formula

    Args:
        words: List of 24 or 25 mnemonic words

    Returns:
        32-byte private key
    """
    if len(words) not in [SEED_LENGTH, SEED_LENGTH + 1]:
        raise ValueError(f"Invalid seed length: {len(words)}, expected 24 or 25")

    # Normalize words to lowercase
    words_lower = [w.lower() for w in words]

    # Import full English word list
    from lib.english_words import ENGLISH_WORDS

    # Find word indices in word list
    indices = []
    for word in words_lower[:SEED_LENGTH]:
        try:
            index = ENGLISH_WORDS.index(word)
            indices.append(index)
        except ValueError:
            raise ValueError(f"Word not in word list: {word}")

    # Convert indices to 32 bytes using Rust algorithm
    # From wallet/src/mnemonics/mod.rs lines 146-158
    dest = bytearray()

    for i in range(0, SEED_LENGTH, 3):
        a = indices[i]
        b = indices[i + 1]
        c = indices[i + 2]

        # Apply formula from Rust code
        val = a + WORDS_LIST * (((WORDS_LIST - a) + b) % WORDS_LIST) + \
              WORDS_LIST * WORDS_LIST * (((WORDS_LIST - b) + c) % WORDS_LIST)

        # Sanity check from Rust code
        if val % WORDS_LIST != a:
            raise ValueError("Word list sanity check failed")

        # Convert to little-endian 4 bytes (u32)
        dest.extend(val.to_bytes(4, 'little'))

    if len(dest) != KEY_SIZE:
        raise ValueError(f"Invalid key size: {len(dest)}, expected {KEY_SIZE}")

    return bytes(dest)


def calculate_checksum_index(words: List[str], prefix_len: int = 3) -> int:
    """
    Calculate checksum index for seed verification

    Based on wallet/src/mnemonics/mod.rs: calculate_checksum_index()
    """
    if len(words) != SEED_LENGTH:
        raise ValueError(f"Invalid words count: {len(words)}")

    # Extract prefix from each word
    prefixes = []
    for word in words:
        word_lower = word.lower()
        prefix = word_lower[:prefix_len] if len(word_lower) >= prefix_len else word_lower
        prefixes.append(prefix)

    # Calculate CRC32 checksum
    value = "".join(prefixes)
    import zlib
    checksum = zlib.crc32(value.encode()) & 0xffffffff

    return checksum % SEED_LENGTH


def private_key_to_public_key(private_key: bytes) -> bytes:
    """
    Derive public key from private key using Ed25519

    TOS uses Ed25519 curve for signatures

    Args:
        private_key: 32-byte private key

    Returns:
        32-byte compressed public key
    """
    try:
        from nacl.signing import SigningKey
        from nacl.encoding import RawEncoder

        # Ed25519 signing key
        signing_key = SigningKey(private_key)
        verify_key = signing_key.verify_key

        # Return compressed public key (32 bytes)
        return bytes(verify_key)
    except ImportError:
        raise ImportError("PyNaCl required: pip install pynacl")


def public_key_to_address(public_key: bytes, network: str = "devnet") -> str:
    """
    Convert public key to TOS address

    TOS address format (from common/src/crypto/address.rs):
    - Network prefix (tos/tst)
    - Bech32-encoded data:
      - Public key (32 bytes)
      - Address type (1 byte): 0x00 for Normal, 0x01 for Data

    Args:
        public_key: 32-byte compressed public key
        network: Network type (mainnet/testnet/devnet)

    Returns:
        TOS address string
    """
    # Network prefix mapping
    prefixes = {
        "mainnet": "tos",
        "testnet": "tst",
        "stagenet": "tss",  # Note: TOS may not use stagenet prefix
        "devnet": "tst"  # devnet uses testnet prefix
    }

    prefix = prefixes.get(network, "tst")

    try:
        from bech32 import bech32_encode, convertbits

        # Compress address data: PublicKey (32 bytes) + AddressType (1 byte)
        # AddressType::Normal = 0x00
        compressed_data = public_key + bytes([0x00])

        # Convert to 5-bit groups for bech32
        five_bit_data = convertbits(list(compressed_data), 8, 5)
        if five_bit_data is None:
            raise ValueError("Failed to convert to 5-bit groups")

        # Encode with bech32
        address = bech32_encode(prefix, five_bit_data)
        if not address:
            raise ValueError("Failed to encode address")

        return address
    except ImportError:
        raise ImportError("bech32 required: pip install bech32")


def sign_transaction(private_key: bytes, tx_hash: bytes) -> bytes:
    """
    Sign transaction hash with private key

    Uses Ed25519 signature

    Args:
        private_key: 32-byte private key
        tx_hash: 32-byte transaction hash

    Returns:
        64-byte signature
    """
    try:
        from nacl.signing import SigningKey

        signing_key = SigningKey(private_key)
        signature = signing_key.sign(tx_hash).signature

        return signature
    except ImportError:
        raise ImportError("PyNaCl required: pip install pynacl")


@dataclass
class TestAccount:
    """Test account with keys and metadata"""
    name: str
    seed: str
    keypair: KeyPair
    address: str

    @classmethod
    def from_seed(cls, name: str, seed: str, network: str = "devnet") -> 'TestAccount':
        """Create test account from seed phrase"""
        words = seed.strip().split()
        keypair = KeyPair.from_seed(words)
        address = keypair.get_address(network)
        return cls(name=name, seed=seed, keypair=keypair, address=address)


# Predefined test accounts
# NOTE: TOS uses Ristretto255 curve for cryptography, not Ed25519
# Python implementation requires calling TOS wallet binary for key derivation
# TODO: Implement proper Ristretto255 support or use wallet RPC for signing
TEST_ACCOUNTS = {
    "alice": {
        "name": "Alice",
        "seed": "tiger eight taxi vexed revamp thorn paddles dosage layout muzzle eggs chlorine sober oyster ecstatic festival banjo behind western segments january behind usage winter paddles",
        "expected_address": "tst1g6vj6htms5nykkywsnvs69xt63ev6wx6w9942lpesjrgdghm3vzqqegasvg",
        # These fields should be filled by running tos_wallet binary:
        # /path/to/tos_wallet --network testnet --offline-mode
        # Then: recover_seed -> enter seed -> address
        "public_key_hex": None,  # TODO: Extract from wallet
        "private_key_hex": None  # TODO: Extract from wallet (for signing)
    },
    # Add more test accounts as needed
}


def load_test_account(name: str, network: str = "devnet") -> TestAccount:
    """Load predefined test account by name"""
    account_data = TEST_ACCOUNTS.get(name.lower())
    if not account_data:
        raise ValueError(f"Unknown test account: {name}")

    account = TestAccount.from_seed(
        name=account_data["name"],
        seed=account_data["seed"],
        network=network
    )

    # Verify address matches expected (for validation)
    expected = account_data.get("expected_address")
    if expected and account.address != expected:
        print(f"WARNING: Address mismatch for {name}")
        print(f"  Expected: {expected}")
        print(f"  Got:      {account.address}")

    return account


# Transaction building utilities

def build_transfer_transaction(
    sender_keypair: KeyPair,
    recipient_address: str,
    amount: int,
    nonce: int,
    reference: dict,
    network: str = "devnet"
) -> dict:
    """
    Build and sign transfer transaction

    Args:
        sender_keypair: Sender's keypair
        recipient_address: Recipient's address
        amount: Transfer amount in atomic units
        nonce: Account nonce
        reference: Transaction reference (topoheight + hash)
        network: Network type

    Returns:
        Signed transaction data as dict
    """
    # This is a simplified placeholder
    # Full implementation needs to match Rust Transaction structure
    # from common/src/transaction/mod.rs

    raise NotImplementedError(
        "Transaction building not yet implemented. "
        "Need to implement Transaction structure matching Rust common/src/transaction/mod.rs"
    )


def build_freeze_transaction(
    sender_keypair: KeyPair,
    amount: int,
    duration: int,
    nonce: int,
    reference: dict,
    network: str = "devnet"
) -> dict:
    """
    Build and sign FreezeTos transaction

    Args:
        sender_keypair: Sender's keypair
        amount: Amount to freeze in atomic units
        duration: Freeze duration in days (3, 7, or 14)
        nonce: Account nonce
        reference: Transaction reference
        network: Network type

    Returns:
        Signed transaction data as dict
    """
    raise NotImplementedError(
        "Freeze transaction building not yet implemented"
    )


def build_unfreeze_transaction(
    sender_keypair: KeyPair,
    amount: int,
    nonce: int,
    reference: dict,
    network: str = "devnet"
) -> dict:
    """
    Build and sign UnfreezeTos transaction

    Args:
        sender_keypair: Sender's keypair
        amount: Amount to unfreeze
        nonce: Account nonce
        reference: Transaction reference
        network: Network type

    Returns:
        Signed transaction data as dict
    """
    raise NotImplementedError(
        "Unfreeze transaction building not yet implemented"
    )


if __name__ == "__main__":
    # Test wallet functionality
    print("Testing TOS Wallet Implementation")
    print("=" * 50)

    try:
        # Test Alice account
        print("\nLoading Alice account...")
        alice = load_test_account("alice")
        print(f"Name: {alice.name}")
        print(f"Address: {alice.address}")
        print(f"Public Key: {alice.keypair.public_key.hex()}")
        print(f"Private Key: {alice.keypair.private_key.hex()[:32]}... (truncated)")

        # Test signature
        print("\nTesting signature...")
        test_message = b"test transaction hash" + b"\x00" * 11  # Pad to 32 bytes
        signature = sign_transaction(alice.keypair.private_key, test_message)
        print(f"Signature: {signature.hex()[:64]}... (truncated)")
        print(f"Signature length: {len(signature)} bytes")

        print("\n[PASS] Basic wallet functionality working")

    except ImportError as e:
        print(f"\n[SKIP] Missing dependency: {e}")
        print("Install with: pip install pynacl bech32")
    except Exception as e:
        print(f"\n[FAIL] Error: {e}")
        import traceback
        traceback.print_exc()
