use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    account::{AgentAccountMeta, SessionKey},
    crypto::{Hash, PublicKey, Signature},
};

use super::errors::{A2AError, A2AResult};
use super::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_task_anchor: Option<TosTaskAnchor>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::Rejected
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub reference_task_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    #[serde(rename = "ROLE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(flatten)]
    pub content: PartContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PartContent {
    Text { text: String },
    Bytes { raw: String },
    Url { url: String },
    Data { data: Value },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
    #[serde(default)]
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub version: String,
    pub supported_interfaces: Vec<AgentInterface>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    pub capabilities: AgentCapabilities,
    #[serde(default)]
    pub security_schemes: HashMap<String, SecurityScheme>,
    #[serde(default)]
    pub security_requirements: Vec<SecurityRequirement>,
    #[serde(default)]
    pub default_input_modes: Vec<String>,
    #[serde(default)]
    pub default_output_modes: Vec<String>,
    #[serde(default)]
    pub skills: Vec<AgentSkill>,
    #[serde(default)]
    pub signatures: Vec<AgentCardSignature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_identity: Option<TosAgentIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arbitration: Option<ArbitrationExtension>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
    pub protocol_version: String,
    pub url: String,
    pub protocol_binding: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    pub url: String,
    pub organization: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notifications: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_agent_card: Option<bool>,
    #[serde(default)]
    pub extensions: Vec<AgentExtension>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_on_chain_settlement: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExtension {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub input_modes: Vec<String>,
    #[serde(default)]
    pub output_modes: Vec<String>,
    #[serde(default)]
    pub security_requirements: Vec<SecurityRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_base_cost: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArbitrationExtension {
    pub expertise_domains: Vec<String>,
    pub fee_basis_points: u16,
    pub min_escrow_value: u64,
    pub max_escrow_value: u64,
    pub committee_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_resolution_hours: Option<u32>,
    pub languages: Vec<String>,
    pub contact_preferences: ContactPreferences,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactPreferences {
    pub preferred_method: String,
    pub response_time_hours: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCardSignature {
    pub protected: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SecurityScheme {
    ApiKey {
        #[serde(rename = "apiKeySecurityScheme")]
        api_key_security_scheme: ApiKeySecurityScheme,
    },
    HttpAuth {
        #[serde(rename = "httpAuthSecurityScheme")]
        http_auth_security_scheme: HttpAuthSecurityScheme,
    },
    OAuth2 {
        #[serde(rename = "oauth2SecurityScheme")]
        oauth2_security_scheme: OAuth2SecurityScheme,
    },
    OpenIdConnect {
        #[serde(rename = "openIdConnectSecurityScheme")]
        open_id_connect_security_scheme: OpenIdConnectSecurityScheme,
    },
    MutualTls {
        #[serde(rename = "mutualTlsSecurityScheme")]
        mutual_tls_security_scheme: MutualTlsSecurityScheme,
    },
    TosSignature {
        #[serde(rename = "tosSignatureSecurityScheme")]
        tos_signature_security_scheme: TosSignatureSecurityScheme,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StringList {
    #[serde(default)]
    pub list: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRequirement {
    #[serde(default)]
    pub schemes: HashMap<String, StringList>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub location: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub scheme: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_format: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2SecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub flows: OAuthFlows,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth2_metadata_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OAuthFlows {
    AuthorizationCode {
        #[serde(rename = "authorizationCode")]
        authorization_code: AuthorizationCodeFlow,
    },
    ClientCredentials {
        #[serde(rename = "clientCredentials")]
        client_credentials: ClientCredentialsFlow,
    },
    Implicit {
        #[serde(rename = "implicit")]
        implicit: ImplicitFlow,
    },
    Password {
        #[serde(rename = "password")]
        password: PasswordFlow,
    },
    DeviceCode {
        #[serde(rename = "deviceCode")]
        device_code: DeviceCodeOAuthFlow,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationCodeFlow {
    pub authorization_url: String,
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(default)]
    pub scopes: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkce_required: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentialsFlow {
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(default)]
    pub scopes: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplicitFlow {
    pub authorization_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(default)]
    pub scopes: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordFlow {
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    #[serde(default)]
    pub scopes: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeOAuthFlow {
    pub device_authorization_url: String,
    pub token_url: String,
    #[serde(default)]
    pub scopes: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenIdConnectSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub open_id_connect_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutualTlsSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosSignatureSecurityScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub chain_id: u64,
    #[serde(default)]
    pub allowed_signers: Vec<TosSignerType>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TosSignerType {
    Owner,
    Controller,
    SessionKey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosSignature {
    pub signer: TosSignerType,
    pub value: String,
    pub timestamp: u64,
    pub nonce: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key_id: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<SendMessageConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
    #[serde(default)]
    pub accepted_output_modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notification_config: Option<PushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<i32>,
    pub blocking: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SendMessageResponse {
    Task { task: Box<Task> },
    Message { message: Message },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_timestamp_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_artifacts: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResponse {
    #[serde(default)]
    pub tasks: Vec<Task>,
    pub next_page_token: String,
    pub page_size: i32,
    pub total_size: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelTaskRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeToTaskRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExtendedAgentCardRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamResponse {
    Task {
        task: Task,
    },
    Message {
        message: Message,
    },
    StatusUpdate {
        #[serde(rename = "statusUpdate")]
        status_update: TaskStatusUpdateEvent,
    },
    ArtifactUpdate {
        #[serde(rename = "artifactUpdate")]
        artifact_update: TaskArtifactUpdateEvent,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusUpdateEvent {
    pub task_id: String,
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactUpdateEvent {
    pub task_id: String,
    pub context_id: String,
    pub artifact: Artifact,
    pub append: bool,
    pub last_chunk: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotificationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthenticationInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationInfo {
    pub scheme: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
    pub id: String,
    pub task_id: String,
    pub push_notification_config: PushNotificationConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskPushNotificationConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub task_id: String,
    pub config: TaskPushNotificationConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskPushNotificationConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub task_id: String,
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskPushNotificationConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskPushNotificationConfigResponse {
    #[serde(default)]
    pub configs: Vec<TaskPushNotificationConfig>,
    pub next_page_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskPushNotificationConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub task_id: String,
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosSettlementExtension {
    pub uri: String,
    pub required: bool,
    pub params: TosSettlementParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosSettlementParams {
    #[serde(default)]
    pub supported_currencies: Vec<String>,
    pub min_payment: u64,
    pub max_payment: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escrow_contract: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosTaskAnchor {
    pub escrow_id: u64,
    pub agent_account: PublicKey,
    pub settlement_status: SettlementStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SettlementStatus {
    None,
    EscrowLocked,
    Claimed,
    Refunded,
    Disputed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosAgentIdentity {
    pub agent_account: PublicKey,
    pub controller: PublicKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reputation_score_bps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_proof: Option<TosIdentityProof>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TosIdentityProof {
    pub proof_type: String,
    pub signature: String,
    pub created_at_block: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_block: Option<u64>,
}

pub trait TosSettlementBridge {
    fn create_task_escrow(
        &mut self,
        client_account: &PublicKey,
        agent_account: &PublicKey,
        task_id: &str,
        amount: u64,
        deadline_block: u64,
    ) -> A2AResult<u64>;

    fn claim_task_reward(&mut self, escrow_id: u64, result_hash: Hash) -> A2AResult<u64>;

    fn reclaim_timeout(&mut self, escrow_id: u64) -> A2AResult<u64>;
}

pub trait AgentAccountReader {
    type Error: std::fmt::Display;

    fn get_agent_account_meta(
        &self,
        agent_account: &PublicKey,
    ) -> Result<Option<AgentAccountMeta>, Self::Error>;

    fn get_session_key(
        &self,
        agent_account: &PublicKey,
        key_id: u64,
    ) -> Result<Option<SessionKey>, Self::Error>;

    fn get_topoheight(&self) -> Result<u64, Self::Error>;
}

pub fn verify_tos_signature<R: AgentAccountReader>(
    signature: &TosSignature,
    agent_account: &PublicKey,
    message: &[u8],
    chain_state: &R,
) -> A2AResult<()> {
    let meta = chain_state
        .get_agent_account_meta(agent_account)
        .map_err(|e| A2AError::InternalError {
            message: e.to_string(),
        })?
        .ok_or_else(|| A2AError::TosIdentityVerificationFailed {
            agent_account: hex::encode(agent_account.as_bytes()),
        })?;

    if meta.status == 1 {
        return Err(A2AError::TosAccountFrozen);
    }

    let signing_key = match signature.signer {
        TosSignerType::Owner => meta.owner,
        TosSignerType::Controller => meta.controller,
        TosSignerType::SessionKey => {
            let key_id = signature
                .session_key_id
                .ok_or(A2AError::TosSignatureInvalid)?;
            let session_key = chain_state
                .get_session_key(agent_account, key_id)
                .map_err(|e| A2AError::InternalError {
                    message: e.to_string(),
                })?
                .ok_or(A2AError::TosSignatureInvalid)?;

            let current_height =
                chain_state
                    .get_topoheight()
                    .map_err(|e| A2AError::InternalError {
                        message: e.to_string(),
                    })?;
            if current_height >= session_key.expiry_topoheight {
                return Err(A2AError::TosSessionKeyExpired);
            }

            session_key.public_key
        }
    };

    let sig_hex = signature
        .value
        .strip_prefix("0x")
        .unwrap_or(&signature.value);
    let sig = Signature::from_hex(sig_hex).map_err(|_| A2AError::TosSignatureInvalid)?;

    let signing_key = signing_key
        .decompress()
        .map_err(|_| A2AError::TosSignatureInvalid)?;

    if !sig.verify(message, &signing_key) {
        return Err(A2AError::TosSignatureInvalid);
    }

    Ok(())
}
