#!/usr/bin/env python3
"""
Extract test account keys from TOS wallet

This script helps generate test accounts by:
1. Creating wallet files from seeds
2. Extracting addresses (from wallet display)
3. Documenting keys for TEST_ACCOUNTS

Since TOS uses Ristretto255 (no Python library), we use the wallet binary.
"""

import subprocess
import sys
import os
import json
import time

WALLET_BIN = "/Users/tomisetsu/tos-network/tos/target/release/tos_wallet"
NETWORK = "testnet"
WALLET_DIR = "/tmp/tos_test_wallets"

# Test account seeds
TEST_SEEDS = {
    "alice": "tiger eight taxi vexed revamp thorn paddles dosage layout muzzle eggs chlorine sober oyster ecstatic festival banjo behind western segments january behind usage winter paddles",
    "bob": "ocean swift mountain eagle dancing river frozen sunset golden meadow crystal palace harmony wisdom ancient forest keeper silver dragon mystic lunar phase",
    "charlie": "cosmic nebula stellar quantum photon aurora borealis cascade thunder lightning plasma fusion reactor galaxy spiral vortex infinite eternal cosmic ray burst"
}

def check_wallet_exists():
    """Check if wallet binary exists"""
    if not os.path.exists(WALLET_BIN):
        print(f"ERROR: Wallet binary not found at {WALLET_BIN}")
        print("Please build wallet first: cargo build --release --bin tos_wallet")
        sys.exit(1)
    print(f"[OK] Found wallet binary: {WALLET_BIN}")

def create_wallet_from_seed(name: str, seed: str, password: str = "test123"):
    """
    Create a wallet from seed using tos_wallet binary

    Returns the wallet path
    """
    wallet_path = os.path.join(WALLET_DIR, name)

    # Remove existing wallet
    if os.path.exists(wallet_path):
        import shutil
        shutil.rmtree(wallet_path)

    os.makedirs(wallet_path, exist_ok=True)

    print(f"\n=== Creating wallet for {name} ===")
    print(f"Wallet path: {wallet_path}")
    print(f"Seed: {seed[:50]}...")

    # Create a temporary input file with commands
    input_file = f"/tmp/wallet_input_{name}.txt"
    with open(input_file, 'w') as f:
        f.write("recover_seed\n")
        f.write(f"{seed}\n")
        f.write(f"{password}\n")
        f.write(f"{password}\n")
        f.write("address\n")
        f.write("balance\n")
        f.write("exit\n")

    # Run wallet with input
    try:
        result = subprocess.run(
            [
                WALLET_BIN,
                "--network", NETWORK,
                "--wallet-path", wallet_path,
                "--offline-mode",
                "--disable-ascii-art",
                "--disable-interactive-mode"
            ],
            stdin=open(input_file, 'r'),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            text=True
        )

        output = result.stdout + result.stderr

        # Extract address from output
        import re
        address_match = re.search(r'(tst1[a-z0-9]+)', output)
        if address_match:
            address = address_match.group(1)
            print(f"[OK] Address: {address}")
            return wallet_path, address
        else:
            print(f"[ERROR] Could not extract address from output")
            print(f"Output: {output[:500]}")
            return wallet_path, None

    except subprocess.TimeoutExpired:
        print(f"[ERROR] Wallet command timed out")
        return wallet_path, None
    except Exception as e:
        print(f"[ERROR] Failed to create wallet: {e}")
        return wallet_path, None
    finally:
        if os.path.exists(input_file):
            os.remove(input_file)

def manual_extraction_guide(name: str, seed: str):
    """Print manual extraction guide for a test account"""
    print(f"\n=== Manual Key Extraction for {name} ===")
    print(f"Run the following commands:\n")
    print(f"cd {WALLET_DIR}")
    print(f"{WALLET_BIN} --wallet-path {name} --network {NETWORK} --offline-mode\n")
    print(f"In the wallet prompt:")
    print(f"1. Type: recover_seed")
    print(f"2. Enter seed: {seed}")
    print(f"3. Enter password: test123")
    print(f"4. Confirm password: test123")
    print(f"5. Type: address       # Note the address")
    print(f"6. Type: balance       # Verify it works")
    print(f"7. Type: exit\n")
    print(f"Then update TEST_ACCOUNTS in lib/wallet.py with the address.")

def generate_all_accounts():
    """Generate all test accounts"""
    print("=== TOS Test Account Generator ===\n")

    check_wallet_exists()

    os.makedirs(WALLET_DIR, exist_ok=True)

    results = {}

    for name, seed in TEST_SEEDS.items():
        wallet_path, address = create_wallet_from_seed(name, seed)
        results[name] = {
            "seed": seed,
            "address": address,
            "wallet_path": wallet_path
        }

        if not address:
            # Fallback to manual guide
            manual_extraction_guide(name, seed)

    # Generate Python code for TEST_ACCOUNTS
    print("\n=== Generated TEST_ACCOUNTS Code ===\n")
    print("TEST_ACCOUNTS = {")
    for name, data in results.items():
        if data['address']:
            print(f'    "{name}": {{')
            print(f'        "name": "{name.capitalize()}",')
            print(f'        "seed": "{data["seed"]}",')
            print(f'        "address": "{data["address"]}",')
            print(f'        "wallet_path": "{data["wallet_path"]}"')
            print(f'    }},')
    print("}")

    print("\n=== Summary ===")
    for name, data in results.items():
        status = "[OK]" if data['address'] else "[MANUAL NEEDED]"
        print(f"{status} {name}: {data['address'] or 'See manual guide above'}")

    return results

if __name__ == "__main__":
    results = generate_all_accounts()
