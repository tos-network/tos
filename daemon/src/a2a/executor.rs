use std::sync::Arc;

use async_trait::async_trait;
use once_cell::sync::OnceCell;
use rand::RngCore;
use tokio::sync::Semaphore;

use tos_common::a2a::{
    A2AError, A2AResult, Artifact, FileContent, Message, Part, PartContent, Role, Task, TaskState,
    TaskStatus,
};

use super::now_iso_timestamp;

#[derive(Clone)]
pub struct ExecutionResult {
    pub assistant_message: Message,
    pub artifacts: Vec<Artifact>,
}

#[async_trait]
pub trait A2AExecutor: Send + Sync {
    async fn execute(&self, task: &Task, message: &Message) -> A2AResult<ExecutionResult>;
}

struct RuleBasedExecutor {
    semaphore: Arc<Semaphore>,
}

#[async_trait]
impl A2AExecutor for RuleBasedExecutor {
    async fn execute(&self, task: &Task, message: &Message) -> A2AResult<ExecutionResult> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| A2AError::InternalError {
                message: "semaphore closed".to_string(),
            })?;
        let response_text = summarize_message(message);
        let assistant_message = Message {
            message_id: new_id("msg-"),
            context_id: Some(task.context_id.clone()),
            task_id: Some(task.id.clone()),
            role: Role::Agent,
            parts: vec![text_part(format!("Processed message:\n{response_text}"))],
            metadata: None,
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        };
        let artifact = Artifact {
            artifact_id: new_id("artifact-"),
            name: Some("result".to_string()),
            description: Some("Daemon execution result".to_string()),
            parts: if message.parts.is_empty() {
                vec![text_part(response_text.clone())]
            } else {
                message.parts.clone()
            },
            metadata: None,
            extensions: Vec::new(),
        };
        Ok(ExecutionResult {
            assistant_message,
            artifacts: vec![artifact],
        })
    }
}

static EXECUTOR: OnceCell<Arc<dyn A2AExecutor>> = OnceCell::new();

pub fn set_executor(executor: Arc<dyn A2AExecutor>) {
    let _ = EXECUTOR.set(executor);
}

pub fn get_executor() -> Arc<dyn A2AExecutor> {
    EXECUTOR
        .get_or_init(|| {
            Arc::new(RuleBasedExecutor {
                semaphore: Arc::new(Semaphore::new(4)),
            })
        })
        .clone()
}

pub fn default_executor(concurrency: usize) -> Arc<dyn A2AExecutor> {
    Arc::new(RuleBasedExecutor {
        semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
    })
}

pub fn build_final_status(_task: &Task, assistant_message: Message) -> TaskStatus {
    TaskStatus {
        state: TaskState::Completed,
        message: Some(assistant_message),
        timestamp: Some(now_iso_timestamp()),
    }
}

pub fn text_part(text: String) -> Part {
    Part {
        content: PartContent::Text { text },
        metadata: None,
    }
}

pub fn summarize_message(message: &Message) -> String {
    let mut texts = Vec::new();
    let mut files = Vec::new();
    let mut data_keys = Vec::new();
    for part in &message.parts {
        match &part.content {
            PartContent::Text { text } => {
                if !text.trim().is_empty() {
                    texts.push(text.trim().to_string());
                }
            }
            PartContent::File { file } => {
                let label = match &file.file {
                    FileContent::Uri { file_with_uri } => {
                        format!("uri={}", file_with_uri)
                    }
                    FileContent::Bytes { file_with_bytes } => {
                        format!("bytes(base64_len={})", file_with_bytes.len())
                    }
                };
                if let Some(name) = file.name.as_ref() {
                    files.push(format!("{name} ({label})"));
                } else {
                    files.push(label);
                }
            }
            PartContent::Data { data } => {
                for key in data.data.keys() {
                    data_keys.push(key.clone());
                }
            }
        }
    }
    let mut summary = Vec::new();
    if texts.is_empty() {
        summary.push("Text: (none)".to_string());
    } else {
        summary.push("Text:".to_string());
        summary.extend(texts);
    }
    if files.is_empty() {
        summary.push("Files: 0".to_string());
    } else {
        summary.push(format!("Files: {}", files.join(", ")));
    }
    if data_keys.is_empty() {
        summary.push("Data keys: 0".to_string());
    } else {
        summary.push(format!("Data keys: {}", data_keys.join(", ")));
    }
    summary.join("\n")
}

fn new_id(prefix: &str) -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", hex::encode(bytes))
}
