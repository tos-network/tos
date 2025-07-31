#!/usr/bin/env python3
"""
Terminos Wallet Batch Mode Test - Basic Commands

This script tests the wallet's basic commands in batch mode:
- help: Show this help (no parameters)
- version: Show the current version (no parameters)
- exit: Shutdown the application (no parameters)
- set_log_level: Set the log level (requires level parameter)

Usage:
    python3 test_basic_commands.py

Requirements:
    - Python 3.6+
    - Cargo and Rust toolchain
    - terminos-wallet binary must be built
"""

import subprocess
import sys
import os
from typing import Dict, List, Tuple

class BasicCommandsTester:
    """Test basic commands functionality"""
    
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
    
    def test_help_command(self) -> List[Tuple[str, Dict]]:
        """Test help command (no parameters)"""
        print("\n=== Testing help Command ===")
        
        tests = [
            ("help", "help (no parameters)"),
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
    
    def test_version_command(self) -> List[Tuple[str, Dict]]:
        """Test version command (no parameters)"""
        print("\n=== Testing version Command ===")
        
        tests = [
            ("version", "version (no parameters)"),
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
    
    def test_exit_command(self) -> List[Tuple[str, Dict]]:
        """Test exit command (no parameters)"""
        print("\n=== Testing exit Command ===")
        
        tests = [
            ("exit", "exit (no parameters)"),
        ]
        
        results = []
        for cmd_with_args, description in tests:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            # Exit command is expected to fail (exit code 1) as it terminates the program
            if not result["success"] and "Exit command was called" in result["stderr"]:
                print(f"✅ {cmd_with_args}: PASSED (expected behavior)")
                # Mark as success for our test purposes
                result["success"] = True
            elif result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_set_log_level_command(self) -> List[Tuple[str, Dict]]:
        """Test set_log_level command with different levels"""
        print("\n=== Testing set_log_level Command ===")
        
        tests = [
            ("set_log_level info", "set_log_level with info"),
            ("set_log_level debug", "set_log_level with debug"),
            ("set_log_level warn", "set_log_level with warn"),
            ("set_log_level error", "set_log_level with error"),
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
        """Run all basic command tests"""
        print("🚀 Starting Basic Commands Tests")
        print("=" * 50)
        
        all_results = []
        
        # Test basic commands
        all_results.extend(self.test_help_command())
        all_results.extend(self.test_version_command())
        all_results.extend(self.test_exit_command())
        all_results.extend(self.test_set_log_level_command())
        
        # Summary
        print("\n" + "=" * 50)
        print("📊 TEST SUMMARY")
        print("=" * 50)
        
        passed = 0
        total = len(all_results)
        
        for cmd, result in all_results:
            if result["success"]:
                passed += 1
        
        print(f"Passed: {passed}/{total}")
        print(f"Failed: {total - passed}/{total}")
        
        if passed == total:
            print("🎉 All basic command tests passed!")
            return True
        else:
            print("⚠️  Some basic command tests failed!")
            return False

def main():
    """Main test runner"""
    tester = BasicCommandsTester()
    
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