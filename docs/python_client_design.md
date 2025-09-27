# TOS AI Python Client Design

## 1. Main Client Class

```python
# tos_ai/client.py
import asyncio
import aiohttp
import json
from typing import Dict, List, Optional, AsyncGenerator
from datetime import datetime, timedelta

from .types import *
from .miner import AIMiner
from .validator import AIValidator
from .publisher import TaskPublisher

class TOSAIClient:
    """TOS AI Mining Python Client"""

    def __init__(self, rpc_url: str = "http://localhost:8545", wallet_path: Optional[str] = None):
        self.rpc_url = rpc_url
        self.wallet_path = wallet_path
        self.session: Optional[aiohttp.ClientSession] = None

        # Sub-modules
        self.miner = AIMiner(self)
        self.validator = AIValidator(self)
        self.publisher = TaskPublisher(self)

    async def __aenter__(self):
        self.session = aiohttp.ClientSession()
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if self.session:
            await self.session.close()

    async def rpc_call(self, method: str, params: Dict = None) -> Dict:
        """Call RPC interface"""
        if not self.session:
            self.session = aiohttp.ClientSession()

        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params or {},
            "id": 1
        }

        async with self.session.post(self.rpc_url, json=payload) as resp:
            result = await resp.json()
            if "error" in result:
                raise AIRPCError(result["error"])
            return result["result"]

    # General methods
    async def get_network_status(self) -> NetworkStatus:
        """Get network status"""
        result = await self.rpc_call("ai_getNetworkStatus")
        return NetworkStatus.from_dict(result)

    async def get_task_details(self, task_id: str) -> TaskDetails:
        """Get task details"""
        result = await self.rpc_call("ai_getTaskDetails", {"task_id": task_id})
        return TaskDetails.from_dict(result)

    async def list_active_tasks(self, filters: Optional[TaskFilters] = None) -> List[TaskSummary]:
        """List active tasks"""
        params = {"filters": filters.to_dict()} if filters else {}
        result = await self.rpc_call("ai_listActiveTasks", params)
        return [TaskSummary.from_dict(task) for task in result["tasks"]]

    async def get_miner_stats(self, miner_address: str) -> MinerStats:
        """Get miner statistics"""
        result = await self.rpc_call("ai_getMinerStats", {"miner_address": miner_address})
        return MinerStats.from_dict(result)

# Exception classes
class AIRPCError(Exception):
    """AI RPC call exception"""
    pass

class AIValidationError(Exception):
    """AI validation exception"""
    pass
```

## 2. Data Type Definitions

