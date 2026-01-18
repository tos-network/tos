pub mod auth;
pub mod executor;
pub mod grpc;
mod notify;
pub mod registry;
pub mod router_executor;
mod storage;

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use log::error;
use once_cell::sync::OnceCell;
use rand::RngCore;

use tos_common::a2a::Value;
use tos_common::{
    a2a::{
        A2AError, A2AResult, A2AService, AgentCapabilities, AgentCard, AgentInterface,
        AgentProvider, ApiKeySecurityScheme, Artifact, CancelTaskRequest,
        GetExtendedAgentCardRequest, GetTaskRequest, HttpAuthSecurityScheme, ListTasksRequest,
        ListTasksResponse, Message, OAuth2SecurityScheme, OAuthFlows, PartContent,
        PushNotificationConfig, Role, Security, SecurityScheme, SendMessageConfiguration,
        SendMessageRequest, SendMessageResponse, SetTaskPushNotificationConfigRequest,
        SettlementStatus, StreamResponse, SubscribeToTaskRequest, Task, TaskArtifactUpdateEvent,
        TaskPushNotificationConfig, TaskState, TaskStatus, TaskStatusUpdateEvent,
        TosSignatureSecurityScheme, TosTaskAnchor, MAX_ARTIFACTS_PER_TASK, MAX_DATA_PART_BYTES,
        MAX_FILE_INLINE_BYTES, MAX_HISTORY_LENGTH, MAX_METADATA_KEYS, MAX_PARTS_PER_MESSAGE,
        MAX_PUSH_CONFIGS_PER_TASK, MAX_TEXT_PART_BYTES,
    },
    config::VERSION,
    crypto::{Address, Hash},
};

use crate::core::blockchain::Blockchain;
use crate::core::storage::Storage;

use storage::{
    get_or_init, is_terminal, make_push_name, normalize_push_name, normalize_task_name,
    now_iso_timestamp, A2AStoreError,
};

