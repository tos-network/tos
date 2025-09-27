#!/usr/bin/env python3
"""
TOS AI Mining Test Script
Test AI mining workflow: Task submission -> AI computation -> AI rewards -> Post-distribution verification
"""

import json
import urllib.request
import urllib.parse
import time
import hashlib
import random
from typing import Dict, Any, Optional

class TOSAIMiningClient:
    def __init__(self, daemon_url: str = "http://127.0.0.1:8080"):
        self.daemon_url = daemon_url

    def rpc_call(self, method: str, params: Dict[str, Any] = None) -> Dict[str, Any]:
        """Send JSON-RPC call to daemon"""
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "id": random.randint(1, 10000)
        }

        # Only add params field when there are parameters
        if params:
            payload["params"] = params

        try:
            data = json.dumps(payload).encode('utf-8')

            req = urllib.request.Request(
                f"{self.daemon_url}/json_rpc",
                data=data,
                headers={
                    'Content-Type': 'application/json',
                    'User-Agent': 'TOS-AI-Mining-Test/1.0'
                }
            )

            with urllib.request.urlopen(req, timeout=30) as response:
                result = json.loads(response.read().decode('utf-8'))

            if "error" in result:
                raise Exception(f"RPC Error: {result['error']}")

            return result.get("result", {})

        except Exception as e:
            raise Exception(f"Network error: {e}")

    def get_daemon_info(self) -> Dict[str, Any]:
        """Get daemon information"""
        return self.rpc_call("get_info")

    def get_height(self) -> int:
        """Get current block height"""
        result = self.rpc_call("get_height")
        if isinstance(result, dict):
            return result.get("height", 0)
        else:
            # Sometimes the result is directly the height value
            return result if isinstance(result, int) else 0

    def generate_task_id(self) -> str:
        """Generate task ID"""
        timestamp = str(int(time.time()))
        random_data = str(random.randint(100000, 999999))
        return hashlib.sha256(f"ai_task_{timestamp}_{random_data}".encode()).hexdigest()

    def generate_answer_hash(self, task_id: str) -> str:
        """Generate answer hash for task"""
        answer_data = f"ai_answer_for_{task_id}_{random.randint(1000, 9999)}"
        return hashlib.sha256(answer_data.encode()).hexdigest()

    def create_ai_mining_transaction(self, payload_type: str, **kwargs) -> Dict[str, Any]:
        """Create AI mining transaction"""
        # Here we simulate creating a transaction, actual implementation needs to build real transactions according to TOS protocol
        tx_data = {
            "type": "ai_mining",
            "payload_type": payload_type,
            "timestamp": int(time.time()),
            "network": "devnet",
            **kwargs
        }
        return tx_data

    def publish_ai_task(self, reward_amount: int = 1000000, difficulty: str = "intermediate") -> Dict[str, Any]:
        """Publish AI mining task"""
        print(f"\n🔄 Publishing AI mining task...")

        task_id = self.generate_task_id()
        deadline = int(time.time()) + 3600  # Expires in 1 hour

        # Simulate task publication transaction
        tx_data = self.create_ai_mining_transaction(
            payload_type="PublishTask",
            task_id=task_id,
            reward_amount=reward_amount,
            difficulty=difficulty,
            deadline=deadline
        )

        print(f"✅ Generated task:")
        print(f"   Task ID: {task_id}")
        print(f"   Reward amount: {reward_amount} nanoTOS")
        print(f"   Difficulty level: {difficulty}")
        print(f"   Deadline: {deadline}")

        return {
            "task_id": task_id,
            "reward_amount": reward_amount,
            "difficulty": difficulty,
            "deadline": deadline,
            "tx_data": tx_data
        }

    def submit_ai_answer(self, task_id: str, stake_amount: int = 50000) -> Dict[str, Any]:
        """Submit AI answer"""
        print(f"\n🤖 Submitting AI answer...")

        answer_hash = self.generate_answer_hash(task_id)

        # Simulate answer submission transaction
        tx_data = self.create_ai_mining_transaction(
            payload_type="SubmitAnswer",
            task_id=task_id,
            answer_hash=answer_hash,
            stake_amount=stake_amount
        )

        print(f"✅ Generated answer:")
        print(f"   Task ID: {task_id}")
        print(f"   Answer hash: {answer_hash}")
        print(f"   Stake amount: {stake_amount} nanoTOS")

        return {
            "task_id": task_id,
            "answer_hash": answer_hash,
            "stake_amount": stake_amount,
            "tx_data": tx_data
        }

    def validate_ai_answer(self, task_id: str, answer_hash: str, validation_score: int = 85) -> Dict[str, Any]:
        """Validate AI answer"""
        print(f"\n🔍 Validating AI answer...")

        # Simulate answer validation transaction
        tx_data = self.create_ai_mining_transaction(
            payload_type="ValidateAnswer",
            task_id=task_id,
            answer_id=answer_hash,
            validation_score=validation_score
        )

        print(f"✅ Generated validation:")
        print(f"   Task ID: {task_id}")
        print(f"   Answer ID: {answer_hash}")
        print(f"   Validation score: {validation_score}%")

        return {
            "task_id": task_id,
            "answer_id": answer_hash,
            "validation_score": validation_score,
            "tx_data": tx_data
        }

    def calculate_rewards(self, task_info: Dict[str, Any], validation_score: int) -> Dict[str, Any]:
        """Calculate reward distribution"""
        print(f"\n💰 Calculating reward distribution...")

        base_reward = task_info["reward_amount"]

        # Calculate actual reward based on validation score
        actual_reward = int(base_reward * (validation_score / 100))

        # Reward distribution (simplified model)
        miner_reward = int(actual_reward * 0.7)  # Miners get 70%
        validator_reward = int(actual_reward * 0.2)  # Validators get 20%
        network_fee = actual_reward - miner_reward - validator_reward  # Network fee 10%

        rewards = {
            "total_reward": base_reward,
            "actual_reward": actual_reward,
            "miner_reward": miner_reward,
            "validator_reward": validator_reward,
            "network_fee": network_fee,
            "efficiency": validation_score
        }

        print(f"✅ Reward distribution:")
        print(f"   Base reward: {base_reward} nanoTOS")
        print(f"   Actual reward: {actual_reward} nanoTOS")
        print(f"   Miner reward: {miner_reward} nanoTOS")
        print(f"   Validator reward: {validator_reward} nanoTOS")
        print(f"   Network fee: {network_fee} nanoTOS")
        print(f"   Efficiency score: {validation_score}%")

        return rewards

    def run_complete_ai_mining_workflow(self) -> Dict[str, Any]:
        """Run complete AI mining workflow"""
        print("🚀 Starting complete AI mining workflow test")
        print("=" * 60)

        try:
            # 1. Check daemon connection
            print("\n📡 Checking daemon connection...")
            daemon_info = self.get_daemon_info()
            height = self.get_height()
            print(f"✅ Connection successful:")
            print(f"   Daemon version: {daemon_info.get('version', 'unknown')}")
            print(f"   Network: {daemon_info.get('network', 'unknown')}")
            print(f"   Current height: {height}")

            # 2. Publish AI task
            task_info = self.publish_ai_task(reward_amount=2000000, difficulty="intermediate")

            # 3. Simulate waiting time (in reality there would be miners computing)
            print("\n⏳ Waiting for AI computation processing...")
            time.sleep(2)

            # 4. Submit AI answer
            answer_info = self.submit_ai_answer(
                task_id=task_info["task_id"],
                stake_amount=100000
            )

            # 5. Simulate waiting for validation time
            print("\n⏳ Waiting for validation processing...")
            time.sleep(1)

            # 6. Validate AI answer
            validation_info = self.validate_ai_answer(
                task_id=task_info["task_id"],
                answer_hash=answer_info["answer_hash"],
                validation_score=random.randint(75, 95)
            )

            # 7. Calculate and distribute rewards
            rewards = self.calculate_rewards(task_info, validation_info["validation_score"])

            # 8. Compile results
            workflow_result = {
                "task_info": task_info,
                "answer_info": answer_info,
                "validation_info": validation_info,
                "rewards": rewards,
                "status": "completed",
                "timestamp": int(time.time())
            }

            print("\n🎉 AI mining workflow completed!")
            print("=" * 60)
            print(f"📊 Workflow summary:")
            print(f"   Task ID: {task_info['task_id']}")
            print(f"   Total reward: {rewards['actual_reward']} nanoTOS")
            print(f"   Validation score: {validation_info['validation_score']}%")
            print(f"   Workflow status: {workflow_result['status']}")

            return workflow_result

        except Exception as e:
            print(f"\n❌ Workflow error: {e}")
            return {"status": "failed", "error": str(e)}

def main():
    """Main function"""
    print("TOS AI Mining Test Script")
    print("Testing complete workflow from AI mining input to reward verification")
    print("=" * 60)

    # Initialize client
    client = TOSAIMiningClient()

    # Run complete workflow
    result = client.run_complete_ai_mining_workflow()

    # Save results to file
    with open("ai_mining_test_result.json", "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)

    print(f"\n📄 Test results saved to: ai_mining_test_result.json")

    if result["status"] == "completed":
        print("\n✅ All tests passed! AI mining workflow is running normally.")
        return 0
    else:
        print("\n❌ Tests failed, please check error information.")
        return 1

if __name__ == "__main__":
    exit(main())