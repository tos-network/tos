# AI Mining Task Configuration File (task.json) Examples

## 1. Basic Code Optimization Task

```json
{
  "title": "Rust Sorting Algorithm Performance Optimization",
  "description": "Optimize existing bubble sort algorithm to improve execution efficiency while maintaining code readability",
  "task_type": {
    "CodeAnalysis": {
      "language": "rust"
    }
  },
  "difficulty_level": "Intermediate",
  "reward_amount": 50,
  "stake_required": 5,
  "max_participants": 10,
  "deadline_hours": 48,
  "quality_threshold": 75,
  "verification_type": {
    "PeerReview": {
      "required_reviewers": 3,
      "consensus_threshold": 0.67
    }
  },
  "task_data": {
    "code_to_analyze": "fn bubble_sort<T: Ord>(arr: &mut [T]) {\n    let len = arr.len();\n    for i in 0..len {\n        for j in 0..len - 1 - i {\n            if arr[j] > arr[j + 1] {\n                arr.swap(j, j + 1);\n            }\n        }\n    }\n}",
    "requirements": [
      "Use more efficient sorting algorithm",
      "Maintain O(n log n) average time complexity",
      "Add performance benchmarking",
      "Provide algorithm complexity analysis",
      "Maintain code readability"
    ],
    "test_cases": [
      {
        "input": "[64, 34, 25, 12, 22, 11, 90]",
        "expected_output": "[11, 12, 22, 25, 34, 64, 90]"
      },
      {
        "input": "[5, 2, 4, 6, 1, 3]",
        "expected_output": "[1, 2, 3, 4, 5, 6]"
      }
    ],
    "performance_requirements": {
      "max_time_complexity": "O(n log n)",
      "max_space_complexity": "O(log n)",
      "benchmark_size": 10000
    }
  },
  "submission_requirements": {
    "include_code": true,
    "include_tests": true,
    "include_benchmarks": true,
    "include_documentation": true,
    "max_file_size_mb": 10
  }
}
```

## 2. Security Audit Task

```json
{
  "title": "Smart Contract Security Vulnerability Audit",
  "description": "Audit DeFi smart contracts to identify and report potential security vulnerabilities",
  "task_type": {
    "SecurityAudit": {
      "scope": "SmartContract"
    }
  },
  "difficulty_level": "Expert",
  "reward_amount": 200,
  "stake_required": 20,
  "max_participants": 5,
  "deadline_hours": 168,
  "quality_threshold": 85,
  "verification_type": {
    "ExpertReview": {
      "expert_count": 2
    }
  },
  "task_data": {
    "contract_code": "// Solidity contract code...",
    "contract_address": "0x1234567890abcdef...",
    "audit_scope": [
      "Reentrancy attack vulnerabilities",
      "Integer overflow/underflow",
      "Access control issues",
      "Logic errors",
      "Gas optimization recommendations"
    ],
    "previous_audits": [
      {
        "auditor": "CertiK",
        "date": "2024-01-15",
        "findings": 3,
        "report_url": "https://..."
      }
    ]
  },
  "submission_requirements": {
    "include_vulnerability_report": true,
    "include_severity_rating": true,
    "include_fix_recommendations": true,
    "include_proof_of_concept": false,
    "report_format": "markdown"
  }
}
```

## 3. Data Analysis Task

```json
{
  "title": "Cryptocurrency Price Trend Analysis",
  "description": "Analyze BTC/ETH price data to predict 7-day future trends",
  "task_type": {
    "DataAnalysis": {
      "data_type": "TimeSeries"
    }
  },
  "difficulty_level": "Advanced",
  "reward_amount": 100,
  "stake_required": 10,
  "max_participants": 8,
  "deadline_hours": 72,
  "quality_threshold": 80,
  "verification_type": {
    "Hybrid": {
      "auto_weight": 0.3,
      "peer_weight": 0.4,
      "expert_weight": 0.3
    }
  },
  "task_data": {
    "dataset_url": "https://api.binance.com/api/v3/klines?symbol=BTCUSDT&interval=1h&limit=1000",
    "data_format": "json",
    "analysis_requirements": [
      "Technical indicator analysis (RSI, MACD, Bollinger Bands)",
      "Price trend identification",
      "Support/resistance level calculation",
      "7-day price prediction",
      "Risk assessment"
    ],
    "evaluation_metrics": [
      "Prediction accuracy (MAPE < 5%)",
      "Model interpretability",
      "Visualization quality",
      "Data cleaning completeness"
    ]
  },
  "submission_requirements": {
    "include_analysis_report": true,
    "include_visualizations": true,
    "include_code": true,
    "include_model_explanation": true,
    "preferred_languages": ["python", "r", "jupyter"]
  }
}
```

