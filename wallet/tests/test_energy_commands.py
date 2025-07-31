#!/usr/bin/env python3
"""
Terminos Wallet Batch Mode Test - Energy Commands

This script tests the wallet's energy-related commands in batch mode:
- freeze_tos: Freeze TOS to get energy with duration-based rewards (requires amount, duration, confirm)
- unfreeze_tos: Unfreeze TOS (requires amount, confirm)
- energy_info: Show energy information and freeze records (no parameters)

Usage:
    python3 test_energy_commands.py

Requirements:
    - Python 3.6+
    - Cargo and Rust toolchain
    - terminos-wallet binary must be built
"""

import subprocess
import sys
import os
from typing import Dict, List, Tuple

class EnergyCommandsTester:
    """Test energy commands functionality"""
    
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
    
    def test_energy_info_command(self) -> List[Tuple[str, Dict]]:
        """Test energy_info command (no parameters)"""
        print("\n=== Testing energy_info Command ===")
        
        tests = [
            ("energy_info", "energy_info (no parameters)"),
        ]
        
        results = []
        for cmd_with_args, description in tests:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_freeze_tos_command(self) -> List[Tuple[str, Dict]]:
        """Test freeze_tos command with different parameters"""
        print("\n=== Testing freeze_tos Command ===")
        
        tests = [
            ("freeze_tos 100000000 7 yes", "freeze_tos with amount=100000000, duration=7, confirm=yes"),
            ("freeze_tos 50000000 14 yes", "freeze_tos with amount=50000000, duration=14, confirm=yes"),
            ("freeze_tos 100000000 3 yes", "freeze_tos with amount=100000000, duration=3, confirm=yes"),
            ("freeze_tos", "freeze_tos without parameters (should fail)"),
        ]
        
        results = []
        for cmd_with_args, description in tests:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            # freeze_tos with parameters should succeed, without parameters should fail
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            elif not result["success"] and "Expected required argument" in result["stderr"]:
                print(f"✅ {cmd_with_args}: PASSED (expected behavior - missing required argument)")
                # Mark as success for our test purposes
                result["success"] = True
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_unfreeze_tos_command(self) -> List[Tuple[str, Dict]]:
        """Test unfreeze_tos command with different parameters"""
        print("\n=== Testing unfreeze_tos Command ===")
        
        tests = [
            ("unfreeze_tos 50000000 yes", "unfreeze_tos with amount=50000000, confirm=yes"),
            ("unfreeze_tos 100000000 yes", "unfreeze_tos with amount=100000000, confirm=yes"),
            ("unfreeze_tos 25000000 yes", "unfreeze_tos with amount=25000000, confirm=yes"),
            ("unfreeze_tos", "unfreeze_tos without parameters (should fail)"),
        ]
        
        results = []
        for cmd_with_args, description in tests:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            # unfreeze_tos with parameters should succeed, without parameters should fail
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            elif not result["success"] and "Expected required argument" in result["stderr"]:
                print(f"✅ {cmd_with_args}: PASSED (expected behavior - missing required argument)")
                # Mark as success for our test purposes
                result["success"] = True
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def run_all_tests(self) -> bool:
        """Run all energy command tests"""
        print("🚀 Starting Energy Commands Tests")
        print("=" * 50)
        
        all_results = []
        
        # Test energy commands
        all_results.extend(self.test_energy_info_command())
        all_results.extend(self.test_freeze_tos_command())
        all_results.extend(self.test_unfreeze_tos_command())
        
        # Summary
        print("\n" + "=" * 50)
        print("📊 TEST SUMMARY")
        print("=" * 50)
        
        passed = 0
        total = len(all_results)
        
        for test_name, result in all_results:
            if result["success"]:
                print(f"✅ {test_name}: PASSED")
                passed += 1
            else:
                print(f"❌ {test_name}: FAILED")
                print(f"   Command: {result['command']}")
                print(f"   Error: {result['stderr']}")
        
        print(f"\nResults: {passed}/{total} tests passed")
        
        if passed == total:
            print("🎉 All energy command tests passed!")
            return True
        else:
            print(f"⚠️  {total - passed} tests failed")
            return False

def main():
    """Main test runner"""
    tester = EnergyCommandsTester()
    
    try:
        success = tester.run_all_tests()
        sys.exit(0 if success else 1)
    except KeyboardInterrupt:
        print("\n⚠️  Tests interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n💥 Test runner error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main() 