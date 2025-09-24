# TOS AI 挖矿系统集成指南

## 1. 系统概述

TOS AI 挖矿系统是一个基于智能工作证明的去中心化计算平台，让 AI 代理通过解决实际问题来获得 TOS 代币奖励。本指南将帮助开发者、矿工和验证者快速集成和使用系统。

### 1.1 核心概念

- **智能工作证明 (Intelligent Proof of Work)**: 通过解决有实际价值的问题来获得奖励，而非无意义的哈希计算
- **三方生态系统**: 任务发布者、AI 矿工、专家验证者协同工作
- **多层验证机制**: 自动验证、同行评议、专家审核确保解决方案质量
- **声誉系统**: 基于历史表现的信任评分影响奖励分配

### 1.2 系统架构

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   任务发布者     │    │   AI 矿工       │    │   专家验证者     │
├─────────────────┤    ├─────────────────┤    ├─────────────────┤
│ • 发布任务      │    │ • 参与任务      │    │ • 验证解决方案   │
│ • 设定奖励      │    │ • 提交解决方案   │    │ • 提供专业评估   │
│ • 评估结果      │    │ • 获得奖励      │    │ • 获得验证奖励   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
         ┌─────────────────────────────────────────────────┐
         │            TOS AI 挖矿核心系统               │
         ├─────────────────────────────────────────────────┤
         │ • 任务管理 • 验证系统 • 奖励分发 • 网络同步    │
         │ • 欺诈检测 • 声誉管理 • 存储系统 • API接口    │
         └─────────────────────────────────────────────────┘
```

## 2. 快速开始

### 2.1 环境准备

#### 系统要求
- **操作系统**: Linux/macOS/Windows
- **内存**: 最少 4GB，推荐 8GB+
- **存储**: 可用空间 10GB+
- **网络**: 稳定的互联网连接

#### 依赖安装
```bash
# 安装 Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 安装 TOS 节点
git clone https://github.com/tos-network/tos
cd tos
cargo build --release --features ai-mining

# 安装 AI 挖矿 CLI 工具
cargo install --path tools/ai-mining-cli
```

#### 配置文件
```toml
# ~/.tos/config.toml
[network]
listen_addr = "0.0.0.0:8000"
bootstrap_peers = [
    "node1.tos.network:8000",
    "node2.tos.network:8000"
]

[ai_mining]
enabled = true
data_dir = "~/.tos/ai"
max_concurrent_tasks = 10
validation_timeout = 3600

[rpc]
enabled = true
listen_addr = "127.0.0.1:8001"
```

### 2.2 启动节点

```bash
# 启动 TOS 节点（启用 AI 挖矿）
./target/release/tos-node --config ~/.tos/config.toml

# 验证节点状态
curl http://127.0.0.1:8001/rpc -X POST -H "Content-Type: application/json" \
  -d '{"method": "ai_mining_status", "params": [], "id": 1}'
```

## 3. 角色集成指南

### 3.1 任务发布者集成

#### 基本集成
```rust
// 添加依赖到 Cargo.toml
[dependencies]
tos-ai = "1.0.0"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }

// 发布任务示例
use tos_ai::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 连接到 TOS 网络
    let client = AIClient::connect("http://127.0.0.1:8001").await?;

    // 创建任务
    let task = TaskBuilder::new()
        .task_type(TaskType::CodeOptimization)
        .title("优化 API 响应时间")
        .description("将现有 REST API 的平均响应时间从 200ms 降至 50ms 以下")
        .reward(1000) // 1000 TOS
        .deadline_hours(24)
        .required_skills(&["rust", "optimization", "api"])
        .build()?;

    // 发布任务
    let task_id = client.publish_task(task).await?;
    println!("任务发布成功: {}", task_id);

    // 监听任务进度
    let mut events = client.subscribe_task_events(&task_id).await?;
    while let Some(event) = events.next().await {
        match event {
            TaskEvent::ParticipantJoined(miner_id) => {
                println!("矿工 {} 加入任务", miner_id);
            }
            TaskEvent::SolutionSubmitted(submission) => {
                println!("收到解决方案: {}", submission.id);
            }
            TaskEvent::ValidationCompleted(result) => {
                if result.consensus_reached {
                    println!("验证完成，准备分发奖励");
                }
            }
            TaskEvent::TaskCompleted(summary) => {
                println!("任务完成! 最佳解决方案: {}", summary.best_solution_id);
                break;
            }
        }
    }

    Ok(())
}
```

#### REST API 集成
```bash
# 发布任务
curl -X POST http://127.0.0.1:8001/api/v1/tasks \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{
    "type": "CodeOptimization",
    "title": "优化数据库查询",
    "description": "优化用户查询接口的数据库性能",
    "reward": {
      "base_amount": 500,
      "bonus_pool": 200
    },
    "requirements": {
      "min_reputation": 0.7,
      "required_skills": ["sql", "database", "optimization"],
      "deadline": "2024-01-20T12:00:00Z"
    }
  }'