## 4. Algorithm Optimization Task

```json
{
  "title": "Graph Shortest Path Algorithm Optimization",
  "description": "Optimize shortest path algorithm performance for large-scale graph data",
  "task_type": {
    "AlgorithmOptimization": {
      "domain": "GraphAlgorithms"
    }
  },
  "difficulty_level": "Expert",
  "reward_amount": 300,
  "stake_required": 30,
  "max_participants": 6,
  "deadline_hours": 240,
  "quality_threshold": 90,
  "verification_type": {
    "PeerReview": {
      "required_reviewers": 5,
      "consensus_threshold": 0.8
    }
  },
  "task_data": {
    "baseline_algorithm": "Dijkstra",
    "graph_characteristics": {
      "nodes": 1000000,
      "edges": 5000000,
      "graph_type": "directed_weighted",
      "edge_weight_range": [1, 100]
    },
    "optimization_targets": [
      "Reduce time complexity",
      "Lower space complexity",
      "Support parallel computing",
      "Handle dynamic graph updates"
    ],
    "performance_baselines": {
      "dijkstra_time": "2.5s",
      "dijkstra_memory": "800MB",
      "target_improvement": "50%"
    },
    "test_datasets": [
      {
        "name": "road_network",
        "size": "1M nodes",
        "download_url": "https://..."
      },
      {
        "name": "social_network",
        "size": "2M nodes",
        "download_url": "https://..."
      }
    ]
  },
  "submission_requirements": {
    "include_algorithm_implementation": true,
    "include_performance_comparison": true,
    "include_complexity_analysis": true,
    "include_parallel_version": false,
    "programming_languages": ["rust", "cpp", "go"],
    "benchmark_required": true
  }
}
```

## 5. Machine Learning Task

```json
{
  "title": "Image Classification Model Compression Optimization",
  "description": "Compress ResNet-50 model while maintaining accuracy",
  "task_type": {
    "DataAnalysis": {
      "data_type": "Image"
    }
  },
  "difficulty_level": "Expert",
  "reward_amount": 400,
  "stake_required": 40,
  "max_participants": 4,
  "deadline_hours": 336,
  "quality_threshold": 95,
  "verification_type": {
    "ExpertReview": {
      "expert_count": 3
    }
  },
  "task_data": {
    "base_model": "ResNet-50",
    "dataset": "CIFAR-10",
    "current_metrics": {
      "accuracy": 0.92,
      "model_size": "98MB",
      "inference_time": "15ms",
      "memory_usage": "2.1GB"
    },
    "optimization_goals": {
      "target_accuracy": "> 0.90",
      "target_size": "< 25MB",
      "target_inference_time": "< 8ms",
      "target_memory": "< 1GB"
    },
    "allowed_techniques": [
      "Quantization",
      "Pruning",
      "Knowledge Distillation",
      "Neural Architecture Search (NAS)",
      "Lightweight architecture design"
    ],
    "evaluation_environment": {
      "hardware": "NVIDIA RTX 3080",
      "framework": ["pytorch", "tensorflow"],
      "batch_size": 32
    }
  },
  "submission_requirements": {
    "include_compressed_model": true,
    "include_training_code": true,
    "include_evaluation_script": true,
    "include_comparison_report": true,
    "model_format": ["onnx", "torchscript"],
    "documentation_required": true
  }
}
```

## 6. task.json Field Descriptions

### Basic Fields
- **title**: Task title
- **description**: Detailed task description
- **task_type**: Task type and specific configuration
- **difficulty_level**: Difficulty level (Beginner/Intermediate/Advanced/Expert)
- **reward_amount**: Reward amount (TOS)
- **stake_required**: Required stake for participation (TOS)
- **max_participants**: Maximum number of participants
- **deadline_hours**: Deadline (in hours)
- **quality_threshold**: Quality threshold (0-100)

### Verification Configuration
- **verification_type**: Verification method
  - `Automatic`: Automatic verification
  - `PeerReview`: Peer review
  - `ExpertReview`: Expert review
  - `Hybrid`: Hybrid verification

### Task Data
- **task_data**: Specific task data
  - Code, datasets, requirements, etc.
  - Test cases and evaluation criteria
  - Performance benchmarks and targets

### Submission Requirements
- **submission_requirements**: Submission requirements
  - File types and formats
  - Size limitations
  - Required components

## 7. CLI Processing Workflow

```bash
# 1. Validate JSON format
tos-ai task validate -c task.json

# 2. Estimate costs
tos-ai task estimate -c task.json

# 3. Publish task
tos-ai task publish -c task.json

# 4. Check task status
tos-ai task status <task_id>
```

This design provides a flexible and standardized task definition format that supports various types of AI mining tasks.