pub fn set_base_dir(dir: &str) {
    storage::set_base_dir(dir);
    registry::set_base_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tos_common::crypto::{Address, AddressType, Hash, PublicKey};
    use tos_common::escrow::{EscrowAccount, EscrowState};
    use tos_common::serializer::Serializer;

    fn sample_pubkey(byte: u8) -> PublicKey {
        PublicKey::from_bytes(&[byte; 32]).expect("valid pubkey")
    }

    fn addr_str(key: PublicKey) -> String {
        Address::new(true, AddressType::Normal, key)
            .as_string()
            .expect("address string")
    }

    fn build_escrow(
        id: Hash,
        task_id: &str,
        payee: PublicKey,
        state: EscrowState,
        timeout_at: u64,
    ) -> EscrowAccount {
        EscrowAccount {
            id,
            task_id: task_id.to_string(),
            payer: sample_pubkey(9),
            payee,
            amount: 10,
            total_amount: 10,
            released_amount: 0,
            refunded_amount: 0,
            pending_release_amount: None,
            challenge_deposit: 0,
            asset: Hash::zero(),
            state,
            dispute_id: None,
            dispute_round: None,
            challenge_window: 0,
            challenge_deposit_bps: 0,
            optimistic_release: false,
            release_requested_at: None,
            created_at: 0,
            updated_at: 0,
            timeout_at,
            timeout_blocks: 0,
            arbitration_config: None,
            dispute: None,
            appeal: None,
            resolutions: Vec::new(),
        }
    }

    fn build_metadata(
        escrow_hash: &Hash,
        agent_account: &PublicKey,
    ) -> std::collections::HashMap<String, Value> {
        let agent_account = addr_str(agent_account.clone());
        let settlement = json!({
            "escrowHash": format!("0x{}", escrow_hash.to_hex()),
            "agentAccount": agent_account,
            "escrowId": 12345
        });
        let mut meta = std::collections::HashMap::new();
        meta.insert("tosSettlement".to_string(), settlement);
        meta
    }

    #[test]
    fn escrow_hash_validation_accepts_matching_anchor() {
        let task_id = "task-abc123";
        let escrow_hash = Hash::new([7u8; 32]);
        let payee = sample_pubkey(4);
        let escrow = build_escrow(
            escrow_hash.clone(),
            task_id,
            payee.clone(),
            EscrowState::Funded,
            100,
        );
        let metadata = build_metadata(&escrow_hash, &payee);

        let anchor =
            validate_settlement_anchor_with_escrow(task_id, Some(&metadata), 10, Some(escrow))
                .expect("valid anchor")
                .expect("anchor present");

        assert_eq!(anchor.escrow_id, 12345);
        assert_eq!(anchor.agent_account, payee);
    }

    #[test]
    fn escrow_hash_validation_rejects_task_mismatch() {
        let escrow_hash = Hash::new([8u8; 32]);
        let payee = sample_pubkey(5);
        let escrow = build_escrow(
            escrow_hash.clone(),
            "task-on-chain",
            payee.clone(),
            EscrowState::Funded,
            100,
        );
        let metadata = build_metadata(&escrow_hash, &payee);

        let err = validate_settlement_anchor_with_escrow(
            "task-request",
            Some(&metadata),
            10,
            Some(escrow),
        )
        .err()
        .expect("should fail");

        assert!(err.to_string().contains("task_id mismatch"));
    }

    #[test]
    fn escrow_hash_validation_rejects_terminal_or_timeout() {
        let escrow_hash = Hash::new([9u8; 32]);
        let payee = sample_pubkey(6);
        let escrow = build_escrow(
            escrow_hash.clone(),
            "task-1",
            payee.clone(),
            EscrowState::Released,
            100,
        );
        let metadata = build_metadata(&escrow_hash, &payee);

        let err =
            validate_settlement_anchor_with_escrow("task-1", Some(&metadata), 10, Some(escrow))
                .err()
                .expect("should fail");

        assert!(err.to_string().contains("disallowed state"));

        let escrow_timeout = build_escrow(
            escrow_hash.clone(),
            "task-1",
            payee.clone(),
            EscrowState::Funded,
            10,
        );
        let err = validate_settlement_anchor_with_escrow(
            "task-1",
            Some(&metadata),
            10,
            Some(escrow_timeout),
        )
        .err()
        .expect("should fail");

        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn escrow_validation_allows_only_configured_states() {
        let escrow_hash = Hash::new([10u8; 32]);
        let payee = sample_pubkey(7);
        let metadata = build_metadata(&escrow_hash, &payee);
        let escrow = build_escrow(
            escrow_hash.clone(),
            "task-2",
            payee.clone(),
            EscrowState::Challenged,
            100,
        );
        let config = SettlementValidationConfig {
            validate_states: true,
            allowed_states: vec!["funded".to_string()],
            validate_timeout: false,
            validate_amounts: false,
        };

        let err = validate_settlement_anchor_with_config(
            "task-2",
            Some(&metadata),
            10,
            Some(escrow),
            &config,
        )
        .err()
        .expect("should fail");

        assert!(err.to_string().contains("disallowed state"));
    }

    #[test]
    fn escrow_validation_enforces_max_cost_when_enabled() {
        let escrow_hash = Hash::new([11u8; 32]);
        let payee = sample_pubkey(8);
        let mut metadata = build_metadata(&escrow_hash, &payee);
        metadata.insert(
            "tosSettlement".to_string(),
            json!({
                "escrowHash": format!("0x{}", escrow_hash.to_hex()),
                "agentAccount": addr_str(payee.clone()),
                "maxCost": 5000
            }),
        );
        let escrow = build_escrow(
            escrow_hash.clone(),
            "task-3",
            payee.clone(),
            EscrowState::Funded,
            100,
        );
        let escrow = EscrowAccount {
            amount: 1000,
            ..escrow
        };
        let config = SettlementValidationConfig {
            validate_states: false,
            allowed_states: Vec::new(),
            validate_timeout: false,
            validate_amounts: true,
        };

        let err = validate_settlement_anchor_with_config(
            "task-3",
            Some(&metadata),
            10,
            Some(escrow),
            &config,
        )
        .err()
        .expect("should fail");

        assert!(err.to_string().contains("maxCost"));
    }
}
pub struct A2ADaemonService<S: Storage> {
    blockchain: Arc<Blockchain<S>>,
}

#[derive(Clone, Debug)]
pub struct SettlementValidationConfig {
    pub validate_states: bool,
    pub allowed_states: Vec<String>,
    pub validate_timeout: bool,
    pub validate_amounts: bool,
}

static SETTLEMENT_CONFIG: OnceCell<SettlementValidationConfig> = OnceCell::new();

pub fn set_settlement_validation_config(config: SettlementValidationConfig) {
    let _ = SETTLEMENT_CONFIG.set(config);
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
            arbitration: None,
        }
    }

    async fn validate_settlement_anchor(
        &self,
        task_id: &str,
        metadata: Option<&std::collections::HashMap<String, Value>>,
    ) -> A2AResult<Option<TosTaskAnchor>> {
        let escrow_hash = parse_escrow_hash(metadata)?;
        let escrow = if let Some(escrow_hash) = escrow_hash.as_ref() {
            let storage = self.blockchain.get_storage().read().await;
            Some(
                storage
                    .get_escrow(escrow_hash)
                    .await
                    .map_err(|e| A2AError::TosEscrowFailed {
                        reason: e.to_string(),
                    })?
                    .ok_or_else(|| A2AError::TosEscrowFailed {
                        reason: "escrow not found".to_string(),
                    })?,
            )
        } else {
            None
        };

        validate_settlement_anchor_with_escrow(
            task_id,
            metadata,
            self.blockchain.get_topo_height(),
            escrow,
        )
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

fn parse_settlement_anchor(
    metadata: Option<&std::collections::HashMap<String, Value>>,
) -> A2AResult<Option<TosTaskAnchor>> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let Some(settlement) = metadata.get("tosSettlement") else {
        return Ok(None);
    };
    let Some(obj) = settlement.as_object() else {
        return Err(A2AError::TosEscrowFailed {
            reason: "invalid tosSettlement metadata".to_string(),
        });
    };

    let escrow_id = match obj.get("escrowId") {
        Some(value) => match value {
            Value::Number(num) => num.as_u64(),
            Value::String(s) => s.parse::<u64>().ok(),
            _ => None,
        },
        None => Some(0),
    }
    .ok_or_else(|| A2AError::TosEscrowFailed {
        reason: "invalid escrowId".to_string(),
    })?;

    let agent_account = obj
        .get("agentAccount")
        .and_then(|v| v.as_str())
        .ok_or_else(|| A2AError::TosEscrowFailed {
            reason: "missing agentAccount".to_string(),
        })
        .and_then(|s| {
            Address::from_str(s)
                .map(|addr| addr.to_public_key())
                .map_err(|e| A2AError::TosEscrowFailed {
                    reason: format!("invalid agentAccount: {}", e),
                })
        })?;

    let settlement_status = obj
        .get("settlementStatus")
        .and_then(|v| v.as_str())
        .and_then(parse_settlement_status)
        .unwrap_or(SettlementStatus::EscrowLocked);

    Ok(Some(TosTaskAnchor {
        escrow_id,
        agent_account,
        settlement_status,
    }))
}