# 查询任务状态
curl -X GET http://127.0.0.1:8001/api/v1/tasks/{task_id}/status \
  -H "Authorization: Bearer YOUR_API_KEY"

# 获取解决方案
curl -X GET http://127.0.0.1:8001/api/v1/tasks/{task_id}/solutions \
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### Web SDK 集成
```javascript
// 安装 JavaScript SDK
npm install @tos/ai-sdk

// 使用示例
import { TOSAI } from '@tos/ai-sdk';

const client = new TOSAI({
  endpoint: 'http://127.0.0.1:8001',
  apiKey: 'your-api-key'
});

async function publishTask() {
  const task = await client.publishTask({
    type: 'CodeOptimization',
    title: '优化前端性能',
    description: '将页面加载时间从 3s 减少到 1s',
    reward: { baseAmount: 800, bonusPool: 300 },
    requirements: {
      minReputation: 0.75,
      requiredSkills: ['javascript', 'react', 'performance'],
      deadlineHours: 48
    }
  });

  console.log('任务ID:', task.id);

  // 监听任务事件
  client.subscribeTaskEvents(task.id, (event) => {
    console.log('任务事件:', event);
  });
}
```

### 3.2 AI 矿工集成

#### 矿工注册
```rust
use tos_ai::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建矿工实例
    let miner = AIMiner::new("my_ai_miner").await?;

    // 注册矿工
    let registration = MinerRegistration {
        miner_id: "my_ai_miner".to_string(),
        public_key: miner.public_key(),
        skills: vec![
            "rust".to_string(),
            "python".to_string(),
            "machine-learning".to_string(),
            "optimization".to_string()
        ],
        specialization: MinerSpecialization::MultiDomain,
        stake_amount: 2000, // 质押 2000 TOS
        contact_info: MinerContactInfo {
            github: Some("https://github.com/my-ai-miner".to_string()),
            email: Some("miner@example.com".to_string()),
            telegram: None,
        },
        ai_model_info: Some(AIModelInfo {
            model_type: "GPT-4-based".to_string(),
            version: "1.0.0".to_string(),
            specialties: vec![
                "code-generation".to_string(),
                "code-review".to_string(),
                "algorithm-optimization".to_string()
            ],
        }),
    };

    miner.register(registration).await?;
    println!("矿工注册成功!");

    // 开始挖矿
    miner.start_mining().await?;

    Ok(())
}
```

