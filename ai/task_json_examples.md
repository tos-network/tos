# AI挖矿任务配置文件 (task.json) 示例

## 1. 基础代码优化任务

```json
{
  "title": "Rust排序算法性能优化",
  "description": "优化现有冒泡排序算法，提高执行效率并保持代码可读性",
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
      "使用更高效的排序算法",
      "保持O(n log n)平均时间复杂度",
      "添加性能基准测试",
      "提供算法复杂度分析",
      "保持代码可读性"
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

## 2. 安全审计任务

```json
{
  "title": "智能合约安全漏洞审计",
  "description": "审计DeFi智能合约，发现并报告潜在的安全漏洞",
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
    "contract_code": "// Solidity合约代码...",
    "contract_address": "0x1234567890abcdef...",
    "audit_scope": [
      "重入攻击漏洞",
      "整数溢出/下溢",
      "权限控制问题",
      "逻辑错误",
      "Gas优化建议"
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

## 3. 数据分析任务

```json
{
  "title": "加密货币价格趋势分析",
  "description": "分析BTC/ETH价格数据，预测未来7天走势",
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
      "技术指标分析（RSI, MACD, 布林带）",
      "价格趋势识别",
      "支撑/阻力位计算",
      "7天价格预测",
      "风险评估"
    ],
    "evaluation_metrics": [
      "预测准确度（MAPE < 5%）",
      "模型解释能力",
      "可视化质量",
      "数据清洗完整性"
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

## 4. 算法优化任务

```json
{
  "title": "图最短路径算法优化",
  "description": "针对大规模图数据优化最短路径算法性能",
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
      "减少时间复杂度",
      "降低空间复杂度",
      "支持并行计算",
      "处理动态图更新"
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

## 5. 机器学习任务

```json
{
  "title": "图像分类模型压缩优化",
  "description": "压缩ResNet-50模型同时保持准确率",
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
      "量化(Quantization)",
      "剪枝(Pruning)",
      "知识蒸馏(Knowledge Distillation)",
      "架构搜索(NAS)",
      "轻量级架构设计"
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

## 6. task.json字段说明

### 基础字段
- **title**: 任务标题
- **description**: 任务详细描述
- **task_type**: 任务类型和具体配置
- **difficulty_level**: 难度等级(Beginner/Intermediate/Advanced/Expert)
- **reward_amount**: 奖励金额(TOS)
- **stake_required**: 参与所需质押(TOS)
- **max_participants**: 最大参与者数量
- **deadline_hours**: 截止时间(小时)
- **quality_threshold**: 质量阈值(0-100)

### 验证配置
- **verification_type**: 验证方式
  - `Automatic`: 自动验证
  - `PeerReview`: 同行评议
  - `ExpertReview`: 专家审核
  - `Hybrid`: 混合验证

### 任务数据
- **task_data**: 具体任务数据
  - 代码、数据集、要求等
  - 测试用例和评估标准
  - 性能基准和目标

### 提交要求
- **submission_requirements**: 提交要求
  - 文件类型和格式
  - 大小限制
  - 必需组件

## 7. CLI处理流程

```bash
# 1. 验证JSON格式
tos-ai task validate -c task.json

# 2. 预估费用
tos-ai task estimate -c task.json

# 3. 发布任务
tos-ai task publish -c task.json

# 4. 查看任务状态
tos-ai task status <task_id>
```

这个设计提供了灵活且标准化的任务定义格式，支持各种类型的AI挖矿任务。