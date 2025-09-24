# TOS AI Mining API Reference

## Overview

This document describes the RPC API interface for TOS AI Mining functionality. The API provides comprehensive access to task management, miner operations, validation, and reward distribution.

## RPC Interface Structure

### Core API Handler
```rust
pub struct AIRpcHandler {
    task_manager: Arc<RwLock<TaskManager>>,
    miner_registry: Arc<RwLock<MinerRegistry>>,
    enabled: bool,
}
```

## API Endpoints

### 1. Network Information

#### `ai_get_network_info`
Get AI mining network status and statistics.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_get_network_info",
  "params": {},
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "active_tasks": 1250,
    "total_miners": 3450,
    "online_miners": 2100,
    "total_rewards_distributed": "15000000000000",
    "network_hash_rate": 450.7,
    "average_task_completion_time": 3600,
    "current_difficulty": "Intermediate"
  },
  "id": 1
}
```

### 2. Task Management

#### `ai_publish_task`
Publish a new AI mining task.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_publish_task",
  "params": {
    "task_data": {
      "title": "Optimize Rust Sorting Algorithm",
      "description": "Improve bubble sort performance while maintaining readability",
      "task_type": {
        "CodeAnalysis": {
          "language": "rust"
        }
      },
      "difficulty_level": "Intermediate",
      "reward_amount": "50000000000",
      "stake_required": "5000000000",
      "deadline": 1640995200,
      "max_participants": 10,
      "verification_type": {
        "PeerReview": {
          "required_reviewers": 3,
          "consensus_threshold": 0.67
        }
      },
      "quality_threshold": 75
    },
    "publisher": "tos1abc123...",
    "current_block_height": 1500000
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "task_id": "0x1234567890abcdef...",
    "status": "published",
    "estimated_gas_cost": "1500000",
    "estimated_participants": 8,
    "recommended_deadline": 1640995200
  },
  "id": 1
}
```

#### `ai_get_task_details`
Get detailed information about a specific task.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_get_task_details",
  "params": {
    "task_id": "0x1234567890abcdef..."
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "task_id": "0x1234567890abcdef...",
    "title": "Optimize Rust Sorting Algorithm",
    "description": "Improve bubble sort performance...",
    "status": "InProgress",
    "publisher": "tos1abc123...",
    "reward_amount": "50000000000",
    "stake_required": "5000000000",
    "deadline": 1640995200,
    "participants_count": 5,
    "max_participants": 10,
    "submissions_count": 2,
    "creation_time": 1640991600,
    "task_type": "CodeAnalysis",
    "difficulty_level": "Intermediate",
    "verification_type": "PeerReview"
  },
  "id": 1
}
```

#### `ai_list_active_tasks`
List currently active tasks with optional filtering.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_list_active_tasks",
  "params": {
    "filters": {
      "task_type": "CodeAnalysis",
      "difficulty_level": "Intermediate",
      "min_reward": "10000000000",
      "max_participants": 10
    },
    "sort_by": "reward_amount",
    "sort_order": "desc",
    "limit": 50,
    "offset": 0
  },
  "id": 1
}
```

### 3. Miner Operations

#### `ai_register_miner`
Register as an AI miner.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_register_miner",
  "params": {
    "miner_config": {
      "miner_address": "tos1def456...",
      "specializations": ["CodeAnalysis", "DataAnalysis"],
      "stake_amount": "10000000000",
      "contact_info": {
        "email": "miner@example.com",
        "github": "https://github.com/miner"
      }
    }
  },
  "id": 1
}
```

#### `ai_participate_task`
Participate in a specific task.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_participate_task",
  "params": {
    "task_id": "0x1234567890abcdef...",
    "miner": "tos1def456...",
    "stake_amount": "5000000000"
  },
  "id": 1
}
```

#### `ai_submit_solution`
Submit a solution for a task.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_submit_solution",
  "params": {
    "task_id": "0x1234567890abcdef...",
    "solution": {
      "code": "fn quick_sort<T: Ord>(arr: &mut [T]) { ... }",
      "documentation": "## Algorithm Analysis\n\nThis implementation...",
      "test_results": {
        "passed": 15,
        "failed": 0,
        "performance_improvement": "95%"
      }
    },
    "submitter": "tos1def456..."
  },
  "id": 1
}
```

### 4. Validation Operations

#### `ai_validate_submission`
Submit validation for a solution.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_validate_submission",
  "params": {
    "task_id": "0x1234567890abcdef...",
    "submission_id": "0xabcdef1234567890...",
    "validation_result": {
      "Approve": {
        "quality_score": 85,
        "reasoning": "Excellent implementation with good performance improvements"
      }
    },
    "validator": "tos1ghi789..."
  },
  "id": 1
}
```