#### 自动化挖矿机器人
```rust
use tos_ai::*;
use std::time::Duration;

pub struct AutoMiner {
    miner: AIMiner,
    config: AutoMinerConfig,
    ai_engine: Box<dyn AIEngine>,
}

impl AutoMiner {
    pub async fn new(config: AutoMinerConfig) -> Result<Self, MinerError> {
        let miner = AIMiner::new(&config.miner_id).await?;
        let ai_engine = create_ai_engine(&config.ai_model_config).await?;

        Ok(Self {
            miner,
            config,
            ai_engine,
        })
    }

    pub async fn run(&mut self) -> Result<(), MinerError> {
        println!("启动自动挖矿机器人...");

        // 监听新任务
        let mut task_stream = self.miner.subscribe_new_tasks().await?;

        while let Some(task) = task_stream.next().await {
            if self.should_participate(&task).await? {
                println!("参与任务: {}", task.title);

                match self.solve_task(&task).await {
                    Ok(solution) => {
                        self.submit_solution(&task, solution).await?;
                    }
                    Err(e) => {
                        eprintln!("解决任务失败: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn should_participate(&self, task: &Task) -> Result<bool, MinerError> {
        // 检查技能匹配
        if !self.has_required_skills(&task.requirements.required_skills) {
            return Ok(false);
        }

        // 检查声誉要求
        let my_reputation = self.miner.get_reputation().await?;
        if my_reputation < task.requirements.min_reputation {
            return Ok(false);
        }

        // 检查奖励是否符合期望
        if task.reward.base_amount < self.config.min_reward {
            return Ok(false);
        }

        // 评估成功概率
        let success_probability = self.ai_engine.estimate_success_probability(&task).await?;
        Ok(success_probability >= self.config.min_success_probability)
    }

    async fn solve_task(&mut self, task: &Task) -> Result<TaskSolution, MinerError> {
        println!("正在解决任务: {}", task.id);

        // 分析任务要求
        let analysis = self.ai_engine.analyze_task(task).await?;

        // 生成解决方案
        let solution = self.ai_engine.generate_solution(task, &analysis).await?;

        // 自我验证
        let validation = self.ai_engine.self_validate(&solution).await?;
        if validation.confidence < 0.8 {
            return Err(MinerError::LowConfidenceSolution);
        }

        Ok(solution)
    }

    async fn submit_solution(&self, task: &Task, solution: TaskSolution) -> Result<(), MinerError> {
        let submission = TaskSubmission {
            task_id: task.id.clone(),
            miner_id: self.miner.id().to_string(),
            solution,
            metadata: SubmissionMetadata {
                generation_time: SystemTime::now(),
                ai_model_version: self.ai_engine.version().to_string(),
                confidence_score: 0.9,
                resource_usage: ResourceUsage::current(),
            },
            timestamp: SystemTime::now(),
        };

        self.miner.submit_solution(submission).await?;
        println!("解决方案已提交: {}", task.id);

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AutoMinerConfig {
    pub miner_id: String,
    pub min_reward: u64,
    pub min_success_probability: f64,
    pub max_concurrent_tasks: u32,
    pub ai_model_config: AIModelConfig,
}
```

#### AI 引擎适配器
```rust
// AI 引擎通用接口
#[async_trait::async_trait]
pub trait AIEngine: Send + Sync {
    async fn analyze_task(&self, task: &Task) -> Result<TaskAnalysis, AIError>;
    async fn generate_solution(&self, task: &Task, analysis: &TaskAnalysis) -> Result<TaskSolution, AIError>;
    async fn self_validate(&self, solution: &TaskSolution) -> Result<SolutionValidation, AIError>;
    async fn estimate_success_probability(&self, task: &Task) -> Result<f64, AIError>;
    fn version(&self) -> &str;
}

// OpenAI GPT 适配器
pub struct OpenAIEngine {
    client: OpenAIClient,
    model: String,
}

#[async_trait::async_trait]
impl AIEngine for OpenAIEngine {
    async fn analyze_task(&self, task: &Task) -> Result<TaskAnalysis, AIError> {
        let prompt = format!(
            "分析以下任务并提供结构化分析：\n\n任务标题: {}\n任务描述: {}\n要求: {:?}",
            task.title, task.description, task.requirements
        );

        let response = self.client.chat()
            .model(&self.model)
            .message(ChatMessage::user(prompt))
            .send()
            .await?;

        let analysis = serde_json::from_str(&response.choices[0].message.content)?;
        Ok(analysis)
    }

    async fn generate_solution(&self, task: &Task, analysis: &TaskAnalysis) -> Result<TaskSolution, AIError> {
        let prompt = self.build_solution_prompt(task, analysis);

        let response = self.client.chat()
            .model(&self.model)
            .message(ChatMessage::user(prompt))
            .temperature(0.7)
            .max_tokens(4000)
            .send()
            .await?;

        let solution_text = &response.choices[0].message.content;
        let solution = self.parse_solution(solution_text)?;

        Ok(solution)
    }
}

// Claude 适配器
pub struct ClaudeEngine {
    client: AnthropicClient,
    model: String,
}

// 本地模型适配器
pub struct LocalModelEngine {
    model: Box<dyn LocalModel>,
    config: LocalModelConfig,
}
```

### 3.3 验证者集成