fn parse_settlement_status(value: &str) -> Option<SettlementStatus> {
    match value {
        "none" => Some(SettlementStatus::None),
        "escrow-locked" | "escrowLocked" => Some(SettlementStatus::EscrowLocked),
        "claimed" => Some(SettlementStatus::Claimed),
        "refunded" => Some(SettlementStatus::Refunded),
        "disputed" => Some(SettlementStatus::Disputed),
        _ => None,
    }
}

fn parse_escrow_hash(
    metadata: Option<&std::collections::HashMap<String, Value>>,
) -> A2AResult<Option<Hash>> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let Some(settlement) = metadata.get("tosSettlement") else {
        return Ok(None);
    };
    let Some(obj) = settlement.as_object() else {
        return Err(A2AError::TosEscrowFailed {
            reason: "invalid tosSettlement metadata".to_string(),
        });
    };
    let Some(value) = obj.get("escrowHash") else {
        return Ok(None);
    };
    let Some(hash_str) = value.as_str() else {
        return Err(A2AError::TosEscrowFailed {
            reason: "invalid escrowHash".to_string(),
        });
    };
    let hash_str = hash_str.strip_prefix("0x").unwrap_or(hash_str);
    Hash::from_str(hash_str)
        .map(Some)
        .map_err(|e| A2AError::TosEscrowFailed {
            reason: e.to_string(),
        })
}

fn validate_settlement_anchor_with_escrow(
    task_id: &str,
    metadata: Option<&std::collections::HashMap<String, Value>>,
    topoheight: u64,
    escrow: Option<tos_common::escrow::EscrowAccount>,
) -> A2AResult<Option<TosTaskAnchor>> {
    let config = settlement_validation_config_struct();
    validate_settlement_anchor_with_config(task_id, metadata, topoheight, escrow, &config)
}

fn settlement_validation_config_struct() -> SettlementValidationConfig {
    let default = SettlementValidationConfig {
        validate_states: true,
        allowed_states: vec![
            "created".to_string(),
            "funded".to_string(),
            "pending-release".to_string(),
            "challenged".to_string(),
        ],
        validate_timeout: true,
        validate_amounts: false,
    };
    let mut cfg = SETTLEMENT_CONFIG.get().cloned().unwrap_or(default);
    cfg.allowed_states = cfg
        .allowed_states
        .into_iter()
        .map(|state| state.to_lowercase())
        .collect();
    cfg
}