```python
# tos_ai/types.py
from dataclasses import dataclass, asdict
from typing import Dict, List, Optional, Union
from enum import Enum
import json

class TaskType(Enum):
    CODE_ANALYSIS = "code_analysis"
    SECURITY_AUDIT = "security_audit"
    DATA_ANALYSIS = "data_analysis"
    ALGORITHM_OPTIMIZATION = "algorithm_optimization"
    LOGIC_REASONING = "logic_reasoning"
    GENERAL_TASK = "general_task"

class DifficultyLevel(Enum):
    BEGINNER = "beginner"
    INTERMEDIATE = "intermediate"
    ADVANCED = "advanced"
    EXPERT = "expert"

class VerificationType(Enum):
    AUTOMATIC = "automatic"
    PEER_REVIEW = "peer_review"
    EXPERT_REVIEW = "expert_review"
    HYBRID = "hybrid"

@dataclass
class TaskFilters:
    task_type: Optional[TaskType] = None
    difficulty: Optional[DifficultyLevel] = None
    min_reward: Optional[int] = None
    max_reward: Optional[int] = None
    deadline_before: Optional[datetime] = None

    def to_dict(self) -> Dict:
        result = {}
        if self.task_type:
            result["task_type"] = self.task_type.value
        if self.difficulty:
            result["difficulty"] = self.difficulty.value
        if self.min_reward:
            result["min_reward"] = self.min_reward
        if self.max_reward:
            result["max_reward"] = self.max_reward
        if self.deadline_before:
            result["deadline_before"] = int(self.deadline_before.timestamp())
        return result

@dataclass
class TaskSummary:
    task_id: str
    title: str
    task_type: TaskType
    difficulty: DifficultyLevel
    reward_amount: int
    deadline: datetime
    participants_count: int
    max_participants: int
    publisher: str

    @classmethod
    def from_dict(cls, data: Dict) -> 'TaskSummary':
        return cls(
            task_id=data["task_id"],
            title=data["title"],
            task_type=TaskType(data["task_type"]),
            difficulty=DifficultyLevel(data["difficulty"]),
            reward_amount=data["reward_amount"],
            deadline=datetime.fromtimestamp(data["deadline"]),
            participants_count=data["participants_count"],
            max_participants=data["max_participants"],
            publisher=data["publisher"]
        )

@dataclass
class TaskDetails:
    task_id: str
    title: str
    description: str
    task_type: TaskType
    difficulty: DifficultyLevel
    reward_amount: int
    stake_required: int
    deadline: datetime
    verification_type: VerificationType
    quality_threshold: int
    participants: List[str]
    submissions: List[Dict]

    @classmethod
    def from_dict(cls, data: Dict) -> 'TaskDetails':
        return cls(
            task_id=data["task_id"],
            title=data["title"],
            description=data["description"],
            task_type=TaskType(data["task_type"]),
            difficulty=DifficultyLevel(data["difficulty"]),
            reward_amount=data["reward_amount"],
            stake_required=data["stake_required"],
            deadline=datetime.fromtimestamp(data["deadline"]),
            verification_type=VerificationType(data["verification_type"]),
            quality_threshold=data["quality_threshold"],
            participants=data["participants"],
            submissions=data["submissions"]
        )

@dataclass
class NetworkStatus:
    active_tasks: int
    active_miners: int
    total_rewards_distributed: int
    network_hash_rate: float

    @classmethod
    def from_dict(cls, data: Dict) -> 'NetworkStatus':
        return cls(**data)

@dataclass
class MinerStats:
    address: str
    reputation_score: int
    tasks_completed: int
    tasks_failed: int
    total_earnings: int
    success_rate: float
    specializations: List[TaskType]

    @classmethod
    def from_dict(cls, data: Dict) -> 'MinerStats':
        return cls(
            address=data["address"],
            reputation_score=data["reputation_score"],
            tasks_completed=data["tasks_completed"],
            tasks_failed=data["tasks_failed"],
            total_earnings=data["total_earnings"],
            success_rate=data["success_rate"],
            specializations=[TaskType(spec) for spec in data["specializations"]]
        )
```

## 3. Miner Functionality Module