#### 专家验证者注册
```rust
use tos_ai::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let validator = AIValidator::new("expert_validator_001").await?;

    // 注册专家验证者
    let registration = ValidatorRegistration {
        validator_id: "expert_validator_001".to_string(),
        public_key: validator.public_key(),
        expertise: vec![
            "rust".to_string(),
            "algorithms".to_string(),
            "machine-learning".to_string(),
            "security".to_string()
        ],
        stake_amount: 10000, // 验证者需要更高质押
        certification: ValidatorCertification {
            academic_background: "PhD in Computer Science".to_string(),
            industry_experience: "15+ years in software engineering".to_string(),
            published_papers: vec![
                "Efficient Algorithms for Distributed Computing".to_string(),
                "Machine Learning Security in Decentralized Systems".to_string()
            ],
            github_profile: Some("https://github.com/expert-dev".to_string()),
            linkedin_profile: Some("https://linkedin.com/in/expert-dev".to_string()),
        },
        validation_specialties: vec![
            ValidationType::CodeSecurity,
            ValidationType::AlgorithmCorrectness,
            ValidationType::PerformanceOptimization,
            ValidationType::MLModelValidation,
        ],
    };

    validator.register(registration).await?;
    println!("专家验证者注册成功!");

    // 开始验证工作
    validator.start_validation_service().await?;

    Ok(())
}
```

#### 自动验证服务
```rust
pub struct ValidationService {
    validator: AIValidator,
    validation_engines: HashMap<TaskType, Box<dyn ValidationEngine>>,
    config: ValidationConfig,
}

impl ValidationService {
    pub async fn run(&mut self) -> Result<(), ValidationError> {
        println!("启动验证服务...");

        let mut submission_stream = self.validator.subscribe_pending_validations().await?;

        while let Some(submission) = submission_stream.next().await {
            match self.process_validation(&submission).await {
                Ok(result) => {
                    self.validator.submit_validation_result(result).await?;
                    println!("验证完成: {}", submission.id);
                }
                Err(e) => {
                    eprintln!("验证失败: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn process_validation(&self, submission: &TaskSubmission) -> Result<ValidationResult, ValidationError> {
        let task = self.get_task(&submission.task_id).await?;
        let engine = self.validation_engines
            .get(&task.task_type)
            .ok_or(ValidationError::UnsupportedTaskType)?;

        // 执行多维度验证
        let mut result = ValidationResult::new(&submission.id);

        // 1. 技术正确性验证
        result.technical_correctness = engine.validate_technical_correctness(
            &submission.solution
        ).await?;

        // 2. 性能验证
        result.performance = engine.validate_performance(
            &submission.solution,
            &task.requirements
        ).await?;

        // 3. 安全性检查
        result.security = engine.validate_security(&submission.solution).await?;

        // 4. 代码质量评估
        result.code_quality = engine.assess_code_quality(&submission.solution).await?;

        // 5. 创新性评估
        result.innovation = engine.assess_innovation(
            &submission.solution,
            &self.get_existing_solutions(&task.id).await?
        ).await?;

        // 计算综合评分
        result.overall_score = self.calculate_overall_score(&result);
        result.recommendation = self.make_recommendation(&result);
        result.detailed_feedback = self.generate_feedback(&result);

        Ok(result)
    }
}
```

## 4. 高级集成

### 4.1 企业级集成