fn escrow_state_name(state: &tos_common::escrow::EscrowState) -> &str {
    match state {
        tos_common::escrow::EscrowState::Created => "created",
        tos_common::escrow::EscrowState::Funded => "funded",
        tos_common::escrow::EscrowState::PendingRelease => "pending-release",
        tos_common::escrow::EscrowState::Challenged => "challenged",
        tos_common::escrow::EscrowState::Released => "released",
        tos_common::escrow::EscrowState::Refunded => "refunded",
        tos_common::escrow::EscrowState::Resolved => "resolved",
        tos_common::escrow::EscrowState::Expired => "expired",
    }
}

fn parse_tos_settlement_max_cost(
    metadata: Option<&std::collections::HashMap<String, Value>>,
) -> Option<u64> {
    let settlement = metadata?.get("tosSettlement")?;
    let obj = settlement.as_object()?;
    let value = obj.get("maxCost")?;
    match value {
        Value::Number(num) => num.as_u64(),
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    }
}

fn validate_settlement_anchor_with_config(
    task_id: &str,
    metadata: Option<&std::collections::HashMap<String, Value>>,
    topoheight: u64,
    escrow: Option<tos_common::escrow::EscrowAccount>,
    config: &SettlementValidationConfig,
) -> A2AResult<Option<TosTaskAnchor>> {
    let anchor = parse_settlement_anchor(metadata)?;
    let escrow_hash = parse_escrow_hash(metadata)?;

    // If no anchor data, no validation needed
    let Some(anchor) = anchor else {
        return Ok(None);
    };

    // If anchor exists but escrowHash is missing, reject
    // This prevents bypassing validation by omitting escrowHash
    let Some(_escrow_hash) = escrow_hash else {
        return Err(A2AError::TosEscrowFailed {
            reason: "escrowHash is required when tosSettlement anchor data is present".to_string(),
        });
    };

    // anchor was already unwrapped above
    let Some(escrow) = escrow else {
        return Err(A2AError::TosEscrowFailed {
            reason: "escrow not found".to_string(),
        });
    };

    if escrow.task_id != task_id {
        return Err(A2AError::TosEscrowFailed {
            reason: "escrow task_id mismatch".to_string(),
        });
    }
    if escrow.payee != anchor.agent_account {
        return Err(A2AError::TosEscrowFailed {
            reason: "escrow payee mismatch".to_string(),
        });
    }
    if config.validate_states {
        let state_name = escrow_state_name(&escrow.state);
        if !config
            .allowed_states
            .iter()
            .any(|allowed| allowed == state_name)
        {
            return Err(A2AError::TosEscrowFailed {
                reason: "escrow is in disallowed state".to_string(),
            });
        }
    }
    if config.validate_timeout && topoheight >= escrow.timeout_at {
        return Err(A2AError::TosEscrowFailed {
            reason: "escrow timeout reached".to_string(),
        });
    }
    if config.validate_amounts {
        if let Some(max_cost) = parse_tos_settlement_max_cost(metadata) {
            if escrow.amount < max_cost {
                return Err(A2AError::TosEscrowFailed {
                    reason: "escrow amount below maxCost".to_string(),
                });
            }
        }
    }

    Ok(Some(anchor))
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
                        // Calculate decoded size from base64: approximately (len * 3) / 4
                        // Account for padding by being conservative
                        file_with_bytes.len().saturating_mul(3) / 4
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

fn validate_arbitration_evidence(message: &Message) -> A2AResult<()> {
    let Some(metadata) = message.metadata.as_ref() else {
        return Ok(());
    };
    let keys = ["dispute_id", "escrow_id", "party_role"];
    let has_any = keys.iter().any(|key| metadata.contains_key(*key));
    if !has_any {
        return Ok(());
    }
    for key in keys {
        match metadata.get(key) {
            Some(Value::String(value)) if !value.trim().is_empty() => {}
            Some(_) => {
                return Err(A2AError::InvalidParams {
                    message: format!("metadata.{key} must be a non-empty string"),
                })
            }
            None => {
                return Err(A2AError::InvalidParams {
                    message: format!("metadata.{key} is required for evidence submission"),
                })
            }
        }
    }
    let has_file = message
        .parts
        .iter()
        .any(|part| matches!(part.content, PartContent::File { .. }));
    if !has_file {
        return Err(A2AError::InvalidParams {
            message: "evidence submission requires a file part".to_string(),
        });
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
        validate_arbitration_evidence(&message)?;

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
            let tos_task_anchor = self
                .validate_settlement_anchor(&task_id, request.metadata.as_ref())
                .await?;
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
                tos_task_anchor,
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
        validate_arbitration_evidence(&message)?;

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
            let tos_task_anchor = self
                .validate_settlement_anchor(&task_id, request.metadata.as_ref())
                .await?;
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
                tos_task_anchor,
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
