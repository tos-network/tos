use std::sync::Arc;

use async_trait::async_trait;
use log::warn;
use tos_common::a2a::{
    A2AError, A2AResult, Message, SendMessageConfiguration, SendMessageRequest,
    SendMessageResponse, Task,
};

use super::executor::{A2AExecutor, ExecutionResult};
use super::registry::router::{AgentRouter, RouterConfig, RoutingStrategy};
use super::registry::{global_registry, router::extract_required_skills};

pub struct AgentRouterExecutor {
    router: AgentRouter,
    config: RouterConfig,
    local: Arc<dyn A2AExecutor>,
}

impl AgentRouterExecutor {
    pub fn new(local: Arc<dyn A2AExecutor>, config: RouterConfig) -> Self {
        let router = AgentRouter::new(global_registry()).with_config(config.clone());
        Self {
            router,
            config,
            local,
        }
    }

    fn pick_strategy(&self, message: &Message) -> RoutingStrategy {
        let Some(metadata) = message.metadata.as_ref() else {
            return self.config.strategy;
        };
        let Some(strategy) = metadata
            .get("routingStrategy")
            .or_else(|| metadata.get("routing_strategy"))
            .and_then(|v| v.as_str())
        else {
            return self.config.strategy;
        };
        match strategy {
            "first_match" | "firstMatch" => RoutingStrategy::FirstMatch,
            "lowest_latency" | "lowestLatency" => RoutingStrategy::LowestLatency,
            "highest_reputation" | "highestReputation" => RoutingStrategy::HighestReputation,
            "round_robin" | "roundRobin" => RoutingStrategy::RoundRobin,
            "weighted_random" | "weightedRandom" => RoutingStrategy::WeightedRandom,
            _ => self.config.strategy,
        }
    }

    async fn forward(
        &self,
        task: &Task,
        message: &Message,
        required_skills: &[String],
    ) -> A2AResult<ExecutionResult> {
        let strategy = self.pick_strategy(message);
        let agent = self
            .router
            .route_agent_by_skills(required_skills, strategy, true)
            .await
            .map_err(|_| A2AError::InternalError {
                message: "no available agents for required skill".to_string(),
            })?;

        let mut forwarded_message = message.clone();
        forwarded_message.task_id = None;
        forwarded_message.context_id = None;

        let request = SendMessageRequest {
            tenant: None,
            message: forwarded_message,
            configuration: Some(SendMessageConfiguration {
                accepted_output_modes: vec!["message".to_string()],
                push_notification_config: None,
                history_length: None,
                blocking: true,
            }),
            metadata: task.metadata.clone(),
        };

        let response = self.router.forward_request(&agent, request).await?;
        self.response_to_execution(task, response)
    }

    fn response_to_execution(
        &self,
        task: &Task,
        response: SendMessageResponse,
    ) -> A2AResult<ExecutionResult> {
        match response {
            SendMessageResponse::Message { mut message } => {
                message.task_id = Some(task.id.clone());
                message.context_id = Some(task.context_id.clone());
                Ok(ExecutionResult {
                    assistant_message: message,
                    artifacts: Vec::new(),
                })
            }
            SendMessageResponse::Task {
                task: mut remote_task,
            } => {
                let assistant_message = remote_task
                    .status
                    .message
                    .take()
                    .or_else(|| {
                        remote_task
                            .history
                            .iter()
                            .rev()
                            .find(|msg| matches!(msg.role, tos_common::a2a::Role::Agent))
                            .cloned()
                    })
                    .ok_or_else(|| A2AError::InvalidAgentResponseError {
                        message: "remote task missing agent message".to_string(),
                    })?;
                let mut assistant_message = assistant_message;
                assistant_message.task_id = Some(task.id.clone());
                assistant_message.context_id = Some(task.context_id.clone());
                Ok(ExecutionResult {
                    assistant_message,
                    artifacts: remote_task.artifacts,
                })
            }
        }
    }
}

#[async_trait]
impl A2AExecutor for AgentRouterExecutor {
    async fn execute(&self, task: &Task, message: &Message) -> A2AResult<ExecutionResult> {
        let required_skills = extract_required_skills(message);
        if required_skills.is_empty() {
            return self.local.execute(task, message).await;
        }

        match self.forward(task, message, &required_skills).await {
            Ok(result) => Ok(result),
            Err(err) => {
                if self.config.fallback_to_local {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("routing failed, falling back to local executor: {}", err);
                    }
                    self.local.execute(task, message).await
                } else {
                    Err(err)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::executor::{text_part, ExecutionResult};
    use tos_common::a2a::Value;
    use tos_common::a2a::{Message, Part, PartContent, Role, Task, TaskState, TaskStatus};

    struct TestExecutor;

    #[async_trait]
    impl A2AExecutor for TestExecutor {
        async fn execute(&self, task: &Task, _message: &Message) -> A2AResult<ExecutionResult> {
            let assistant_message = Message {
                message_id: "msg-local".to_string(),
                context_id: Some(task.context_id.clone()),
                task_id: Some(task.id.clone()),
                role: Role::Agent,
                parts: vec![text_part("local".to_string())],
                metadata: None,
                extensions: Vec::new(),
                reference_task_ids: Vec::new(),
            };
            Ok(ExecutionResult {
                assistant_message,
                artifacts: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn falls_back_to_local_when_no_agents() -> Result<(), Box<dyn std::error::Error>> {
        let config = RouterConfig {
            strategy: RoutingStrategy::FirstMatch,
            timeout_ms: 10,
            retry_count: 0,
            fallback_to_local: true,
        };
        let executor = AgentRouterExecutor::new(Arc::new(TestExecutor), config);

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "required_skills".to_string(),
            Value::Array(vec![Value::String("skill:missing-test-only".to_string())]),
        );

        let task = Task {
            id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: None,
            },
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata: None,
            tos_task_anchor: None,
        };

        let message = Message {
            message_id: "msg-1".to_string(),
            context_id: Some("ctx-1".to_string()),
            task_id: Some("task-1".to_string()),
            role: Role::User,
            parts: vec![Part {
                content: PartContent::Text {
                    text: "hi".to_string(),
                },
                filename: None,
                media_type: None,
                metadata: None,
            }],
            metadata: Some(metadata),
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        };

        let result = executor.execute(&task, &message).await?;
        let PartContent::Text { text } = &result.assistant_message.parts[0].content else {
            return Err("unexpected content".into());
        };
        assert_eq!(text, "local");
        Ok(())
    }
}
