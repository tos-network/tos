pub const PROTOCOL_VERSION: &str = "1.0";

pub const HEADER_VERSION: &str = "a2a-version";
pub const HEADER_EXTENSIONS: &str = "a2a-extensions";
pub const HEADER_AUTHORIZATION: &str = "authorization";
pub const HEADER_TOS_SIGNATURE: &str = "a2a-tos-signature";

pub const EXTENSION_TOS_SETTLEMENT: &str = "urn:tos:a2a:extension:settlement:v1";

pub const METHOD_SEND_MESSAGE: &str = "SendMessage";
pub const METHOD_SEND_STREAMING_MESSAGE: &str = "SendStreamingMessage";
pub const METHOD_GET_TASK: &str = "GetTask";
pub const METHOD_LIST_TASKS: &str = "ListTasks";
pub const METHOD_CANCEL_TASK: &str = "CancelTask";
pub const METHOD_SUBSCRIBE_TO_TASK: &str = "SubscribeToTask";
pub const METHOD_CREATE_TASK_PUSH_CONFIG: &str = "CreateTaskPushNotificationConfig";
pub const METHOD_GET_TASK_PUSH_CONFIG: &str = "GetTaskPushNotificationConfig";
pub const METHOD_LIST_TASK_PUSH_CONFIG: &str = "ListTaskPushNotificationConfig";
pub const METHOD_DELETE_TASK_PUSH_CONFIG: &str = "DeleteTaskPushNotificationConfig";
pub const METHOD_GET_EXTENDED_AGENT_CARD: &str = "GetExtendedAgentCard";

pub const ENDPOINT_AGENT_CARD: &str = "/.well-known/agent-card.json";
pub const ENDPOINT_SEND_MESSAGE: &str = "/message:send";
pub const ENDPOINT_SEND_STREAMING_MESSAGE: &str = "/message:stream";
pub const ENDPOINT_TASKS: &str = "/tasks";
pub const ENDPOINT_TASK: &str = "/tasks/{id}";
pub const ENDPOINT_CANCEL_TASK: &str = "/tasks/{id}:cancel";
pub const ENDPOINT_SUBSCRIBE_TASK: &str = "/tasks/{id}:subscribe";
pub const ENDPOINT_TASK_PUSH_CONFIGS: &str = "/tasks/{id}/pushNotificationConfigs";
pub const ENDPOINT_TASK_PUSH_CONFIG: &str = "/tasks/{id}/pushNotificationConfigs/{configId}";
pub const ENDPOINT_EXTENDED_AGENT_CARD: &str = "/extendedAgentCard";

pub const ENDPOINT_SEND_MESSAGE_UNVERSIONED: &str = "/message:send";
pub const ENDPOINT_SEND_STREAMING_MESSAGE_UNVERSIONED: &str = "/message:stream";
pub const ENDPOINT_TASKS_UNVERSIONED: &str = "/tasks";
pub const ENDPOINT_TASK_UNVERSIONED: &str = "/tasks/{id}";
pub const ENDPOINT_CANCEL_TASK_UNVERSIONED: &str = "/tasks/{id}:cancel";
pub const ENDPOINT_SUBSCRIBE_TASK_UNVERSIONED: &str = "/tasks/{id}:subscribe";
pub const ENDPOINT_TASK_PUSH_CONFIGS_UNVERSIONED: &str = "/tasks/{id}/pushNotificationConfigs";
pub const ENDPOINT_TASK_PUSH_CONFIG_UNVERSIONED: &str =
    "/tasks/{id}/pushNotificationConfigs/{configId}";
pub const ENDPOINT_EXTENDED_AGENT_CARD_UNVERSIONED: &str = "/extendedAgentCard";
