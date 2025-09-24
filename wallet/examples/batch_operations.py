#!/usr/bin/env python3
"""
TOS Wallet JSON batch operations - Python example
"""

import json
import subprocess
import tempfile
import os

class TOSWalletBatch:
    def __init__(self, wallet_path, password):
        self.wallet_path = wallet_path
        self.password = password
        self.base_cmd = [
            "tos_wallet",
            "--wallet-path", wallet_path,
            "--password", password
        ]

    def execute_exec_command(self, command):
        """Execute a command using --exec (simple)"""
        cmd = self.base_cmd + ["--exec", command]

        print(f"Executing --exec command: {command}")
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            print("Output:", result.stdout)
            return result.stdout
        except subprocess.CalledProcessError as e:
            print("Error:", e.stderr)
            return None

    def execute_json_command(self, command_config):
        """Execute a command using a JSON string"""
        json_str = json.dumps(command_config)
        cmd = self.base_cmd + ["--json", json_str]

        print(f"Executing JSON command: {command_config['command']}")
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            print("Output:", result.stdout)
            return result.stdout
        except subprocess.CalledProcessError as e:
            print("Error:", e.stderr)
            return None

    def execute_json_file(self, json_config):
        """Execute a command using a temporary JSON file"""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.json', delete=False) as f:
            json.dump(json_config, f, indent=2)
            temp_file = f.name

        cmd = self.base_cmd + ["--json-file", temp_file]

        print(f"Executing command: {json_config['command']}")
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            print("Output:", result.stdout)
            return result.stdout
        except subprocess.CalledProcessError as e:
            print("Error:", e.stderr)
            return None
        finally:
            os.unlink(temp_file)


def main():
    # Initialize wallet batch processor
    wallet = TOSWalletBatch("my_test_wallet", "test123")

    print("=== TOS Wallet Exec Mode Operations (Python) ===")

    # 1. Query balance - using --exec (simple)
    print("\n1. Query TOS balance (--exec)...")
    wallet.execute_exec_command("balance TOS")

    # 1a. Query balance - using JSON (structured)
    print("\n1a. Query TOS balance (JSON)...")
    balance_config = {
        "command": "balance",
        "params": {
            "asset": "TOS"
        }
    }
    wallet.execute_json_command(balance_config)

    # 2. Get address - using --exec (simple)
    print("\n2. Get wallet address (--exec)...")
    wallet.execute_exec_command("address")

    # 2a. Get address - using JSON (consistent)
    print("\n2a. Get wallet address (JSON)...")
    address_config = {
        "command": "address",
        "params": {}
    }
    wallet.execute_json_command(address_config)

    # 3. Set nonce - using --exec (simple)
    print("\n3. Set nonce to 200 (--exec)...")
    wallet.execute_exec_command("set_nonce 200")

    # 3a. Set nonce - using JSON file (structured)
    print("\n3a. Set nonce to 200 (JSON file)...")
    nonce_config = {
        "command": "set_nonce",
        "params": {
            "nonce": 200
        }
    }
    wallet.execute_json_file(nonce_config)

    # 4. Transfer - using --exec (one-liner)
    print("\n4. Transfer TOS (--exec)...")
    wallet.execute_exec_command("transfer TOS tos1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq8cczjp 0.5")

    # 4a. Transfer - using JSON file (complex configuration)
    print("\n4a. Transfer TOS (JSON file)...")
    transfer_config = {
        "command": "transfer",
        "params": {
            "address": "tos1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq8cczjp",
            "amount": "0.5",
            "asset": "TOS",
            "confirm": "yes"
        }
    }
    wallet.execute_json_file(transfer_config)

    print("\n=== Exec mode operations completed ===")


if __name__ == "__main__":
    main()