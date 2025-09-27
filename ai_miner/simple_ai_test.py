#!/usr/bin/env python3
"""
Simplified TOS AI Mining Test Script
"""

import json
import urllib.request
import time
import hashlib
import random

def test_daemon_connection():
    """Test daemon connection"""
    print("🔧 Testing daemon connection...")

    payload = {
        "jsonrpc": "2.0",
        "method": "get_info",
        "id": 1
    }

    try:
        data = json.dumps(payload).encode('utf-8')
        req = urllib.request.Request(
            "http://127.0.0.1:8080/json_rpc",
            data=data,
            headers={'Content-Type': 'application/json'}
        )

        with urllib.request.urlopen(req, timeout=10) as response:
            result = json.loads(response.read().decode('utf-8'))

        if "error" in result:
            print(f"❌ RPC error: {result['error']}")
            return False

        info = result.get("result", {})
        print(f"✅ Daemon connected successfully!")
        print(f"   Version: {info.get('version', 'unknown')}")
        print(f"   Network: {info.get('network', 'unknown')}")

        return True
    except Exception as e:
        print(f"❌ Connection failed: {e}")
        return False

def simulate_ai_mining_workflow():
    """Simulate AI mining workflow"""
    print("\n🚀 Simulating AI mining workflow")
    print("=" * 50)

    # 1. Generate task ID
    task_id = hashlib.sha256(f"ai_task_{int(time.time())}_{random.randint(1000, 9999)}".encode()).hexdigest()
    print(f"📝 Generated task ID: {task_id[:16]}...")

    # 2. Simulate task publication
    print("📤 Simulating AI mining task publication...")
    task_info = {
        "task_id": task_id,
        "reward_amount": 2000000,  # 2M nanoTOS
        "difficulty": "intermediate",
        "deadline": int(time.time()) + 3600  # 1 hour later
    }
    print(f"   Reward: {task_info['reward_amount']} nanoTOS")
    print(f"   Difficulty: {task_info['difficulty']}")

    time.sleep(1)

    # 3. Simulate AI computation
    print("🤖 Simulating AI computation processing...")
    answer_hash = hashlib.sha256(f"ai_answer_{task_id}_{random.randint(1000, 9999)}".encode()).hexdigest()
    print(f"   Generated answer hash: {answer_hash[:16]}...")

    time.sleep(1)

    # 4. Simulate answer validation
    print("🔍 Simulating answer validation...")
    validation_score = random.randint(75, 95)
    print(f"   Validation score: {validation_score}%")

    time.sleep(1)

    # 5. Calculate reward distribution
    print("💰 Calculating reward distribution...")
    base_reward = task_info["reward_amount"]
    actual_reward = int(base_reward * (validation_score / 100))
    miner_reward = int(actual_reward * 0.7)
    validator_reward = int(actual_reward * 0.2)
    network_fee = actual_reward - miner_reward - validator_reward

    rewards = {
        "base_reward": base_reward,
        "actual_reward": actual_reward,
        "miner_reward": miner_reward,
        "validator_reward": validator_reward,
        "network_fee": network_fee
    }

    print(f"   Base reward: {base_reward} nanoTOS")
    print(f"   Actual reward: {actual_reward} nanoTOS")
    print(f"   Miner reward: {miner_reward} nanoTOS")
    print(f"   Validator reward: {validator_reward} nanoTOS")
    print(f"   Network fee: {network_fee} nanoTOS")

    # 6. Compile results
    result = {
        "task_info": task_info,
        "answer_hash": answer_hash,
        "validation_score": validation_score,
        "rewards": rewards,
        "timestamp": int(time.time()),
        "status": "simulation_complete"
    }

    return result

def main():
    """Main function"""
    print("TOS AI Mining Simplified Test")
    print("=" * 50)

    # Test daemon connection
    if not test_daemon_connection():
        print("\n❌ Daemon connection failed, please ensure daemon is running")
        return 1

    # Run workflow simulation
    result = simulate_ai_mining_workflow()

    # Save results
    with open("simple_ai_test_result.json", "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)

    print(f"\n✅ AI mining workflow simulation completed!")
    print(f"📄 Results saved to: simple_ai_test_result.json")
    print(f"🏆 Final status: {result['status']}")

    return 0

if __name__ == "__main__":
    exit(main())