pub mod auth;
pub mod executor;
pub mod grpc;
mod notify;
mod storage;

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use log::error;
use rand::RngCore;

use tos_common::{
    a2a::{
        A2AError, A2AResult, A2AService, AgentCapabilities, AgentCard, AgentInterface,
        AgentProvider, ApiKeySecurityScheme, Artifact, CancelTaskRequest,
        GetExtendedAgentCardRequest, GetTaskRequest, HttpAuthSecurityScheme, ListTasksRequest,
        ListTasksResponse, Message, OAuth2SecurityScheme, OAuthFlows, PartContent,
        PushNotificationConfig, Role, Security, SecurityScheme, SendMessageConfiguration,
        SendMessageRequest, SendMessageResponse, SetTaskPushNotificationConfigRequest,
        StreamResponse, SubscribeToTaskRequest, Task, TaskArtifactUpdateEvent,
        TaskPushNotificationConfig, TaskState, TaskStatus, TaskStatusUpdateEvent,
        TosSignatureSecurityScheme, MAX_ARTIFACTS_PER_TASK, MAX_DATA_PART_BYTES,
        MAX_FILE_INLINE_BYTES, MAX_HISTORY_LENGTH, MAX_METADATA_KEYS, MAX_PARTS_PER_MESSAGE,
        MAX_PUSH_CONFIGS_PER_TASK, MAX_TEXT_PART_BYTES,
    },
    config::VERSION,
};

use crate::core::blockchain::Blockchain;
use crate::core::storage::Storage;

use storage::{
    get_or_init, is_terminal, make_push_name, normalize_push_name, normalize_task_name,
    now_iso_timestamp, A2AStoreError,
};

pub fn set_base_dir(dir: &str) {
    storage::set_base_dir(dir);
}

pub struct A2ADaemonService<S: Storage> {
    blockchain: Arc<Blockchain<S>>,
}

impl<S: Storage> A2ADaemonService<S> {
    pub fn new(blockchain: Arc<Blockchain<S>>) -> Self {
        Self { blockchain }
    }