```python
# tos_ai/miner.py
import asyncio
from typing import Dict, List, Optional, AsyncGenerator
from datetime import datetime

from .types import *

class AIMiner:
    """AI Miner functionality module"""

    def __init__(self, client):
        self.client = client
        self.is_running = False
        self.auto_tasks = []

    async def register(self, miner_config: MinerConfig) -> str:
        """Register miner"""
        params = {
            "miner_config": miner_config.to_dict()
        }
        result = await self.client.rpc_call("ai_registerMiner", params)
        return result["registration_id"]

    async def participate_task(self, task_id: str, stake_amount: int) -> str:
        """Participate in task"""
        params = {
            "task_id": task_id,
            "stake_amount": stake_amount
        }
        result = await self.client.rpc_call("ai_participateTask", params)
        return result["participation_id"]

    async def submit_solution(self, task_id: str, solution: TaskSolution) -> str:
        """Submit solution"""
        params = {
            "task_id": task_id,
            "solution": solution.to_dict()
        }
        result = await self.client.rpc_call("ai_submitSolution", params)
        return result["submission_id"]

    async def get_my_tasks(self) -> List[TaskSummary]:
        """Get tasks I'm participating in"""
        result = await self.client.rpc_call("ai_getMyTasks")
        return [TaskSummary.from_dict(task) for task in result["tasks"]]

    async def start_auto_mining(self, config: AutoMiningConfig):
        """Start auto mining"""
        self.is_running = True
        print("🚀 Starting automatic AI mining...")

        while self.is_running:
            try:
                # Find suitable tasks
                suitable_tasks = await self._find_suitable_tasks(config)

                for task in suitable_tasks:
                    if await self._should_participate(task, config):
                        await self._auto_participate(task, config)

                # Check status of participated tasks
                await self._check_active_tasks(config)

                # Wait for next round
                await asyncio.sleep(config.check_interval)

            except Exception as e:
                print(f"❌ Auto mining error: {e}")
                await asyncio.sleep(30)  # Wait 30 seconds after error

    def stop_auto_mining(self):
        """Stop auto mining"""
        self.is_running = False
        print("⏹️ Stopping automatic AI mining")

    async def _find_suitable_tasks(self, config: AutoMiningConfig) -> List[TaskSummary]:
        """Find suitable tasks"""
        filters = TaskFilters(
            task_type=config.preferred_task_types[0] if config.preferred_task_types else None,
            min_reward=config.min_reward,
            difficulty=config.preferred_difficulty
        )

        tasks = await self.client.list_active_tasks(filters)
        return [task for task in tasks if self._matches_criteria(task, config)]

    def _matches_criteria(self, task: TaskSummary, config: AutoMiningConfig) -> bool:
        """Check if task matches criteria"""
        # Check task type
        if config.preferred_task_types and task.task_type not in config.preferred_task_types:
            return False

        # Check reward
        if task.reward_amount < config.min_reward:
            return False

        # Check difficulty
        if config.preferred_difficulty and task.difficulty != config.preferred_difficulty:
            return False

        # Check deadline
        time_left = task.deadline - datetime.now()
        if time_left.total_seconds() < config.min_time_left:
            return False

        return True

    async def _should_participate(self, task: TaskSummary, config: AutoMiningConfig) -> bool:
        """Determine if should participate in task"""
        # Check concurrent task count
        my_tasks = await self.get_my_tasks()
        active_count = len([t for t in my_tasks if t.deadline > datetime.now()])

        if active_count >= config.max_concurrent_tasks:
            return False

        # Check success rate prediction
        success_probability = await self._predict_success_probability(task)
        if success_probability < config.min_success_probability:
            return False

        return True

    async def _auto_participate(self, task: TaskSummary, config: AutoMiningConfig):
        """Auto participate in task"""
        try:
            stake_amount = await self._calculate_stake_amount(task, config)
            participation_id = await self.participate_task(task.task_id, stake_amount)

            print(f"✅ Auto participated in task: {task.title} (Reward: {task.reward_amount} TOS)")

            # Auto generate and submit solution
            if config.auto_submit_solutions:
                await self._auto_solve_and_submit(task, config)

        except Exception as e:
            print(f"❌ Failed to participate in task {task.task_id}: {e}")

    async def _auto_solve_and_submit(self, task: TaskSummary, config: AutoMiningConfig):
        """Auto solve task and submit"""
        # Here you can integrate AI models to auto generate solutions
        # For example, call ChatGPT API, Claude API, etc.

        task_details = await self.client.get_task_details(task.task_id)

        if task_details.task_type == TaskType.CODE_ANALYSIS:
            solution = await self._solve_code_analysis(task_details, config)
        elif task_details.task_type == TaskType.DATA_ANALYSIS:
            solution = await self._solve_data_analysis(task_details, config)
        else:
            solution = await self._solve_general_task(task_details, config)

        if solution:
            submission_id = await self.submit_solution(task.task_id, solution)
            print(f"📤 Auto submitted solution: {task.task_id}")

    async def _solve_code_analysis(self, task: TaskDetails, config: AutoMiningConfig) -> Optional[TaskSolution]:
        """Solve code analysis task"""
        # Integrate code analysis AI
        if config.ai_api_key:
            # Call AI API to analyze code
            pass
        return None

    async def _predict_success_probability(self, task: TaskSummary) -> float:
        """Predict success probability"""
        # Predict success probability based on historical data and task features
        base_probability = 0.3

        # Adjust based on difficulty
        difficulty_bonus = {
            DifficultyLevel.BEGINNER: 0.3,
            DifficultyLevel.INTERMEDIATE: 0.1,
            DifficultyLevel.ADVANCED: -0.1,
            DifficultyLevel.EXPERT: -0.2
        }.get(task.difficulty, 0)

        return min(1.0, base_probability + difficulty_bonus)

@dataclass
class MinerConfig:
    miner_id: str
    skills: List[str]
    specializations: List[TaskType]
    stake_amount: int
    contact_info: Dict[str, str]

    def to_dict(self) -> Dict:
        return asdict(self)

@dataclass
class AutoMiningConfig:
    preferred_task_types: List[TaskType]
    preferred_difficulty: Optional[DifficultyLevel]
    min_reward: int
    max_concurrent_tasks: int
    min_success_probability: float
    min_time_left: int  # seconds
    check_interval: int  # seconds
    auto_submit_solutions: bool
    ai_api_key: Optional[str]

@dataclass
class TaskSolution:
    content: str
    code: Optional[str]
    documentation: Optional[str]
    test_results: Optional[Dict]
    performance_metrics: Optional[Dict]

    def to_dict(self) -> Dict:
        return asdict(self)
```

