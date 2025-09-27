# TOS AI Multi-Language Client Architecture

## Overview

The TOS AI Mining system is designed with a flexible architecture that supports multiple programming languages for frontend implementation while maintaining a robust Rust-based backend. This document outlines how developers can implement AI mining clients in Python and other languages.

## Architecture Design

### Core Backend (Rust)
- **TOS Node**: Implemented in Rust, providing core blockchain functionality
- **AI Mining Module**: Embedded within the TOS node
- **Standardized APIs**: RPC/HTTP/WebSocket interfaces for external access

### Frontend Clients (Multi-Language)
- **Python Client**: Fully designed and implemented
- **JavaScript/TypeScript**: Web frontend or Node.js applications
- **Go**: High-performance mining clients
- **Java**: Enterprise-level integration
- **C++**: Ultimate performance optimization

## System Architecture Diagram

```
┌─────────────────────────────────────────┐
│         TOS Node (Rust Core)            │
├─────────────────────────────────────────┤
│  • AI Mining Module                     │
│  • Task Management                      │
│  • Validation System                    │
│  • Reward Distribution                  │
└─────────┬───────────────────────────────┘
          │ JSON-RPC / HTTP / WebSocket
          │
┌─────────┴───────────────────────────────┐
│          Standardized API Layer         │
└─────────┬───────────────────────────────┘
          │
    ┌─────┴─────┬─────────┬─────────┬─────────┐
    │           │         │         │         │
┌───▼───┐ ┌────▼───┐ ┌───▼───┐ ┌───▼───┐ ┌───▼───┐
│Python │ │JavaScript│ │  Go   │ │ Java  │ │ C++   │
│Client │ │ Client   │ │Client │ │Client │ │Client │
└───────┘ └─────────┘ └───────┘ └───────┘ └───────┘
```

## Python Client Implementation

### 1. Main Client Class

```python
# tos_ai/client.py
import asyncio
import aiohttp
import json
from typing import Dict, List, Optional
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
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params or {},
            "id": 1
        }

        async with self.session.post(self.rpc_url, json=payload) as response:
            result = await response.json()

            if "error" in result:
                raise Exception(f"RPC Error: {result['error']}")

            return result["result"]
```

### 2. Auto Mining Implementation

```python
# tos_ai/miner.py
class AIMiner:
    def __init__(self, client):
        self.client = client
        self.is_running = False

    async def start_auto_mining(self, config: AutoMiningConfig):
        """Start automatic AI mining"""
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

                # Wait for next round - configurable interval
                await asyncio.sleep(config.check_interval)

            except Exception as e:
                print(f"❌ Auto mining error: {e}")
                await asyncio.sleep(30)  # Wait 30 seconds after error

    def stop_auto_mining(self):
        """Stop automatic mining"""
        self.is_running = False
        print("⏹️ Stopping automatic AI mining")

    async def participate_task(self, task_id: str, stake: int) -> Dict:
        """Participate in a specific task"""
        return await self.client.rpc_call("ai_participate_task", {
            "task_id": task_id,
            "stake_amount": str(stake)
        })

    async def submit_solution(self, task_id: str, solution: TaskSolution) -> Dict:
        """Submit solution for a task"""
        return await self.client.rpc_call("ai_submit_solution", {
            "task_id": task_id,
            "solution_data": solution.to_dict()
        })
```

### 3. Configuration and Types

```python
@dataclass
class AutoMiningConfig:
    target_difficulty: List[str]
    max_concurrent_tasks: int
    min_success_probability: float
    min_time_left: int  # seconds
    check_interval: int  # seconds - default 60 (every minute)
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

## Standardized API Interfaces

### 1. JSON-RPC Interface

```json
{
  "jsonrpc": "2.0",
  "method": "ai_publish_task",
  "params": {
    "task_data": {
      "title": "Optimize Rust Sorting Algorithm",
      "task_type": {"CodeAnalysis": {"language": "rust"}},
      "reward_amount": "50000000000",
      "difficulty_level": "Intermediate"
    }
  },
  "id": 1
}
```

### 2. HTTP REST API

```bash
curl -X POST http://localhost:8545/ai/tasks \
  -H "Content-Type: application/json" \
  -d '{
    "task_type": "CodeAnalysis",
    "language": "rust",
    "reward_amount": "50000000000"
  }'
```

### 3. WebSocket Real-time API

```javascript
const ws = new WebSocket('ws://localhost:8545/ai/ws');

ws.on('task_published', (data) => {
  console.log('New task published:', data);
});

ws.on('validation_completed', (data) => {
  console.log('Validation completed:', data);
});

ws.on('reward_distributed', (data) => {
  console.log('Reward distributed:', data);
});
```

## Usage Examples

### Simple Python Mining Example

```python
import asyncio
from tos_ai import TOSAIClient, AutoMiningConfig

async def simple_mining_example():
    async with TOSAIClient("http://localhost:8545") as client:
        # Get network information
        network_info = await client.get_network_info()
        print(f"Active tasks: {network_info['active_tasks']}")

        # List available tasks
        tasks = await client.list_active_tasks()

        if tasks:
            task = tasks[0]
            # Participate in first suitable task
            await client.miner.participate_task(task['task_id'], stake=5000000000)

            # Submit solution (example)
            solution = TaskSolution(
                content="Optimized bubble sort implementation",
                code="fn bubble_sort(arr: &mut [i32]) { ... }",
                documentation="Performance improved by 25%"
            )
            await client.miner.submit_solution(task['task_id'], solution)

