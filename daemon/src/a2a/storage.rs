use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use once_cell::sync::OnceCell;
use rocksdb::{IteratorMode, Options, DB};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use tos_common::a2a::{
    ListTaskPushNotificationConfigResponse, ListTasksRequest, ListTasksResponse, Task,
    TaskPushNotificationConfig, TaskState, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE,
};
use tos_common::network::Network;

const TASK_PREFIX: &str = "task:";
const PUSH_PREFIX: &str = "push:";

#[derive(Debug, Error)]
pub enum A2AStoreError {
    #[error("rocksdb error: {0}")]
    RocksDb(#[from] rocksdb::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid page token")]
    InvalidPageToken,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone)]
pub struct A2AStore {
    db: Arc<DB>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskRecord {
    task: Task,
    created_at: i64,
    updated_at: i64,
}

static STORE: OnceCell<A2AStore> = OnceCell::new();
static BASE_DIR: OnceCell<PathBuf> = OnceCell::new();

pub fn set_base_dir(dir: &str) {
    let _ = BASE_DIR.set(PathBuf::from(dir));
}

pub fn get_or_init(network: &Network) -> Result<&'static A2AStore, A2AStoreError> {
    STORE.get_or_try_init(|| {
        let base_dir = BASE_DIR.get_or_init(|| PathBuf::from(""));
        let mut path = base_dir.clone();
        path.push("a2a");
        let network = network.to_string().to_lowercase();
        path.push(network);
        A2AStore::open(&path)
    })
}

impl A2AStore {
    pub fn open(path: &Path) -> Result<Self, A2AStoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        } else {
            fs::create_dir_all(path)?;
        }

        let mut opts = Options::default();
        opts.create_if_missing(true);

        let db = DB::open(&opts, path)?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn update_task(&self, task: Task) -> Result<(), A2AStoreError> {
        let now = Utc::now().timestamp();
        let key = task_key(&task.id);
        let created_at = self
            .db
            .get(&key)?
            .and_then(|raw| serde_json::from_slice::<TaskRecord>(&raw).ok())
            .map(|record| record.created_at)
            .unwrap_or(now);
        let record = TaskRecord {
            task,
            created_at,
            updated_at: now,
        };
        let payload = serde_json::to_vec(&record)?;
        self.db.put(key, payload)?;
        Ok(())
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>, A2AStoreError> {
        let key = task_key(task_id);
        let Some(raw) = self.db.get(key)? else {
            return Ok(None);
        };
        let record: TaskRecord = serde_json::from_slice(&raw)?;
        Ok(Some(record.task))
    }

    pub fn list_tasks(
        &self,
        request: &ListTasksRequest,
    ) -> Result<ListTasksResponse, A2AStoreError> {
        let page_size = request
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE as i32)
            .clamp(1, MAX_PAGE_SIZE as i32) as usize;

        let offset = match request.page_token.as_deref() {
            Some(token) if !token.is_empty() => token
                .parse::<usize>()
                .map_err(|_| A2AStoreError::InvalidPageToken)?,
            _ => 0,
        };

        let mut tasks: Vec<TaskRecord> = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item?;
            if !key.starts_with(TASK_PREFIX.as_bytes()) {
                continue;
            }
            let record: TaskRecord = serde_json::from_slice(&value)?;
            if let Some(context_id) = request.context_id.as_ref() {
                if &record.task.context_id != context_id {
                    continue;
                }
            }
            if let Some(status) = request.status.as_ref() {
                if &record.task.status.state != status {
                    continue;
                }
            }
            if let Some(ref ts_str) = request.status_timestamp_after {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                    if record.updated_at <= dt.timestamp() {
                        continue;
                    }
                }
            }
            tasks.push(record);
        }

        tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        let total_size = tasks.len();
        let page_tasks = tasks
            .into_iter()
            .skip(offset)
            .take(page_size)
            .map(|mut record| {
                if request.include_artifacts == Some(false) {
                    record.task.artifacts = Vec::new();
                }
                if let Some(history_length) = request.history_length {
                    let limit = history_length.max(0) as usize;
                    if record.task.history.len() > limit {
                        let start = record.task.history.len() - limit;
                        record.task.history = record.task.history[start..].to_vec();
                    }
                }
                record.task
            })
            .collect::<Vec<_>>();

        let next_page_token = if offset + page_size < total_size {
            (offset + page_size).to_string()
        } else {
            String::new()
        };

        Ok(ListTasksResponse {
            tasks: page_tasks,
            next_page_token,
            page_size: page_size as i32,
            total_size: total_size as i32,
        })
    }

    pub fn set_push_config(
        &self,
        task_id: &str,
        config_id: &str,
        config: TaskPushNotificationConfig,
    ) -> Result<(), A2AStoreError> {
        let key = push_key(task_id, config_id);
        let payload = serde_json::to_vec(&config)?;
        self.db.put(key, payload)?;
        Ok(())
    }

    pub fn get_push_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<Option<TaskPushNotificationConfig>, A2AStoreError> {
        let key = push_key(task_id, config_id);
        let Some(raw) = self.db.get(key)? else {
            return Ok(None);
        };
        let config = serde_json::from_slice(&raw)?;
        Ok(Some(config))
    }

    pub fn list_push_configs(
        &self,
        task_id: &str,
        page_size: Option<i32>,
        page_token: Option<String>,
    ) -> Result<ListTaskPushNotificationConfigResponse, A2AStoreError> {
        let page_size = page_size
            .unwrap_or(DEFAULT_PAGE_SIZE as i32)
            .clamp(1, MAX_PAGE_SIZE as i32) as usize;

        let offset = match page_token.as_deref() {
            Some(token) if !token.is_empty() => token
                .parse::<usize>()
                .map_err(|_| A2AStoreError::InvalidPageToken)?,
            _ => 0,
        };

        let prefix = push_prefix(task_id);
        let mut configs: Vec<TaskPushNotificationConfig> = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item?;
            if !key.starts_with(prefix.as_bytes()) {
                continue;
            }
            let config: TaskPushNotificationConfig = serde_json::from_slice(&value)?;
            configs.push(config);
        }

        let total = configs.len();
        let configs = configs.into_iter().skip(offset).take(page_size).collect();

        let next_page_token = if offset + page_size < total {
            (offset + page_size).to_string()
        } else {
            String::new()
        };

        Ok(ListTaskPushNotificationConfigResponse {
            configs,
            next_page_token,
        })
    }

    pub fn delete_push_config(&self, task_id: &str, config_id: &str) -> Result<(), A2AStoreError> {
        let key = push_key(task_id, config_id);
        self.db.delete(key)?;
        Ok(())
    }
}

fn task_key(task_id: &str) -> Vec<u8> {
    format!("{}{}", TASK_PREFIX, task_id).into_bytes()
}

fn push_prefix(task_id: &str) -> String {
    format!("{}{}:", PUSH_PREFIX, task_id)
}

fn push_key(task_id: &str, config_id: &str) -> Vec<u8> {
    format!("{}{}:{}", PUSH_PREFIX, task_id, config_id).into_bytes()
}

pub fn now_iso_timestamp() -> String {
    Utc::now().to_rfc3339()
}

pub fn is_terminal(state: &TaskState) -> bool {
    matches!(
        state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    )
}