## 4. Usage Examples

```python
# examples/simple_miner.py
import asyncio
from tos_ai import TOSAIClient, MinerConfig, AutoMiningConfig, TaskType, DifficultyLevel

async def simple_mining_example():
    async with TOSAIClient("http://localhost:8545") as client:
        # Register miner
        miner_config = MinerConfig(
            miner_id="python_miner_001",
            skills=["python", "data-analysis", "machine-learning"],
            specializations=[TaskType.DATA_ANALYSIS, TaskType.CODE_ANALYSIS],
            stake_amount=1000000000,  # 1 TOS
            contact_info={"email": "miner@example.com"}
        )

        registration_id = await client.miner.register(miner_config)
        print(f"Miner registration successful: {registration_id}")

        # Get network status
        status = await client.get_network_status()
        print(f"Network status: Active tasks {status.active_tasks}, Active miners {status.active_miners}")

        # List tasks
        tasks = await client.list_active_tasks()
        print(f"Found {len(tasks)} active tasks")

        for task in tasks[:3]:  # Show first 3 tasks
            print(f"- {task.title}: {task.reward_amount} TOS, Deadline {task.deadline}")

        # Manually participate in a task
        if tasks:
            task = tasks[0]
            participation_id = await client.miner.participate_task(task.task_id, 1000000000)
            print(f"Task participation successful: {participation_id}")

async def auto_mining_example():
    """Auto mining example"""
    async with TOSAIClient("http://localhost:8545") as client:
        # Configure auto mining
        auto_config = AutoMiningConfig(
            preferred_task_types=[TaskType.CODE_ANALYSIS, TaskType.DATA_ANALYSIS],
            preferred_difficulty=DifficultyLevel.INTERMEDIATE,
            min_reward=5000000000,  # 5 TOS
            max_concurrent_tasks=3,
            min_success_probability=0.4,
            min_time_left=3600,  # 1 hour
            check_interval=60,   # Check every minute
            auto_submit_solutions=False,  # Don't auto submit for now
            ai_api_key=None
        )

        # Start auto mining
        mining_task = asyncio.create_task(client.miner.start_auto_mining(auto_config))

        # Stop after running for 10 minutes
        await asyncio.sleep(600)
        client.miner.stop_auto_mining()
        await mining_task

if __name__ == "__main__":
    # Run simple example
    asyncio.run(simple_mining_example())

    # Or run auto mining
    # asyncio.run(auto_mining_example())
```

## 5. Installation and Deployment

```bash
# Install dependencies
pip install tos-ai-python

# Or install from source
git clone https://github.com/tos-network/tos-ai-python
cd tos-ai-python
pip install -e .
```

## Feature Comparison

### Python Client vs CLI Tool

| Feature | Python Client | CLI Tool |
|---------|---------------|----------|
| Ease of Use | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| Automation | ⭐⭐⭐⭐⭐ | ⭐⭐ |
| Integration | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| AI Integration | ⭐⭐⭐⭐⭐ | ⭐⭐ |
| Resource Usage | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| Performance | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |

The Python client is particularly suitable for:
- Users who need automated AI mining
- Those who want to integrate AI models for automatic solution generation
- Complex business logic and data processing requirements
- Machine learning and data science practitioners