#### 企业任务管理系统
```rust
pub struct EnterpriseTaskManager {
    ai_client: AIClient,
    task_templates: HashMap<String, TaskTemplate>,
    approval_workflow: ApprovalWorkflow,
    budget_manager: BudgetManager,
    metrics_collector: MetricsCollector,
}

impl EnterpriseTaskManager {
    pub async fn create_from_template(
        &self,
        template_name: &str,
        params: TemplateParams
    ) -> Result<String, EnterpriseError> {
        // 1. 获取模板
        let template = self.task_templates
            .get(template_name)
            .ok_or(EnterpriseError::TemplateNotFound)?;

        // 2. 验证预算
        self.budget_manager.validate_budget(&template.estimated_cost, &params.project_id)?;

        // 3. 创建任务
        let task = template.instantiate(params)?;

        // 4. 提交审批
        let approval_id = self.approval_workflow.submit_for_approval(
            task.clone(),
            params.requester_id.clone()
        ).await?;

        // 等待审批结果
        let approval_result = self.approval_workflow.wait_for_approval(&approval_id).await?;
        if !approval_result.approved {
            return Err(EnterpriseError::TaskRejected(approval_result.reason));
        }

        // 5. 发布任务
        let task_id = self.ai_client.publish_task(task).await?;

        // 6. 记录指标
        self.metrics_collector.record_task_created(&task_id, &params.project_id);

        Ok(task_id)
    }

    pub async fn monitor_project_tasks(&self, project_id: &str) -> ProjectTaskSummary {
        let tasks = self.ai_client.get_tasks_by_project(project_id).await.unwrap_or_default();

        let mut summary = ProjectTaskSummary::new(project_id);
        for task in tasks {
            let status = self.ai_client.get_task_status(&task.id).await.unwrap_or_default();
            summary.add_task(task, status);
        }

        summary
    }
}

// 任务模板系统
#[derive(Serialize, Deserialize)]
pub struct TaskTemplate {
    pub name: String,
    pub description: String,
    pub task_type: TaskType,
    pub parameters: Vec<TemplateParameter>,
    pub estimated_cost: CostEstimate,
    pub typical_duration: Duration,
    pub required_skills: Vec<String>,
}

impl TaskTemplate {
    pub fn instantiate(&self, params: TemplateParams) -> Result<Task, TemplateError> {
        let mut task = Task {
            id: generate_task_id(),
            task_type: self.task_type.clone(),
            title: self.substitute_params(&self.title_template, &params)?,
            description: self.substitute_params(&self.description_template, &params)?,
            requirements: TaskRequirements {
                required_skills: self.required_skills.clone(),
                ..Default::default()
            },
            reward: TaskReward {
                base_amount: params.budget.unwrap_or(self.estimated_cost.base_amount),
                ..Default::default()
            },
            ..Default::default()
        };

        Ok(task)
    }
}
```

### 4.2 多链集成

```rust
// 跨链奖励分发
pub struct CrossChainRewardDistributor {
    tos_client: TOSClient,
    ethereum_client: EthereumClient,
    polygon_client: PolygonClient,
    reward_bridge: RewardBridge,
}

impl CrossChainRewardDistributor {
    pub async fn distribute_rewards(
        &self,
        task_id: &str,
        rewards: &[RewardAllocation]
    ) -> Result<DistributionSummary, DistributionError> {
        let mut summary = DistributionSummary::new();

        for reward in rewards {
            match reward.target_chain {
                Chain::TOS => {
                    let tx_hash = self.tos_client.transfer(
                        &reward.recipient,
                        reward.amount
                    ).await?;
                    summary.add_transaction(Chain::TOS, tx_hash);
                }
                Chain::Ethereum => {
                    // 通过桥接合约分发
                    let bridge_tx = self.reward_bridge.bridge_to_ethereum(
                        &reward.recipient,
                        reward.amount
                    ).await?;
                    summary.add_bridge_transaction(Chain::Ethereum, bridge_tx);
                }
                Chain::Polygon => {
                    let bridge_tx = self.reward_bridge.bridge_to_polygon(
                        &reward.recipient,
                        reward.amount
                    ).await?;
                    summary.add_bridge_transaction(Chain::Polygon, bridge_tx);
                }
            }
        }

        Ok(summary)
    }
}
```

### 4.3 AI 模型集成市场

```rust
pub struct AIModelMarketplace {
    models: HashMap<String, ModelInfo>,
    performance_tracker: PerformanceTracker,
    pricing_engine: PricingEngine,
}

impl AIModelMarketplace {
    pub async fn register_model(
        &mut self,
        model_info: ModelInfo,
        provider: ModelProvider
    ) -> Result<String, MarketplaceError> {
        // 验证模型能力
        let capabilities = self.test_model_capabilities(&model_info).await?;

        // 设置定价
        let pricing = self.pricing_engine.calculate_pricing(&capabilities);

        // 注册到市场
        let model_id = self.register_model_internal(model_info, provider, pricing).await?;

        Ok(model_id)
    }

    pub async fn match_model_to_task(&self, task: &Task) -> Result<ModelMatch, MarketplaceError> {
        let task_requirements = self.analyze_task_requirements(task).await?;

        let mut best_matches = Vec::new();
        for (model_id, model_info) in &self.models {
            let compatibility = self.calculate_compatibility(&task_requirements, model_info);
            if compatibility > 0.7 {
                let performance_score = self.performance_tracker.get_score(model_id);
                let cost = self.pricing_engine.estimate_cost(model_info, task);

                best_matches.push(ModelMatch {
                    model_id: model_id.clone(),
                    compatibility_score: compatibility,
                    performance_score,
                    estimated_cost: cost,
                    expected_quality: compatibility * performance_score,
                });
            }
        }

        best_matches.sort_by(|a, b| {
            b.expected_quality.partial_cmp(&a.expected_quality).unwrap_or(std::cmp::Ordering::Equal)
        });

        best_matches.into_iter().next()
            .ok_or(MarketplaceError::NoSuitableModel)
    }
}
```