async def auto_mining_example():
    async with TOSAIClient("http://localhost:8545") as client:
        # Configure automatic mining
        auto_config = AutoMiningConfig(
            target_difficulty=["Intermediate", "Advanced"],
            max_concurrent_tasks=3,
            min_success_probability=0.4,
            min_time_left=3600,  # 1 hour
            check_interval=60,   # Check every minute
            auto_submit_solutions=True,
            ai_api_key="your-ai-service-key"
        )

        # Start auto mining
        mining_task = asyncio.create_task(client.miner.start_auto_mining(auto_config))

        # Run for specified duration
        await asyncio.sleep(3600)  # Run for 1 hour
        client.miner.stop_auto_mining()
        await mining_task

if __name__ == "__main__":
    # Run simple mining
    asyncio.run(simple_mining_example())

    # Or run auto mining
    # asyncio.run(auto_mining_example())
```

### JavaScript/Node.js Client Example

```javascript
// tos-ai-client.js
const WebSocket = require('ws');
const axios = require('axios');

class TOSAIClient {
  constructor(rpcUrl = 'http://localhost:8545') {
    this.rpcUrl = rpcUrl;
    this.ws = null;
  }

  async rpcCall(method, params = {}) {
    const payload = {
      jsonrpc: '2.0',
      method: method,
      params: params,
      id: 1
    };

    const response = await axios.post(this.rpcUrl, payload);

    if (response.data.error) {
      throw new Error(`RPC Error: ${response.data.error.message}`);
    }

    return response.data.result;
  }

  async startAutoMining(config) {
    console.log('🚀 Starting JavaScript AI mining client...');

    while (true) {
      try {
        // Get available tasks
        const tasks = await this.rpcCall('ai_list_active_tasks');

        // Process suitable tasks
        for (const task of tasks) {
          if (this.shouldParticipate(task, config)) {
            await this.participateTask(task.task_id, config.stakeAmount);
          }
        }

        // Wait before next check
        await this.sleep(config.checkInterval * 1000);

      } catch (error) {
        console.error('Mining error:', error);
        await this.sleep(30000); // Wait 30 seconds on error
      }
    }
  }

  async participateTask(taskId, stakeAmount) {
    return await this.rpcCall('ai_participate_task', {
      task_id: taskId,
      stake_amount: stakeAmount.toString()
    });
  }

  shouldParticipate(task, config) {
    return config.targetDifficulties.includes(task.difficulty_level) &&
           task.time_left > config.minTimeLeft;
  }

  sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }
}

// Usage
const client = new TOSAIClient();
client.startAutoMining({
  targetDifficulties: ['Intermediate', 'Advanced'],
  stakeAmount: 5000000000,
  checkInterval: 60,
  minTimeLeft: 3600
});
```

## Task Processing Frequency

The TOS AI mining system operates with the following frequencies:

- **Primary Check Frequency**: **60 seconds** - Check for new tasks every minute
- **Network Sync Frequency**: **30 seconds** - Synchronize network state
- **Real-time Response**: Immediate participation when suitable tasks are found

### Actual Performance Characteristics:

1. **Minute-level Scanning**: Scan for newly published tasks every minute
2. **Real-time Response**: Immediate response to qualifying tasks (no waiting for next cycle)
3. **Continuous Monitoring**: Ongoing monitoring of participated task validation and reward status
4. **Dynamic Strategy Adjustment**: Adjust participation strategy based on success rate and profitability

This frequency design balances:
- **Timeliness**: Won't miss important task opportunities
- **Network Load**: Avoids excessive network requests
- **Resource Consumption**: Reasonable CPU and network resource usage

## Multi-Language Client Benefits

### 1. Flexibility
- Developers can choose familiar programming languages
- Select optimal technology stack for different scenarios
- Quick integration into existing systems

### 2. Scalability
- Core logic guaranteed by Rust for performance and security
- Frontend clients can focus on business logic
- Support for customized mining strategies

### 3. Ecosystem Compatibility
- **Web Applications**: JavaScript integration
- **Enterprise Systems**: Java/.NET integration
- **High-Performance Computing**: C++/Go scenarios
- **Data Science**: Python environments

## Implementation Guidelines

### For Python Developers
1. Use the provided `TOSAIClient` class as foundation
2. Implement custom mining strategies in the `AIMiner` class
3. Leverage asyncio for efficient concurrent task handling
4. Integrate with AI services (OpenAI, Anthropic, etc.) for solution generation

### For Other Languages
1. Implement HTTP/RPC client following the API specification
2. Handle JSON-RPC 2.0 protocol correctly
3. Implement task polling and submission logic
4. Add error handling and retry mechanisms
5. Support configuration for mining parameters

## Security Considerations

1. **Private Key Management**: Secure storage and handling of wallet private keys
2. **API Authentication**: Implement proper authentication for RPC calls
3. **Rate Limiting**: Respect network rate limits to avoid being blocked
4. **Input Validation**: Validate all task data and solutions before submission
5. **Error Handling**: Graceful error handling to prevent client crashes

## Conclusion

The TOS AI Mining system's **Rust Backend + Multi-Language Frontend** architecture enables developers to build AI mining clients in their preferred programming languages while leveraging the robust, high-performance core implemented in Rust. This approach maximizes both developer productivity and system performance, supporting a diverse ecosystem of AI mining participants.

Through standardized APIs and comprehensive documentation, developers can quickly build and deploy AI mining solutions that participate in the TOS "Proof of Intelligent Work" ecosystem, earning rewards by solving real-world computational problems.