# TOS AI Python客户端设计

## 1. 主客户端类

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
    """TOS AI挖矿Python客户端"""

    def __init__(self, rpc_url: str = "http://localhost:8545", wallet_path: Optional[str] = None):
        self.rpc_url = rpc_url
        self.wallet_path = wallet_path
        self.session: Optional[aiohttp.ClientSession] = None

        # 子模块
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
        """调用RPC接口"""
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

    # 通用方法
    async def get_network_status(self) -> NetworkStatus:
        """获取网络状态"""
        result = await self.rpc_call("ai_getNetworkStatus")
        return NetworkStatus.from_dict(result)

    async def get_task_details(self, task_id: str) -> TaskDetails:
        """获取任务详情"""
        result = await self.rpc_call("ai_getTaskDetails", {"task_id": task_id})
        return TaskDetails.from_dict(result)

    async def list_active_tasks(self, filters: Optional[TaskFilters] = None) -> List[TaskSummary]:
        """列出活跃任务"""
        params = {"filters": filters.to_dict()} if filters else {}
        result = await self.rpc_call("ai_listActiveTasks", params)
        return [TaskSummary.from_dict(task) for task in result["tasks"]]

    async def get_miner_stats(self, miner_address: str) -> MinerStats:
        """获取矿工统计"""
        result = await self.rpc_call("ai_getMinerStats", {"miner_address": miner_address})
        return MinerStats.from_dict(result)

# 异常类
class AIRPCError(Exception):
    """AI RPC调用异常"""
    pass

class AIValidationError(Exception):
    """AI验证异常"""
    pass
```

## 2. 数据类型定义

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

## 3. 矿工功能模块

```python
# tos_ai/miner.py
import asyncio
from typing import Dict, List, Optional, AsyncGenerator
from datetime import datetime

from .types import *

class AIMiner:
    """AI矿工功能模块"""

    def __init__(self, client):
        self.client = client
        self.is_running = False
        self.auto_tasks = []

    async def register(self, miner_config: MinerConfig) -> str:
        """注册矿工"""
        params = {
            "miner_config": miner_config.to_dict()
        }
        result = await self.client.rpc_call("ai_registerMiner", params)
        return result["registration_id"]

    async def participate_task(self, task_id: str, stake_amount: int) -> str:
        """参与任务"""
        params = {
            "task_id": task_id,
            "stake_amount": stake_amount
        }
        result = await self.client.rpc_call("ai_participateTask", params)
        return result["participation_id"]

    async def submit_solution(self, task_id: str, solution: TaskSolution) -> str:
        """提交解决方案"""
        params = {
            "task_id": task_id,
            "solution": solution.to_dict()
        }
        result = await self.client.rpc_call("ai_submitSolution", params)
        return result["submission_id"]

    async def get_my_tasks(self) -> List[TaskSummary]:
        """获取我参与的任务"""
        result = await self.client.rpc_call("ai_getMyTasks")
        return [TaskSummary.from_dict(task) for task in result["tasks"]]

    async def start_auto_mining(self, config: AutoMiningConfig):
        """开始自动挖矿"""
        self.is_running = True
        print("🚀 开始自动AI挖矿...")

        while self.is_running:
            try:
                # 查找合适的任务
                suitable_tasks = await self._find_suitable_tasks(config)

                for task in suitable_tasks:
                    if await self._should_participate(task, config):
                        await self._auto_participate(task, config)

                # 检查已参与任务的状态
                await self._check_active_tasks(config)

                # 等待下一轮
                await asyncio.sleep(config.check_interval)

            except Exception as e:
                print(f"❌ 自动挖矿错误: {e}")
                await asyncio.sleep(30)  # 出错后等待30秒

    def stop_auto_mining(self):
        """停止自动挖矿"""
        self.is_running = False
        print("⏹️ 停止自动AI挖矿")

    async def _find_suitable_tasks(self, config: AutoMiningConfig) -> List[TaskSummary]:
        """查找合适的任务"""
        filters = TaskFilters(
            task_type=config.preferred_task_types[0] if config.preferred_task_types else None,
            min_reward=config.min_reward,
            difficulty=config.preferred_difficulty
        )

        tasks = await self.client.list_active_tasks(filters)
        return [task for task in tasks if self._matches_criteria(task, config)]

    def _matches_criteria(self, task: TaskSummary, config: AutoMiningConfig) -> bool:
        """检查任务是否符合标准"""
        # 检查任务类型
        if config.preferred_task_types and task.task_type not in config.preferred_task_types:
            return False

        # 检查奖励
        if task.reward_amount < config.min_reward:
            return False

        # 检查难度
        if config.preferred_difficulty and task.difficulty != config.preferred_difficulty:
            return False

        # 检查截止时间
        time_left = task.deadline - datetime.now()
        if time_left.total_seconds() < config.min_time_left:
            return False

        return True

    async def _should_participate(self, task: TaskSummary, config: AutoMiningConfig) -> bool:
        """判断是否应该参与任务"""
        # 检查并发任务数量
        my_tasks = await self.get_my_tasks()
        active_count = len([t for t in my_tasks if t.deadline > datetime.now()])

        if active_count >= config.max_concurrent_tasks:
            return False

        # 检查成功率预测
        success_probability = await self._predict_success_probability(task)
        if success_probability < config.min_success_probability:
            return False

        return True

    async def _auto_participate(self, task: TaskSummary, config: AutoMiningConfig):
        """自动参与任务"""
        try:
            stake_amount = await self._calculate_stake_amount(task, config)
            participation_id = await self.participate_task(task.task_id, stake_amount)

            print(f"✅ 自动参与任务: {task.title} (奖励: {task.reward_amount} TOS)")

            # 自动生成和提交解决方案
            if config.auto_submit_solutions:
                await self._auto_solve_and_submit(task, config)

        except Exception as e:
            print(f"❌ 参与任务失败 {task.task_id}: {e}")

    async def _auto_solve_and_submit(self, task: TaskSummary, config: AutoMiningConfig):
        """自动解决任务并提交"""
        # 这里可以集成AI模型来自动生成解决方案
        # 例如调用ChatGPT API、Claude API等

        task_details = await self.client.get_task_details(task.task_id)

        if task_details.task_type == TaskType.CODE_ANALYSIS:
            solution = await self._solve_code_analysis(task_details, config)
        elif task_details.task_type == TaskType.DATA_ANALYSIS:
            solution = await self._solve_data_analysis(task_details, config)
        else:
            solution = await self._solve_general_task(task_details, config)

        if solution:
            submission_id = await self.submit_solution(task.task_id, solution)
            print(f"📤 自动提交解决方案: {task.task_id}")

    async def _solve_code_analysis(self, task: TaskDetails, config: AutoMiningConfig) -> Optional[TaskSolution]:
        """解决代码分析任务"""
        # 集成代码分析AI
        if config.ai_api_key:
            # 调用AI API分析代码
            pass
        return None

    async def _predict_success_probability(self, task: TaskSummary) -> float:
        """预测成功概率"""
        # 基于历史数据和任务特征预测成功概率
        base_probability = 0.3

        # 根据难度调整
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
    min_time_left: int  # 秒
    check_interval: int  # 秒
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