### 5. Reward Operations

#### `ai_claim_rewards`
Claim earned rewards.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_claim_rewards",
  "params": {
    "task_ids": ["0x1234567890abcdef...", "0xabcdef1234567890..."],
    "claimant": "tos1def456..."
  },
  "id": 1
}
```

#### `ai_get_miner_stats`
Get miner statistics and performance.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ai_get_miner_stats",
  "params": {
    "miner_address": "tos1def456...",
    "period": "last_30_days"
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "miner_address": "tos1def456...",
    "reputation_score": 8750,
    "success_rate": 0.87,
    "tasks_completed": 45,
    "tasks_failed": 3,
    "total_earnings": "450000000000",
    "current_stake": "25000000000",
    "specializations": ["CodeAnalysis", "DataAnalysis"],
    "certification_level": "Professional",
    "rank": 127,
    "recent_tasks": [
      {
        "task_id": "0x123...",
        "completion_time": 1640991600,
        "reward_earned": "30000000000",
        "quality_score": 92
      }
    ]
  },
  "id": 1
}
```

## CLI Tool Usage

### Task Management
```bash
# Publish a task
tos-ai task publish -c task.json

# List active tasks
tos-ai task list --filter "difficulty=advanced"

# Get task status
tos-ai task status <task_id>
```

### Miner Operations
```bash
# Register as miner
tos-ai miner register -c miner_config.json

# Participate in task
tos-ai miner participate <task_id>

# Submit solution
tos-ai miner submit <task_id> -s solution.rs

# Check miner stats
tos-ai miner stats
```

### Validation
```bash
# Register as validator
tos-ai validator register

# Validate submission
tos-ai validator validate <task_id> <submission_id>

# Check validation queue
tos-ai validator queue
```

## Error Codes

| Code | Message | Description |
|------|---------|-------------|
| -32001 | Invalid task ID | Task not found or invalid format |
| -32002 | Insufficient stake | Not enough TOS staked for operation |
| -32003 | Task expired | Task deadline has passed |
| -32004 | Permission denied | Not authorized for this operation |
| -32005 | Validation failed | Submission validation failed |
| -32006 | Reward already claimed | Rewards for this task already claimed |
| -32007 | Network congestion | Network too busy, try again later |

## Rate Limits

- Task publication: 10 per hour per address
- Solution submission: 100 per hour per address
- Validation requests: 500 per hour per address
- Status queries: 1000 per hour per address

## Authentication

All RPC calls require proper authentication using TOS wallet signatures:

```javascript
const signature = wallet.sign(JSON.stringify(params));
const request = {
  jsonrpc: "2.0",
  method: "ai_publish_task",
  params: params,
  auth: {
    address: wallet.address,
    signature: signature
  },
  id: 1
};
```

## WebSocket Subscriptions

### Real-time Task Updates
```javascript
// Subscribe to new tasks
ws.send(JSON.stringify({
  jsonrpc: "2.0",
  method: "ai_subscribe_new_tasks",
  params: {
    filters: {
      difficulty_level: "Intermediate"
    }
  },
  id: 1
}));

// Subscribe to task status updates
ws.send(JSON.stringify({
  jsonrpc: "2.0",
  method: "ai_subscribe_task_updates",
  params: {
    task_id: "0x1234567890abcdef..."
  },
  id: 2
}));
```

## SDK Integration

### JavaScript/TypeScript
```typescript
import { TOSAIClient } from 'tos-ai-sdk';

const client = new TOSAIClient({
  rpcUrl: 'http://localhost:8545',
  walletPath: './wallet.json'
});

// Publish task
const taskId = await client.publishTask({
  title: "Code Review Task",
  taskType: "CodeAnalysis",
  reward: "50000000000", // 50 TOS
  deadline: Date.now() + 86400000 // 24 hours
});

// Get task details
const task = await client.getTaskDetails(taskId);
```

### Python
```python
from tos_ai import TOSAIClient

client = TOSAIClient("http://localhost:8545")

# List active tasks
tasks = await client.list_active_tasks(
    filters={"difficulty": "intermediate"}
)

# Participate in task
await client.miner.participate_task(task_id, stake_amount=5000000000)
```

This API reference provides comprehensive access to the TOS AI Mining system, enabling developers to build applications, mining tools, and integrations with the decentralized AI computing marketplace.