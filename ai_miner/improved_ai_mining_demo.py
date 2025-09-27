#!/usr/bin/env python3
"""
TOS AI Mining Improved Economic Model Demo
Improved AI mining economic model demo - solving reward/cost ratio issues
"""

import json
import urllib.request
import time
import hashlib
import random
import sys
from datetime import datetime

class ImprovedTOSAIMiningDemo:
    def __init__(self, daemon_url: str = "http://127.0.0.1:8080"):
        self.daemon_url = daemon_url
        self.task_publisher_id = "publisher_001"
        self.miner_id = "miner_001"
        self.validator_id = "validator_001"

    def rpc_call(self, method: str, params: dict = None) -> dict:
        """Call RPC method"""
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "id": random.randint(1, 10000)
        }
        if params:
            payload["params"] = params

        data = json.dumps(payload).encode('utf-8')
        req = urllib.request.Request(
            f"{self.daemon_url}/json_rpc",
            data=data,
            headers={'Content-Type': 'application/json'}
        )

        try:
            with urllib.request.urlopen(req, timeout=30) as response:
                result = json.loads(response.read().decode('utf-8'))

            if "error" in result:
                print(f"❌ RPC Error: {result['error']}")
                return {}
            return result.get("result", {})
        except Exception as e:
            print(f"❌ Network Error: {e}")
            return {}

    def calculate_improved_gas_pricing(self, content_length: int, difficulty: str) -> dict:
        """Improved gas fee calculation"""

        # Base fee
        base_fee = 2500  # 2500 nanoTOS

        # Difficulty multipliers
        difficulty_multipliers = {
            "Basic": 1.0,
            "Intermediate": 1.5,
            "Advanced": 2.0,
            "Expert": 3.0
        }
        difficulty_multiplier = difficulty_multipliers.get(difficulty, 1.0)

        # Improved content storage pricing model
        if content_length <= 100:
            # Short content: free storage to encourage participation
            content_cost = 0
        elif content_length <= 500:
            # Medium content: reduced fees
            content_cost = (content_length - 100) * 100_000  # 0.0001 TOS/byte
        else:
            # Long content: tiered pricing
            content_cost = (
                400 * 100_000 +  # First 500 bytes: 400 * 0.0001 TOS
                (content_length - 500) * 50_000  # Additional: 0.00005 TOS/byte
            )

        total_gas = int(base_fee + content_cost * difficulty_multiplier)

        return {
            "base_fee": base_fee,
            "content_cost": content_cost,
            "difficulty_multiplier": difficulty_multiplier,
            "total_gas": total_gas
        }

    def calculate_improved_rewards(self, difficulty: str, validation_score: int) -> dict:
        """Improved reward calculation model"""

        # Difficulty-based base rewards (significantly increased)
        base_rewards = {
            "Basic": 50_000_000,      # 0.05 TOS
            "Intermediate": 200_000_000,  # 0.2 TOS
            "Advanced": 500_000_000,   # 0.5 TOS
            "Expert": 1_000_000_000    # 1.0 TOS
        }

        base_reward = base_rewards.get(difficulty, 50_000_000)

        # Quality reward multiplier
        quality_multiplier = validation_score / 100.0

        # Scarcity bonus (additional rewards for high-quality answers)
        scarcity_bonus = 1.0
        if validation_score >= 90:
            scarcity_bonus = 1.5  # 50% additional reward
        elif validation_score >= 80:
            scarcity_bonus = 1.2  # 20% additional reward

        actual_reward = int(base_reward * quality_multiplier * scarcity_bonus)

        return {
            "base_reward": base_reward,
            "quality_multiplier": quality_multiplier,
            "scarcity_bonus": scarcity_bonus,
            "actual_reward": actual_reward
        }

    def run_improved_demo(self):
        """Run improved economic model demonstration"""
        print("🚀 TOS AI Mining - Improved Economic Model Demo")
        print("💡 Improved solution to address reward/cost ratio issues")
        print("⏱️ Start time:", datetime.now().strftime("%Y-%m-%d %H:%M:%S"))

        # Check daemon connection
        print("\n🔌 Checking TOS daemon connection...")
        daemon_info = self.rpc_call("get_info")
        if daemon_info:
            print(f"✅ Connection successful: TOS {daemon_info.get('version', 'Unknown')} ({daemon_info.get('network', 'Unknown')})")
        else:
            print("❌ Unable to connect to TOS daemon, continuing with simulation...")

        print("\n" + "="*60)
        print("📊 Economic Model Comparison Analysis")
        print("="*60)

        # Test scenarios with different difficulties and content lengths
        scenarios = [
            {
                "name": "Basic Task - Short Answer",
                "difficulty": "Basic",
                "task_description_length": 80,
                "answer_content_length": 150,
                "validation_score": 85
            },
            {
                "name": "Intermediate Task - Medium Answer",
                "difficulty": "Intermediate",
                "task_description_length": 300,
                "answer_content_length": 600,
                "validation_score": 92
            },
            {
                "name": "Advanced Task - Detailed Answer",
                "difficulty": "Advanced",
                "task_description_length": 500,
                "answer_content_length": 1200,
                "validation_score": 88
            },
            {
                "name": "Expert Task - Complete Report",
                "difficulty": "Expert",
                "task_description_length": 800,
                "answer_content_length": 2000,
                "validation_score": 95
            }
        ]

        print("\n📋 Scenario Comparison Analysis:")
        print("-" * 120)
        print(f"{'Scenario':<20} {'Difficulty':<12} {'Content Len':<12} {'Score':<8} {'Old Cost':<15} {'Old Reward':<15} {'Old Ratio':<12} {'New Cost':<15} {'New Reward':<15} {'New Ratio':<12}")
        print("-" * 120)

        total_old_cost = 0
        total_old_reward = 0
        total_new_cost = 0
        total_new_reward = 0

        for scenario in scenarios:
            name = scenario["name"]
            difficulty = scenario["difficulty"]
            task_len = scenario["task_description_length"]
            answer_len = scenario["answer_content_length"]
            score = scenario["validation_score"]

            # Old model calculation (current v1.1.0)
            old_task_cost = 2500 + task_len * 1_000_000
            old_answer_cost = 1875 + answer_len * 1_000_000
            old_validation_cost = 2187
            old_total_cost = old_task_cost + old_answer_cost + old_validation_cost

            old_base_reward = 2_000_000  # Fixed 2M nanoTOS
            old_actual_reward = int(old_base_reward * score / 100.0)

            old_ratio = old_actual_reward / old_total_cost if old_total_cost > 0 else 0

            # New model calculation
            new_task_gas = self.calculate_improved_gas_pricing(task_len, difficulty)
            new_answer_gas = self.calculate_improved_gas_pricing(answer_len, difficulty)
            new_validation_cost = 2187
            new_total_cost = new_task_gas["total_gas"] + new_answer_gas["total_gas"] + new_validation_cost

            new_reward_calc = self.calculate_improved_rewards(difficulty, score)
            new_actual_reward = new_reward_calc["actual_reward"]

            new_ratio = new_actual_reward / new_total_cost if new_total_cost > 0 else 0

            print(f"{name:<20} {difficulty:<12} {task_len+answer_len:<12} {score:<8} {old_total_cost:<15,} {old_actual_reward:<15,} {old_ratio:<12.3f} {new_total_cost:<15,} {new_actual_reward:<15,} {new_ratio:<12.3f}")

            total_old_cost += old_total_cost
            total_old_reward += old_actual_reward
            total_new_cost += new_total_cost
            total_new_reward += new_actual_reward

        print("-" * 120)
        overall_old_ratio = total_old_reward / total_old_cost if total_old_cost > 0 else 0
        overall_new_ratio = total_new_reward / total_new_cost if total_new_cost > 0 else 0

        print(f"{'Total':<20} {'':<12} {'':<12} {'':<8} {total_old_cost:<15,} {total_old_reward:<15,} {overall_old_ratio:<12.3f} {total_new_cost:<15,} {total_new_reward:<15,} {overall_new_ratio:<12.3f}")

        print("\n" + "="*60)
        print("💡 Detailed Improvement Plan")
        print("="*60)

        print("🔧 **1. Tiered Content Storage Pricing**")
        print("   - First 100 bytes: Free (encourage participation)")
        print("   - 100-500 bytes: 0.0001 TOS/byte (10x reduction)")
        print("   - 500+ bytes: 0.00005 TOS/byte (20x reduction)")

        print("\n🎯 **2. Difficulty-based Reward Model**")
        print("   - Basic tasks: 0.05 TOS (25x increase)")
        print("   - Intermediate tasks: 0.2 TOS (100x increase)")
        print("   - Advanced tasks: 0.5 TOS (250x increase)")
        print("   - Expert tasks: 1.0 TOS (500x increase)")

        print("\n⭐ **3. Quality Reward Multiplier**")
        print("   - 90%+ score: +50% scarcity bonus")
        print("   - 80%+ score: +20% scarcity bonus")
        print("   - Base score: proportional rewards")

        print("\n📈 **4. Economic Impact Comparison**")
        print(f"   - Old model average reward ratio: {overall_old_ratio:.3f}x (severe loss)")
        print(f"   - New model average reward ratio: {overall_new_ratio:.3f}x (profitable)")
        print(f"   - Improvement factor: {overall_new_ratio/overall_old_ratio:.1f}x enhancement")

        print("\n🎯 **5. Implementation Recommendations**")
        print("   - Implement new pricing model immediately")
        print("   - Dynamically adjust based on network usage")
        print("   - Establish reward fund pool for sustainability")
        print("   - Introduce reputation system to further incentivize quality contributions")

        improvement_factor = overall_new_ratio / overall_old_ratio if overall_old_ratio > 0 else float('inf')

        print(f"\n✅ **Summary**: The new model improves the reward/cost ratio by {improvement_factor:.1f}x,")
        print(f"    from {overall_old_ratio:.3f}x to {overall_new_ratio:.3f}x,")
        print(f"    making AI mining a truly economically incentivized activity!")

def main():
    """Main function"""
    demo = ImprovedTOSAIMiningDemo()
    demo.run_improved_demo()

if __name__ == "__main__":
    main()