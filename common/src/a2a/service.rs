use async_trait::async_trait;
use futures::Stream;

use super::{
    A2AResult, AgentCard, CancelTaskRequest, CreateTaskPushNotificationConfigRequest,
    GetExtendedAgentCardRequest, GetTaskRequest, ListTasksRequest, ListTasksResponse,
    SendMessageRequest, SendMessageResponse, StreamResponse, SubscribeToTaskRequest, Task,
    TaskPushNotificationConfig,
};

#[async_trait]
pub trait A2AService: Send + Sync {
    type MessageStream: Stream<Item = StreamResponse> + Send;
    type TaskStream: Stream<Item = StreamResponse> + Send;

    async fn send_message(&self, request: SendMessageRequest) -> A2AResult<SendMessageResponse>;

    async fn send_streaming_message(
        &self,
        request: SendMessageRequest,
    ) -> A2AResult<Self::MessageStream>;

    async fn get_task(&self, request: GetTaskRequest) -> A2AResult<Task>;

    async fn list_tasks(&self, request: ListTasksRequest) -> A2AResult<ListTasksResponse>;

    async fn cancel_task(&self, request: CancelTaskRequest) -> A2AResult<Task>;

    async fn subscribe_to_task(
        &self,
        request: SubscribeToTaskRequest,
    ) -> A2AResult<Self::TaskStream>;

    async fn create_task_push_notification_config(
        &self,
        request: CreateTaskPushNotificationConfigRequest,
    ) -> A2AResult<TaskPushNotificationConfig>;

    async fn get_task_push_notification_config(
        &self,
        request: super::GetTaskPushNotificationConfigRequest,
    ) -> A2AResult<TaskPushNotificationConfig>;

    async fn list_task_push_notification_config(
        &self,
        request: super::ListTaskPushNotificationConfigRequest,
    ) -> A2AResult<super::ListTaskPushNotificationConfigResponse>;

    async fn delete_task_push_notification_config(
        &self,
        request: super::DeleteTaskPushNotificationConfigRequest,
    ) -> A2AResult<()>;

    async fn get_extended_agent_card(
        &self,
        request: GetExtendedAgentCardRequest,
    ) -> A2AResult<AgentCard>;
}
