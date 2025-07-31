#!/usr/bin/env python3
"""
Terminos Wallet Batch Mode Test - List Commands

This script tests the wallet's list commands in batch mode:
- list_balances: List all balances tracked (optional page parameter)
- list_assets: List all detected assets (optional page parameter)
- list_tracked_assets: List all assets marked as tracked (optional page parameter)

Usage:
    python3 test_list_commands.py

Requirements:
    - Python 3.6+
    - Cargo and Rust toolchain
    - terminos-wallet binary must be built
"""

import subprocess
import sys
import os
from typing import Dict, List, Tuple

class ListCommandsTester:
    """Test list commands functionality"""
    
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
    
    def test_list_balances(self) -> List[Tuple[str, Dict]]:
        """Test list_balances command with different page parameters"""
        print("\n=== Testing list_balances Command ===")
        
        tests = [
            ("list_balances 1", "list_balances with page=1"),
            ("list_balances 0", "list_balances with page=0"),
            ("list_balances", "list_balances without page parameter"),
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
    
    def test_list_assets(self) -> List[Tuple[str, Dict]]:
        """Test list_assets command with different page parameters"""
        print("\n=== Testing list_assets Command ===")
        
        tests = [
            ("list_assets 1", "list_assets with page=1"),
            ("list_assets 0", "list_assets with page=0"),
            ("list_assets", "list_assets without page parameter"),
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
    
    def test_list_tracked_assets(self) -> List[Tuple[str, Dict]]:
        """Test list_tracked_assets command with different page parameters"""
        print("\n=== Testing list_tracked_assets Command ===")
        
        tests = [
            ("list_tracked_assets 1", "list_tracked_assets with page=1"),
            ("list_tracked_assets 0", "list_tracked_assets with page=0"),
            ("list_tracked_assets", "list_tracked_assets without page parameter"),
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
    
    def run_all_tests(self) -> bool:
        """Run all list command tests"""
        print("🚀 Starting List Commands Tests")
        print("=" * 50)
        
        all_results = []
        
        # Test list commands
        all_results.extend(self.test_list_balances())
        all_results.extend(self.test_list_assets())
        all_results.extend(self.test_list_tracked_assets())
        
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
            print("🎉 All list command tests passed!")
            return True
        else:
            print(f"⚠️  {total - passed} tests failed")
            return False

def main():
    """Main test runner"""
    tester = ListCommandsTester()
    
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