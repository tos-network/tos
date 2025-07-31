#!/usr/bin/env python3
"""
Terminos Wallet Batch Mode Test - display_address Command

This script tests the wallet's display_address command in batch mode.
The display_address command shows the wallet's address without requiring parameters.

Usage:
    python3 test_display_address.py

Requirements:
    - Python 3.6+
    - Cargo and Rust toolchain
    - terminos-wallet binary must be built
"""

import subprocess
import sys
import os
from typing import Dict

class DisplayAddressTester:
    """Test display_address command functionality"""
    
    def __init__(self):
        self.wallet_binary = "../target/debug/terminos_wallet"
        self.wallet_name = "test_wallet_batch"
        self.wallet_password = "test123"
        
    def run_wallet_command(self, cmd_with_args: str, timeout: int = 30) -> Dict:
        """Run wallet command and return result"""
        try:
            cmd = [self.wallet_binary, "--batch-mode", "--cmd", cmd_with_args, "--wallet-path", self.wallet_name, "--password", self.wallet_password]
            
            print(f"Running: {' '.join(cmd)}")
            
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                cwd=os.getcwd()
            )
            
            return {
                "success": result.returncode == 0,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "returncode": result.returncode,
                "command": ' '.join(cmd)
            }
        except subprocess.TimeoutExpired:
            return {
                "success": False,
                "stdout": "",
                "stderr": "Command timed out",
                "returncode": -1,
                "command": cmd_with_args
            }
        except FileNotFoundError:
            return {
                "success": False,
                "stdout": "",
                "stderr": "Wallet binary not found",
                "returncode": -1,
                "command": cmd_with_args
            }
    
    def test_display_address(self) -> bool:
        """Test display_address command"""
        print("=== Testing display_address Command ===")
        
        print("\nTesting: display_address (no parameters)")
        result = self.run_wallet_command("display_address")
        
        if result["success"]:
            print(f"✅ display_address: PASSED")
            print(f"   Output: {result['stdout'].strip()}")
            return True
        else:
            print(f"❌ display_address: FAILED")
            print(f"   Error: {result['stderr']}")
            return False

def main():
    """Main test runner"""
    tester = DisplayAddressTester()
    
    try:
        success = tester.test_display_address()
        sys.exit(0 if success else 1)
    except KeyboardInterrupt:
        print("\n⚠️  Tests interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n💥 Test runner error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main() 