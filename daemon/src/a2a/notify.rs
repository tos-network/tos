use std::time::Duration;

use reqwest::Client;
use tokio::time::sleep;

use tos_common::a2a::{
    ListTaskPushNotificationConfigResponse, StreamResponse, TaskPushNotificationConfig,
};

use super::storage::A2AStore;

const RETRY_DELAYS_MS: [u64; 3] = [200, 1000, 3000];
const REQUEST_TIMEOUT_SECS: u64 = 5;

pub async fn notify_task_event(store: &A2AStore, task_id: &str, event: StreamResponse) {
    let configs = list_all_configs(store, task_id).await;
    if configs.is_empty() {
        return;
    }

    let client = match Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    for config in configs {
        let client = client.clone();
        let event = event.clone();
        tokio::spawn(async move {
            send_with_retry(&client, &config, &event).await;
        });
    }
}

async fn list_all_configs(store: &A2AStore, task_id: &str) -> Vec<TaskPushNotificationConfig> {
    let mut page_token: Option<String> = None;
    let mut configs = Vec::new();
    loop {
        let response = store
            .list_push_configs(task_id, None, page_token.clone())
            .unwrap_or_else(|_| ListTaskPushNotificationConfigResponse {
                configs: Vec::new(),
                next_page_token: String::new(),
            });
        configs.extend(response.configs);
        if response.next_page_token.is_empty() {
            break;
        }
        page_token = Some(response.next_page_token);
    }
    configs
}

async fn send_with_retry(
    client: &Client,
    config: &TaskPushNotificationConfig,
    event: &StreamResponse,
) {
    for (idx, delay) in RETRY_DELAYS_MS.iter().enumerate() {
        let mut request = client.post(&config.push_notification_config.url);
        if let Some(token) = config.push_notification_config.token.as_ref() {
            request = request.bearer_auth(token);
        }
        let response = request.json(event).send().await;
        if response
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return;
        }
        if idx + 1 < RETRY_DELAYS_MS.len() {
            sleep(Duration::from_millis(*delay)).await;
        }
    }
}