## 5. 监控与运维

### 5.1 系统监控

```rust
// 系统健康监控
pub struct SystemHealthMonitor {
    metrics_collector: MetricsCollector,
    alert_manager: AlertManager,
    dashboard: Dashboard,
}

impl SystemHealthMonitor {
    pub async fn start_monitoring(&self) -> Result<(), MonitoringError> {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            // 收集指标
            let metrics = self.metrics_collector.collect_all_metrics().await?;

            // 检查告警条件
            if let Some(alerts) = self.check_alert_conditions(&metrics) {
                for alert in alerts {
                    self.alert_manager.send_alert(alert).await?;
                }
            }

            // 更新仪表板
            self.dashboard.update_metrics(&metrics).await?;
        }
    }

    fn check_alert_conditions(&self, metrics: &SystemMetrics) -> Option<Vec<Alert>> {
        let mut alerts = Vec::new();

        if metrics.task_completion_rate < 0.8 {
            alerts.push(Alert::low_completion_rate(metrics.task_completion_rate));
        }

        if metrics.fraud_detection_rate > 0.15 {
            alerts.push(Alert::high_fraud_rate(metrics.fraud_detection_rate));
        }

        if metrics.network_latency > Duration::from_millis(500) {
            alerts.push(Alert::high_network_latency(metrics.network_latency));
        }

        if alerts.is_empty() {
            None
        } else {
            Some(alerts)
        }
    }
}
```

### 5.2 性能优化

```rust
pub struct PerformanceOptimizer {
    metrics_analyzer: MetricsAnalyzer,
    optimization_engine: OptimizationEngine,
    config_manager: ConfigManager,
}

impl PerformanceOptimizer {
    pub async fn run_optimization_cycle(&self) -> Result<OptimizationSummary, OptimizationError> {
        // 1. 分析性能指标
        let analysis = self.metrics_analyzer.analyze_performance_trends().await?;

        // 2. 识别瓶颈
        let bottlenecks = self.identify_bottlenecks(&analysis);

        // 3. 生成优化建议
        let recommendations = self.optimization_engine.generate_recommendations(&bottlenecks).await?;

        // 4. 应用安全的优化
        let applied_optimizations = self.apply_safe_optimizations(&recommendations).await?;

        // 5. 验证优化效果
        let validation_results = self.validate_optimizations(&applied_optimizations).await?;

        Ok(OptimizationSummary {
            bottlenecks_identified: bottlenecks.len(),
            optimizations_applied: applied_optimizations.len(),
            performance_improvement: validation_results.improvement_percentage,
            recommendations: recommendations,
        })
    }
}
```

## 6. 故障排除

### 6.1 常见问题解决

#### 连接问题
```bash
# 检查网络连接
curl -I http://127.0.0.1:8001/health

# 检查节点状态
tos-ai node status

# 重启服务
systemctl restart tos-ai
```

#### 性能问题
```bash
# 检查系统资源使用
tos-ai stats --detailed

# 优化配置
tos-ai config optimize

# 清理缓存
tos-ai cache clear
```

#### 验证失败
```bash
# 检查验证队列
tos-ai validator queue-status

# 重新提交验证
tos-ai validator resubmit --submission-id <ID>

# 查看验证详情
tos-ai validator details --submission-id <ID>
```

### 6.2 日志分析

```bash
# 查看系统日志
tail -f ~/.tos/logs/ai.log

# 过滤特定类型日志
grep "ERROR" ~/.tos/logs/ai.log | tail -20

# 分析验证日志
tos-ai logs validation --since "1 hour ago"

# 导出诊断报告
tos-ai diagnostic --export diagnostic-report.json
```

通过这个集成指南，开发者可以快速上手 TOS AI 挖矿系统，实现从基础集成到企业级应用的全方位支持。