    fn store(&self) -> Result<&'static storage::A2AStore, A2AError> {
        get_or_init(self.blockchain.get_network()).map_err(map_store_error)
    }

    fn base_agent_card(&self) -> AgentCard {
        let base_url = std::env::var("TOS_A2A_PUBLIC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
        let grpc_url = std::env::var("TOS_A2A_GRPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:9090".to_string());
        let auth_config = auth::get_auth_config();
        let mut security_schemes = std::collections::HashMap::new();
        let mut security = Vec::new();
        if auth_config
            .as_ref()
            .map(|cfg| !cfg.api_keys.is_empty())
            .unwrap_or(false)
        {
            security_schemes.insert(
                "apiKey".to_string(),
                SecurityScheme::ApiKey {
                    api_key_security_scheme: ApiKeySecurityScheme {
                        description: Some("Authorization: Bearer <key> or x-api-key".to_string()),
                        location: "header".to_string(),
                        name: "Authorization".to_string(),
                    },
                },
            );
            security.push(Security {
                schemes: std::collections::HashMap::from([(
                    "apiKey".to_string(),
                    tos_common::a2a::StringList { list: Vec::new() },
                )]),
            });
            security_schemes.insert(
                "httpBearer".to_string(),
                SecurityScheme::HttpAuth {
                    http_auth_security_scheme: HttpAuthSecurityScheme {
                        description: Some("HTTP Bearer authentication".to_string()),
                        scheme: "bearer".to_string(),
                        bearer_format: Some("opaque".to_string()),
                    },
                },
            );
            security.push(Security {
                schemes: std::collections::HashMap::from([(
                    "httpBearer".to_string(),
                    tos_common::a2a::StringList { list: Vec::new() },
                )]),
            });
        }
        if auth_config
            .as_ref()
            .and_then(|cfg| cfg.oauth_issuer.as_ref())
            .is_some()
        {
            security_schemes.insert(
                "oauth2".to_string(),
                SecurityScheme::OAuth2 {
                    oauth2_security_scheme: OAuth2SecurityScheme {
                        description: Some("OAuth2 JWT (issuer/JWKS)".to_string()),
                        flows: OAuthFlows::ClientCredentials {
                            client_credentials: tos_common::a2a::ClientCredentialsFlow {
                                token_url: auth_config
                                    .as_ref()
                                    .and_then(|cfg| cfg.oauth_issuer.as_ref())
                                    .map(|issuer| format!("{issuer}/oauth/token"))
                                    .unwrap_or_default(),
                                refresh_url: None,
                                scopes: std::collections::HashMap::new(),
                            },
                        },
                        oauth2_metadata_url: auth_config
                            .as_ref()
                            .and_then(|cfg| cfg.oauth_issuer.as_ref())
                            .map(|issuer| format!("{issuer}/.well-known/openid-configuration")),
                    },
                },
            );
            security.push(Security {
                schemes: std::collections::HashMap::from([(
                    "oauth2".to_string(),
                    tos_common::a2a::StringList { list: Vec::new() },
                )]),
            });
        }
        // TOS signature is an optional extension scheme (not required by default per spec)
        security_schemes.insert(
            "tosSignature".to_string(),
            SecurityScheme::TosSignature {
                tos_signature_security_scheme: TosSignatureSecurityScheme {
                    description: Some(
                        "TOS signature over request metadata (optional extension)".to_string(),
                    ),
                    chain_id: self.blockchain.get_network().chain_id(),
                    allowed_signers: Vec::new(),
                },
            },
        );
        // Note: tosSignature is NOT added to required security list per spec
        // "extensions MUST NOT be required by default"

        AgentCard {
            protocol_version: "1.0".to_string(),
            name: "TOS A2A Service".to_string(),
            description: "TOS A2A bridge service".to_string(),
            version: VERSION.to_string(),
            supported_interfaces: vec![
                AgentInterface {
                    url: format!("{base_url}/json_rpc"),
                    protocol_binding: "JSONRPC".to_string(),
                    tenant: None,
                },
                AgentInterface {
                    url: format!("{base_url}/message:send"),
                    protocol_binding: "HTTP+JSON".to_string(),
                    tenant: None,
                },
                AgentInterface {
                    url: format!("{base_url}/a2a/ws"),
                    protocol_binding: "JSONRPC".to_string(), // WebSocket uses JSON-RPC protocol
                    tenant: None,
                },
                AgentInterface {
                    url: grpc_url,
                    protocol_binding: "GRPC".to_string(),
                    tenant: None,
                },
            ],
            provider: Some(AgentProvider {
                url: "https://tos.network".to_string(),
                organization: "TOS Network".to_string(),
            }),
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities {
                streaming: Some(true),
                push_notifications: Some(true),
                state_transition_history: Some(true),
                extensions: Vec::new(),
                tos_on_chain_settlement: Some(false),
            },
            security_schemes,
            security,
            default_input_modes: vec!["text/plain".to_string(), "application/json".to_string()],
            default_output_modes: vec!["text/plain".to_string(), "application/json".to_string()],
            skills: Vec::new(),
            supports_extended_agent_card: Some(true),
            signatures: Vec::new(),
            tos_identity: None,
        }
    }
}

fn map_store_error(err: A2AStoreError) -> A2AError {
    A2AError::InternalError {
        message: err.to_string(),
    }
}

fn new_id(prefix: &str) -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", hex::encode(bytes))
}

fn resolve_context_id(message: &Message) -> String {
    message.context_id.clone().unwrap_or_else(|| new_id("ctx-"))
}

fn make_status_event(task: &Task) -> StreamResponse {
    StreamResponse::StatusUpdate {
        status_update: TaskStatusUpdateEvent {
            task_id: task.id.clone(),
            context_id: task.context_id.clone(),
            status: task.status.clone(),
            r#final: is_terminal(&task.status.state),
            metadata: None,
        },
    }
}

fn make_status_event_with(
    task_id: &str,
    context_id: &str,
    status: TaskStatus,
    final_flag: bool,
) -> StreamResponse {
    StreamResponse::StatusUpdate {
        status_update: TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: context_id.to_string(),
            status,
            r#final: final_flag,
            metadata: None,
        },
    }
}