## 4. 使用示例

```python
# examples/simple_miner.py
import asyncio
from tos_ai import TOSAIClient, MinerConfig, AutoMiningConfig, TaskType, DifficultyLevel

async def simple_mining_example():
    async with TOSAIClient("http://localhost:8545") as client:
        # 注册矿工
        miner_config = MinerConfig(
            miner_id="python_miner_001",
            skills=["python", "data-analysis", "machine-learning"],
            specializations=[TaskType.DATA_ANALYSIS, TaskType.CODE_ANALYSIS],
            stake_amount=1000000000,  # 1 TOS
            contact_info={"email": "miner@example.com"}
        )

        registration_id = await client.miner.register(miner_config)
        print(f"矿工注册成功: {registration_id}")

        # 获取网络状态
        status = await client.get_network_status()
        print(f"网络状态: 活跃任务 {status.active_tasks}, 活跃矿工 {status.active_miners}")

        # 列出任务
        tasks = await client.list_active_tasks()
        print(f"发现 {len(tasks)} 个活跃任务")

        for task in tasks[:3]:  # 显示前3个任务
            print(f"- {task.title}: {task.reward_amount} TOS, 截止 {task.deadline}")

        # 手动参与一个任务
        if tasks:
            task = tasks[0]
            participation_id = await client.miner.participate_task(task.task_id, 1000000000)
            print(f"参与任务成功: {participation_id}")

async def auto_mining_example():
    """自动挖矿示例"""
    async with TOSAIClient("http://localhost:8545") as client:
        # 配置自动挖矿
        auto_config = AutoMiningConfig(
            preferred_task_types=[TaskType.CODE_ANALYSIS, TaskType.DATA_ANALYSIS],
            preferred_difficulty=DifficultyLevel.INTERMEDIATE,
            min_reward=5000000000,  # 5 TOS
            max_concurrent_tasks=3,
            min_success_probability=0.4,
            min_time_left=3600,  # 1小时
            check_interval=60,   # 每分钟检查
            auto_submit_solutions=False,  # 暂不自动提交
            ai_api_key=None
        )

        # 开始自动挖矿
        mining_task = asyncio.create_task(client.miner.start_auto_mining(auto_config))

        # 运行10分钟后停止
        await asyncio.sleep(600)
        client.miner.stop_auto_mining()
        await mining_task

if __name__ == "__main__":
    # 运行简单示例
    asyncio.run(simple_mining_example())

    # 或运行自动挖矿
    # asyncio.run(auto_mining_example())
```

## 5. 安装和部署

```bash
# 安装依赖
pip install tos-ai-python

# 或从源码安装
git clone https://github.com/tos-network/tos-ai-python
cd tos-ai-python
pip install -e .
```

## 优势对比

### Python客户端 vs CLI工具

| 特性 | Python客户端 | CLI工具 |
|-----|-------------|---------|
| 易用性 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| 自动化 | ⭐⭐⭐⭐⭐ | ⭐⭐ |
| 集成性 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| AI集成 | ⭐⭐⭐⭐⭐ | ⭐⭐ |
| 资源消耗 | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| 性能 | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |

Python客户端特别适合：
- 需要自动化AI挖矿的用户
- 想要集成AI模型自动生成解决方案
- 需要复杂业务逻辑和数据处理
- 机器学习和数据科学工作者