fn make_artifact_event(task_id: &str, context_id: &str, artifact: Artifact) -> StreamResponse {
    StreamResponse::ArtifactUpdate {
        artifact_update: TaskArtifactUpdateEvent {
            task_id: task_id.to_string(),
            context_id: context_id.to_string(),
            artifact,
            append: false,
            last_chunk: true,
            metadata: None,
        },
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum OutputMode {
    Stream,
    Task,
    Message,
    Artifact,
}

fn parse_output_modes(config: Option<&SendMessageConfiguration>) -> Vec<OutputMode> {
    let Some(config) = config else {
        return Vec::new();
    };
    config
        .accepted_output_modes
        .iter()
        .filter_map(|mode| match mode.to_ascii_lowercase().as_str() {
            "stream" => Some(OutputMode::Stream),
            "task" => Some(OutputMode::Task),
            "message" => Some(OutputMode::Message),
            "artifact" => Some(OutputMode::Artifact),
            _ => None,
        })
        .collect()
}

fn preferred_output_mode(modes: &[OutputMode]) -> OutputMode {
    if modes.iter().any(|mode| matches!(mode, OutputMode::Stream)) {
        OutputMode::Stream
    } else if modes.iter().any(|mode| matches!(mode, OutputMode::Task)) {
        OutputMode::Task
    } else if modes.iter().any(|mode| matches!(mode, OutputMode::Message)) {
        OutputMode::Message
    } else if modes
        .iter()
        .any(|mode| matches!(mode, OutputMode::Artifact))
    {
        OutputMode::Artifact
    } else {
        OutputMode::Task
    }
}

fn should_stream_artifacts(modes: &[OutputMode]) -> bool {
    if modes.is_empty() {
        return true;
    }
    modes.iter().any(|mode| {
        matches!(
            mode,
            OutputMode::Artifact | OutputMode::Task | OutputMode::Stream
        )
    })
}

fn blocking_enabled(config: Option<&SendMessageConfiguration>) -> bool {
    config.map(|cfg| cfg.blocking).unwrap_or(true)
}

/// Validate message against Anti-DoS limits
fn validate_message_limits(message: &Message) -> A2AResult<()> {
    // Check parts count
    if message.parts.len() > MAX_PARTS_PER_MESSAGE {
        return Err(A2AError::InvalidParams {
            message: format!(
                "message has {} parts, maximum is {}",
                message.parts.len(),
                MAX_PARTS_PER_MESSAGE
            ),
        });
    }

    // Check individual part sizes
    for (i, part) in message.parts.iter().enumerate() {
        match &part.content {
            PartContent::Text { text } => {
                if text.len() > MAX_TEXT_PART_BYTES {
                    return Err(A2AError::InvalidParams {
                        message: format!(
                            "part {} text size {} exceeds maximum {}",
                            i,
                            text.len(),
                            MAX_TEXT_PART_BYTES
                        ),
                    });
                }
            }
            PartContent::File { file } => {
                let size = match &file.file {
                    tos_common::a2a::FileContent::Bytes { file_with_bytes } => {
                        file_with_bytes.len()
                    }
                    tos_common::a2a::FileContent::Uri { .. } => 0, // URI references don't count
                };
                if size > MAX_FILE_INLINE_BYTES {
                    return Err(A2AError::InvalidParams {
                        message: format!(
                            "part {} file size {} exceeds maximum {}",
                            i, size, MAX_FILE_INLINE_BYTES
                        ),
                    });
                }
            }
            PartContent::Data { data } => {
                let size = serde_json::to_string(&data.data)
                    .map(|s| s.len())
                    .unwrap_or(0);
                if size > MAX_DATA_PART_BYTES {
                    return Err(A2AError::InvalidParams {
                        message: format!(
                            "part {} data size {} exceeds maximum {}",
                            i, size, MAX_DATA_PART_BYTES
                        ),
                    });
                }
            }
        }
    }

    // Check metadata keys count
    if let Some(metadata) = &message.metadata {
        if metadata.len() > MAX_METADATA_KEYS {
            return Err(A2AError::InvalidParams {
                message: format!(
                    "message has {} metadata keys, maximum is {}",
                    metadata.len(),
                    MAX_METADATA_KEYS
                ),
            });
        }
    }

    Ok(())
}

/// Check if adding a message would exceed history limit
fn check_history_limit(task: &Task) -> A2AResult<()> {
    if task.history.len() >= MAX_HISTORY_LENGTH {
        return Err(A2AError::InvalidParams {
            message: format!(
                "task has {} history entries, maximum is {}",
                task.history.len(),
                MAX_HISTORY_LENGTH
            ),
        });
    }
    Ok(())
}

fn register_temp_push_configs(
    store: &storage::A2AStore,
    task_id: &str,
    config: Option<PushNotificationConfig>,
) -> Result<Vec<String>, A2AStoreError> {
    let Some(config) = config else {
        return Ok(Vec::new());
    };
    let config_id = config.id.clone().unwrap_or_else(|| new_id("push-"));
    let name = make_push_name(task_id, &config_id);
    let task_config = TaskPushNotificationConfig {
        name,
        push_notification_config: config,
    };
    store.set_push_config(task_id, &config_id, task_config)?;
    Ok(vec![config_id])
}

fn cleanup_temp_push_configs(store: &storage::A2AStore, task_id: &str, config_ids: &[String]) {
    for config_id in config_ids {
        let _ = store.delete_push_config(task_id, config_id);
    }
}

async fn execute_task_flow(
    store: &storage::A2AStore,
    task_id: &str,
    context_id: &str,
    mut task: Task,
) -> A2AResult<(Task, executor::ExecutionResult)> {
    let executor = executor::get_executor();
    let message = task.history.last().ok_or(A2AError::InvalidParams {
        message: "empty message history".to_string(),
    })?;
    let result = match executor.execute(&task, message).await {
        Ok(result) => result,
        Err(err) => {
            task.status.state = TaskState::Failed;
            task.status.timestamp = Some(now_iso_timestamp());
            store.update_task(task.clone()).map_err(map_store_error)?;
            notify::notify_task_event(
                store,
                task_id,
                make_status_event_with(task_id, context_id, task.status.clone(), true),
            )
            .await;
            return Err(err);
        }
    };

    // Check history limit before adding assistant message
    check_history_limit(&task)?;
    task.history.push(result.assistant_message.clone());

    // Check artifacts limit
    let new_artifact_count = task.artifacts.len().saturating_add(result.artifacts.len());
    if new_artifact_count > MAX_ARTIFACTS_PER_TASK {
        return Err(A2AError::InvalidParams {
            message: format!(
                "task would have {} artifacts, maximum is {}",
                new_artifact_count, MAX_ARTIFACTS_PER_TASK
            ),
        });
    }
    task.artifacts.extend(result.artifacts.clone());
    task.status = executor::build_final_status(&task, result.assistant_message.clone());

    store.update_task(task.clone()).map_err(map_store_error)?;
    for artifact in result.artifacts.clone() {
        notify::notify_task_event(
            store,
            task_id,
            make_artifact_event(task_id, context_id, artifact),
        )
        .await;
    }
    notify::notify_task_event(
        store,
        task_id,
        make_status_event_with(task_id, context_id, task.status.clone(), true),
    )
    .await;

    Ok((task, result))
}

#[async_trait]
impl<S: Storage + Send + Sync + 'static> A2AService for A2ADaemonService<S> {
    type MessageStream = stream::Iter<std::vec::IntoIter<StreamResponse>>;
    type TaskStream = stream::Iter<std::vec::IntoIter<StreamResponse>>;

    async fn send_message(&self, request: SendMessageRequest) -> A2AResult<SendMessageResponse> {
        let store = self.store()?;
        let mut message = request.message;

        // Validate Anti-DoS limits
        validate_message_limits(&message)?;

        // Validate request metadata keys limit
        if let Some(metadata) = &request.metadata {
            if metadata.len() > MAX_METADATA_KEYS {
                return Err(A2AError::InvalidParams {
                    message: format!(
                        "request has {} metadata keys, maximum is {}",
                        metadata.len(),
                        MAX_METADATA_KEYS
                    ),
                });
            }
        }

        let (task_id, context_id, mut task) = if let Some(task_id) = message.task_id.clone() {
            let Some(task) = store.get_task(&task_id).map_err(map_store_error)? else {
                return Err(A2AError::TaskNotFoundError { task_id });
            };
            let context_id = task.context_id.clone();
            if let Some(inbound_context) = message.context_id.as_ref() {
                if inbound_context != &context_id {
                    return Err(A2AError::InvalidParams {
                        message: "context_id does not match task".to_string(),
                    });
                }
            }
            (task_id, context_id, task)
        } else {
            let task_id = new_id("task-");
            let context_id = resolve_context_id(&message);
            let task = Task {
                id: task_id.clone(),
                context_id: context_id.clone(),
                status: TaskStatus {
                    state: TaskState::Submitted,
                    message: None,
                    timestamp: Some(now_iso_timestamp()),
                },
                artifacts: Vec::new(),
                history: Vec::new(),
                metadata: request.metadata.clone(),
                tos_task_anchor: None,
            };
            (task_id, context_id, task)
        };

        message.task_id = Some(task_id.clone());
        message.context_id = Some(context_id.clone());
        if matches!(message.role, Role::Unspecified) {
            message.role = Role::User;
        }

        if is_terminal(&task.status.state) {
            return Err(A2AError::UnsupportedOperationError {
                reason: "task is in a terminal state".to_string(),
            });
        }

        // Check history limit before adding
        check_history_limit(&task)?;
        task.history.push(message);

        task.status.state = TaskState::Working;
        task.status.timestamp = Some(now_iso_timestamp());

        store.update_task(task.clone()).map_err(map_store_error)?;
        notify::notify_task_event(
            store,
            &task_id,
            make_status_event_with(&task_id, &context_id, task.status.clone(), false),
        )
        .await;

        let output_modes = parse_output_modes(request.configuration.as_ref());
        let blocking = blocking_enabled(request.configuration.as_ref());
        let temp_push_configs = register_temp_push_configs(
            store,
            &task_id,
            request
                .configuration
                .as_ref()
                .and_then(|config| config.push_notification_config.clone()),
        )
        .map_err(map_store_error)?;

        if !blocking {
            let store = store;
            let task_id = task_id.clone();
            let context_id = context_id.clone();
            let temp_push_configs = temp_push_configs.clone();
            let task = task.clone();
            tokio::spawn(async move {
                if let Err(err) = execute_task_flow(store, &task_id, &context_id, task).await {
                    if log::log_enabled!(log::Level::Error) {
                        error!("A2A task execution failed: {}", err);
                    }
                }
                cleanup_temp_push_configs(store, &task_id, &temp_push_configs);
            });
        }

        let (task, result) = if blocking {
            let result = execute_task_flow(store, &task_id, &context_id, task).await;
            cleanup_temp_push_configs(store, &task_id, &temp_push_configs);
            let (task, result) = result?;
            (task, Some(result))
        } else {
            (task, None)
        };

        let mut response_task = task;
        if let Some(SendMessageConfiguration { history_length, .. }) =
            request.configuration.as_ref()
        {
            if let Some(limit) = *history_length {
                let limit = limit.max(0) as usize;
                if response_task.history.len() > limit {
                    let start = response_task.history.len() - limit;
                    response_task.history = response_task.history[start..].to_vec();
                }
            }
        }

        if !blocking {
            return Ok(SendMessageResponse::Task {
                task: Box::new(response_task),
            });
        }

        match preferred_output_mode(&output_modes) {
            OutputMode::Message => {
                let message = result
                    .ok_or(A2AError::InternalError {
                        message: "missing blocking execution result".to_string(),
                    })?
                    .assistant_message;
                Ok(SendMessageResponse::Message { message })
            }
            _ => Ok(SendMessageResponse::Task {
                task: Box::new(response_task),
            }),
        }
    }

    async fn send_streaming_message(
        &self,
        request: SendMessageRequest,
    ) -> A2AResult<Self::MessageStream> {
        let store = self.store()?;
        let mut message = request.message;

        // Validate Anti-DoS limits
        validate_message_limits(&message)?;

        // Validate request metadata keys limit
        if let Some(metadata) = &request.metadata {
            if metadata.len() > MAX_METADATA_KEYS {
                return Err(A2AError::InvalidParams {
                    message: format!(
                        "request has {} metadata keys, maximum is {}",
                        metadata.len(),
                        MAX_METADATA_KEYS
                    ),
                });
            }
        }

        let (task_id, context_id, mut task) = if let Some(task_id) = message.task_id.clone() {
            let Some(task) = store.get_task(&task_id).map_err(map_store_error)? else {
                return Err(A2AError::TaskNotFoundError { task_id });
            };
            let context_id = task.context_id.clone();
            if let Some(inbound_context) = message.context_id.as_ref() {
                if inbound_context != &context_id {
                    return Err(A2AError::InvalidParams {
                        message: "context_id does not match task".to_string(),
                    });
                }
            }
            (task_id, context_id, task)
        } else {
            let task_id = new_id("task-");
            let context_id = resolve_context_id(&message);
            let task = Task {
                id: task_id.clone(),
                context_id: context_id.clone(),
                status: TaskStatus {
                    state: TaskState::Submitted,
                    message: None,
                    timestamp: Some(now_iso_timestamp()),
                },
                artifacts: Vec::new(),
                history: Vec::new(),
                metadata: request.metadata.clone(),
                tos_task_anchor: None,
            };
            (task_id, context_id, task)
        };

        message.task_id = Some(task_id.clone());
        message.context_id = Some(context_id.clone());
        if matches!(message.role, Role::Unspecified) {
            message.role = Role::User;
        }

        if is_terminal(&task.status.state) {
            return Err(A2AError::UnsupportedOperationError {
                reason: "task is in a terminal state".to_string(),
            });
        }

        // Check history limit before adding
        check_history_limit(&task)?;
        task.history.push(message);

        task.status.state = TaskState::Working;
        task.status.timestamp = Some(now_iso_timestamp());
        let working_status = task.status.clone();

        store.update_task(task.clone()).map_err(map_store_error)?;
        notify::notify_task_event(
            store,
            &task_id,
            make_status_event_with(&task_id, &context_id, working_status.clone(), false),
        )
        .await;

        let output_modes = parse_output_modes(request.configuration.as_ref());
        let temp_push_configs = register_temp_push_configs(
            store,
            &task_id,
            request
                .configuration
                .as_ref()
                .and_then(|config| config.push_notification_config.clone()),
        )
        .map_err(map_store_error)?;

        let (task, result) = execute_task_flow(store, &task_id, &context_id, task).await?;
        cleanup_temp_push_configs(store, &task_id, &temp_push_configs);

        let mut response_task = task;
        if let Some(SendMessageConfiguration { history_length, .. }) =
            request.configuration.as_ref()
        {
            if let Some(limit) = *history_length {
                let limit = limit.max(0) as usize;
                if response_task.history.len() > limit {
                    let start = response_task.history.len() - limit;
                    response_task.history = response_task.history[start..].to_vec();
                }
            }
        }

        // Build stream events per spec: Task/Message first, then status updates, artifacts, final status
        let mut events = Vec::new();

        // First event MUST be Task or Message (per A2A spec)
        let initial_task = Task {
            id: task_id.clone(),
            context_id: context_id.clone(),
            status: working_status.clone(),
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata: None,
            tos_task_anchor: None,
        };
        events.push(StreamResponse::Task { task: initial_task });

        // Stream artifacts if requested
        if should_stream_artifacts(&output_modes) {
            for artifact in result.artifacts.clone() {
                events.push(make_artifact_event(&task_id, &context_id, artifact));
            }
        }

        // Final status update (always required per spec)
        events.push(make_status_event_with(
            &task_id,
            &context_id,
            response_task.status.clone(),
            true,
        ));

        // Final Task or Message based on output mode
        match preferred_output_mode(&output_modes) {
            OutputMode::Message => events.push(StreamResponse::Message {
                message: result.assistant_message.clone(),
            }),
            _ => events.push(StreamResponse::Task {
                task: response_task,
            }),
        }
        Ok(stream::iter(events))
    }

    async fn get_task(&self, request: GetTaskRequest) -> A2AResult<Task> {
        let task_id =
            normalize_task_name(&request.name).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid task name".to_string(),
            })?;
        let store = self.store()?;
        let Some(mut task) = store.get_task(task_id).map_err(map_store_error)? else {
            return Err(A2AError::TaskNotFoundError {
                task_id: task_id.to_string(),
            });
        };
        if let Some(limit) = request.history_length {
            let limit = limit.max(0) as usize;
            if task.history.len() > limit {
                let start = task.history.len() - limit;
                task.history = task.history[start..].to_vec();
            }
        }
        Ok(task)
    }

    async fn list_tasks(&self, request: ListTasksRequest) -> A2AResult<ListTasksResponse> {
        let store = self.store()?;
        store.list_tasks(&request).map_err(map_store_error)
    }

    async fn cancel_task(&self, request: CancelTaskRequest) -> A2AResult<Task> {
        let task_id =
            normalize_task_name(&request.name).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid task name".to_string(),
            })?;
        let store = self.store()?;
        let Some(mut task) = store.get_task(task_id).map_err(map_store_error)? else {
            return Err(A2AError::TaskNotFoundError {
                task_id: task_id.to_string(),
            });
        };
        task.status.state = TaskState::Cancelled;
        task.status.timestamp = Some(now_iso_timestamp());
        store.update_task(task.clone()).map_err(map_store_error)?;
        notify::notify_task_event(
            store,
            task_id,
            make_status_event_with(&task.id, &task.context_id, task.status.clone(), true),
        )
        .await;
        Ok(task)
    }

    async fn subscribe_to_task(
        &self,
        request: SubscribeToTaskRequest,
    ) -> A2AResult<Self::TaskStream> {
        let task_id =
            normalize_task_name(&request.name).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid task name".to_string(),
            })?;
        let store = self.store()?;
        let Some(task) = store.get_task(task_id).map_err(map_store_error)? else {
            return Err(A2AError::TaskNotFoundError {
                task_id: task_id.to_string(),
            });
        };
        let events = vec![
            StreamResponse::Task { task: task.clone() },
            make_status_event(&task),
        ];
        Ok(stream::iter(events))
    }

    async fn set_task_push_notification_config(
        &self,
        request: SetTaskPushNotificationConfigRequest,
    ) -> A2AResult<TaskPushNotificationConfig> {
        let task_id =
            normalize_task_name(&request.parent).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid task parent".to_string(),
            })?;
        let config_id = request.config_id.clone();
        let mut config = request.config.clone();
        config.name = make_push_name(task_id, &config_id);
        let store = self.store()?;

        // Check push config count limit
        let existing = store
            .list_push_configs(task_id, Some(MAX_PUSH_CONFIGS_PER_TASK as i32), None)
            .map_err(map_store_error)?;
        if existing.configs.len() >= MAX_PUSH_CONFIGS_PER_TASK {
            return Err(A2AError::InvalidParams {
                message: format!(
                    "task has {} push configs, maximum is {}",
                    existing.configs.len(),
                    MAX_PUSH_CONFIGS_PER_TASK
                ),
            });
        }

        store
            .set_push_config(task_id, &config_id, config.clone())
            .map_err(map_store_error)?;
        Ok(config)
    }

    async fn get_task_push_notification_config(
        &self,
        request: tos_common::a2a::GetTaskPushNotificationConfigRequest,
    ) -> A2AResult<TaskPushNotificationConfig> {
        let (task_id, config_id) =
            normalize_push_name(&request.name).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid push config name".to_string(),
            })?;
        let store = self.store()?;
        let Some(config) = store
            .get_push_config(task_id, config_id)
            .map_err(map_store_error)?
        else {
            return Err(A2AError::TaskNotFoundError {
                task_id: task_id.to_string(),
            });
        };
        Ok(config)
    }

    async fn list_task_push_notification_config(
        &self,
        request: tos_common::a2a::ListTaskPushNotificationConfigRequest,
    ) -> A2AResult<tos_common::a2a::ListTaskPushNotificationConfigResponse> {
        let task_id =
            normalize_task_name(&request.parent).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid task parent".to_string(),
            })?;
        let store = self.store()?;
        store
            .list_push_configs(task_id, request.page_size, request.page_token)
            .map_err(map_store_error)
    }

    async fn delete_task_push_notification_config(
        &self,
        request: tos_common::a2a::DeleteTaskPushNotificationConfigRequest,
    ) -> A2AResult<()> {
        let (task_id, config_id) =
            normalize_push_name(&request.name).ok_or_else(|| A2AError::InvalidParams {
                message: "invalid push config name".to_string(),
            })?;
        let store = self.store()?;
        store
            .delete_push_config(task_id, config_id)
            .map_err(map_store_error)?;
        Ok(())
    }

    async fn get_extended_agent_card(
        &self,
        _request: GetExtendedAgentCardRequest,
    ) -> A2AResult<AgentCard> {
        Ok(self.base_agent_card())
    }
}
