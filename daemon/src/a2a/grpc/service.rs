use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use prost_types::{value, Struct, Value};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tos_common::rpc::server::RequestMetadata;

use crate::a2a::grpc::proto;
use crate::a2a::grpc::proto::a2a_service_server::A2aService;
use crate::a2a::A2ADaemonService;
use crate::core::blockchain::Blockchain;
use crate::core::storage::Storage;
use tos_common::a2a as common;
use tos_common::a2a::A2AService;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_crypto::curve25519_dalek::ristretto::CompressedRistretto;

/// Extract authentication metadata from gRPC request and verify
async fn verify_grpc_auth<T>(request: &Request<T>) -> Result<(), Status> {
    let metadata = request.metadata();

    // Convert gRPC metadata to HashMap for auth verification
    let mut headers = HashMap::new();

    // Extract standard auth headers from gRPC metadata
    if let Some(auth) = metadata.get("authorization") {
        if let Ok(v) = auth.to_str() {
            headers.insert("authorization".to_string(), v.to_string());
        }
    }
    if let Some(api_key) = metadata.get("x-api-key") {
        if let Ok(v) = api_key.to_str() {
            headers.insert("x-api-key".to_string(), v.to_string());
        }
    }
    // TOS signature headers
    for key in [
        "tos-timestamp",
        "tos-nonce",
        "tos-public-key",
        "tos-signature",
    ] {
        if let Some(value) = metadata.get(key) {
            if let Ok(v) = value.to_str() {
                headers.insert(key.to_string(), v.to_string());
            }
        }
    }

    let meta = RequestMetadata {
        headers,
        body: Vec::new(), // gRPC body is not used for auth
        method: "POST".to_string(),
        path: "/grpc".to_string(),
        query: String::new(),
    };

    crate::a2a::auth::authorize_metadata(&meta)
        .await
        .map_err(|_| Status::unauthenticated("invalid or missing authentication"))?;

    Ok(())
}

pub struct A2AGrpcService<S: Storage> {
    service: A2ADaemonService<S>,
}

impl<S: Storage> A2AGrpcService<S> {
    pub fn new(blockchain: Arc<Blockchain<S>>) -> Self {
        Self {
            service: A2ADaemonService::new(blockchain),
        }
    }
}

#[async_trait]
impl<S: Storage + Send + Sync + 'static> A2aService for A2AGrpcService<S> {
    type SendStreamingMessageStream = ReceiverStream<Result<proto::StreamResponse, Status>>;
    type SubscribeToTaskStream = ReceiverStream<Result<proto::StreamResponse, Status>>;

    async fn send_message(
        &self,
        request: Request<proto::SendMessageRequest>,
    ) -> Result<Response<proto::SendMessageResponse>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_send_message_request_to_common(request.into_inner())?;
        let response = self
            .service
            .send_message(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_send_message_response_to_proto(
            response,
        )))
    }

    async fn send_streaming_message(
        &self,
        request: Request<proto::SendMessageRequest>,
    ) -> Result<Response<Self::SendStreamingMessageStream>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_send_message_request_to_common(request.into_inner())?;
        let mut stream = self
            .service
            .send_streaming_message(request)
            .await
            .map_err(map_a2a_error)?;
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                let _ = tx.send(Ok(common_stream_response_to_proto(item))).await;
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_task(
        &self,
        request: Request<proto::GetTaskRequest>,
    ) -> Result<Response<proto::Task>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_get_task_request_to_common(request.into_inner());
        let response = self
            .service
            .get_task(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_task_to_proto(response)))
    }

    async fn list_tasks(
        &self,
        request: Request<proto::ListTasksRequest>,
    ) -> Result<Response<proto::ListTasksResponse>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_list_tasks_request_to_common(request.into_inner());
        let response = self
            .service
            .list_tasks(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_list_tasks_response_to_proto(response)))
    }

    async fn cancel_task(
        &self,
        request: Request<proto::CancelTaskRequest>,
    ) -> Result<Response<proto::Task>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_cancel_task_request_to_common(request.into_inner());
        let response = self
            .service
            .cancel_task(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_task_to_proto(response)))
    }

    async fn subscribe_to_task(
        &self,
        request: Request<proto::SubscribeToTaskRequest>,
    ) -> Result<Response<Self::SubscribeToTaskStream>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_subscribe_request_to_common(request.into_inner());
        let mut stream = self
            .service
            .subscribe_to_task(request)
            .await
            .map_err(map_a2a_error)?;
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                let _ = tx.send(Ok(common_stream_response_to_proto(item))).await;
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn create_task_push_notification_config(
        &self,
        request: Request<proto::CreateTaskPushNotificationConfigRequest>,
    ) -> Result<Response<proto::TaskPushNotificationConfig>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_create_push_request_to_common(request.into_inner())?;
        let response = self
            .service
            .create_task_push_notification_config(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_push_config_to_proto(response)))
    }

    async fn get_task_push_notification_config(
        &self,
        request: Request<proto::GetTaskPushNotificationConfigRequest>,
    ) -> Result<Response<proto::TaskPushNotificationConfig>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_get_push_request_to_common(request.into_inner());
        let response = self
            .service
            .get_task_push_notification_config(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_push_config_to_proto(response)))
    }

    async fn list_task_push_notification_config(
        &self,
        request: Request<proto::ListTaskPushNotificationConfigRequest>,
    ) -> Result<Response<proto::ListTaskPushNotificationConfigResponse>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_list_push_request_to_common(request.into_inner());
        let response = self
            .service
            .list_task_push_notification_config(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_list_push_response_to_proto(response)))
    }

    async fn delete_task_push_notification_config(
        &self,
        request: Request<proto::DeleteTaskPushNotificationConfigRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_delete_push_request_to_common(request.into_inner());
        self.service
            .delete_task_push_notification_config(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(proto::Empty {}))
    }

    async fn get_extended_agent_card(
        &self,
        request: Request<proto::GetExtendedAgentCardRequest>,
    ) -> Result<Response<proto::AgentCard>, Status> {
        verify_grpc_auth(&request).await?;
        let request = proto_get_extended_card_request_to_common(request.into_inner());
        let response = self
            .service
            .get_extended_agent_card(request)
            .await
            .map_err(map_a2a_error)?;
        Ok(Response::new(common_agent_card_to_proto(response)))
    }
}

fn map_a2a_error(err: common::A2AError) -> Status {
    match err {
        common::A2AError::TaskNotFoundError { .. } => Status::not_found(err.to_string()),
        common::A2AError::InvalidParams { .. } => Status::invalid_argument(err.to_string()),
        common::A2AError::UnsupportedOperationError { .. } => {
            Status::failed_precondition(err.to_string())
        }
        _ => Status::internal(err.to_string()),
    }
}

fn proto_send_message_request_to_common(
    request: proto::SendMessageRequest,
) -> Result<common::SendMessageRequest, Status> {
    Ok(common::SendMessageRequest {
        tenant: empty_to_none(request.tenant),
        message: proto_message_to_common(
            request
                .message
                .ok_or_else(|| Status::invalid_argument("message is required"))?,
        )?,
        configuration: request
            .configuration
            .map(proto_send_message_config_to_common),
        metadata: proto_map_to_json(&request.metadata),
    })
}

fn proto_send_message_config_to_common(
    config: proto::SendMessageConfiguration,
) -> common::SendMessageConfiguration {
    common::SendMessageConfiguration {
        accepted_output_modes: config.accepted_output_modes,
        push_notification_config: config
            .push_notification_config
            .map(proto_push_notification_to_common),
        history_length: if config.history_length == 0 {
            None
        } else {
            Some(config.history_length)
        },
        blocking: config.blocking,
    }
}

fn common_send_message_response_to_proto(
    response: common::SendMessageResponse,
) -> proto::SendMessageResponse {
    let response = match response {
        common::SendMessageResponse::Task { task } => {
            proto::send_message_response::Response::Task(common_task_to_proto(*task))
        }
        common::SendMessageResponse::Message { message } => {
            proto::send_message_response::Response::Message(common_message_to_proto(message))
        }
    };
    proto::SendMessageResponse {
        response: Some(response),
    }
}

fn proto_get_task_request_to_common(request: proto::GetTaskRequest) -> common::GetTaskRequest {
    common::GetTaskRequest {
        tenant: empty_to_none(request.tenant),
        id: request.id,
        history_length: if request.history_length == 0 {
            None
        } else {
            Some(request.history_length)
        },
    }
}

fn proto_list_tasks_request_to_common(
    request: proto::ListTasksRequest,
) -> common::ListTasksRequest {
    common::ListTasksRequest {
        tenant: empty_to_none(request.tenant),
        context_id: empty_to_none(request.context_id),
        status: proto_task_state_to_common(request.status),
        page_size: if request.page_size == 0 {
            None
        } else {
            Some(request.page_size)
        },
        page_token: empty_to_none(request.page_token),
        history_length: if request.history_length == 0 {
            None
        } else {
            Some(request.history_length)
        },
        status_timestamp_after: empty_to_none(request.status_timestamp_after),
        include_artifacts: Some(request.include_artifacts),
    }
}

fn common_list_tasks_response_to_proto(
    response: common::ListTasksResponse,
) -> proto::ListTasksResponse {
    proto::ListTasksResponse {
        tasks: response
            .tasks
            .into_iter()
            .map(common_task_to_proto)
            .collect(),
        next_page_token: response.next_page_token,
        page_size: response.page_size,
        total_size: response.total_size,
    }
}

fn proto_cancel_task_request_to_common(
    request: proto::CancelTaskRequest,
) -> common::CancelTaskRequest {
    common::CancelTaskRequest {
        tenant: empty_to_none(request.tenant),
        id: request.id,
    }
}

fn proto_subscribe_request_to_common(
    request: proto::SubscribeToTaskRequest,
) -> common::SubscribeToTaskRequest {
    common::SubscribeToTaskRequest {
        tenant: empty_to_none(request.tenant),
        id: request.id,
    }
}

fn proto_create_push_request_to_common(
    request: proto::CreateTaskPushNotificationConfigRequest,
) -> Result<common::CreateTaskPushNotificationConfigRequest, Status> {
    Ok(common::CreateTaskPushNotificationConfigRequest {
        tenant: empty_to_none(request.tenant),
        task_id: request.task_id,
        config: proto_push_config_to_common(
            request
                .config
                .ok_or_else(|| Status::invalid_argument("config is required"))?,
        )?,
    })
}

fn proto_get_push_request_to_common(
    request: proto::GetTaskPushNotificationConfigRequest,
) -> common::GetTaskPushNotificationConfigRequest {
    common::GetTaskPushNotificationConfigRequest {
        tenant: empty_to_none(request.tenant),
        task_id: request.task_id,
        id: request.id,
    }
}

fn proto_list_push_request_to_common(
    request: proto::ListTaskPushNotificationConfigRequest,
) -> common::ListTaskPushNotificationConfigRequest {
    common::ListTaskPushNotificationConfigRequest {
        tenant: empty_to_none(request.tenant),
        task_id: request.task_id,
        page_size: if request.page_size == 0 {
            None
        } else {
            Some(request.page_size)
        },
        page_token: empty_to_none(request.page_token),
    }
}

fn proto_delete_push_request_to_common(
    request: proto::DeleteTaskPushNotificationConfigRequest,
) -> common::DeleteTaskPushNotificationConfigRequest {
    common::DeleteTaskPushNotificationConfigRequest {
        tenant: empty_to_none(request.tenant),
        task_id: request.task_id,
        id: request.id,
    }
}

fn common_push_config_to_proto(
    config: common::TaskPushNotificationConfig,
) -> proto::TaskPushNotificationConfig {
    proto::TaskPushNotificationConfig {
        id: config.id,
        task_id: config.task_id,
        push_notification_config: Some(proto::PushNotificationConfig {
            id: config.push_notification_config.id.unwrap_or_default(),
            url: config.push_notification_config.url,
            token: config.push_notification_config.token.unwrap_or_default(),
            authentication: config.push_notification_config.authentication.map(|auth| {
                proto::AuthenticationInfo {
                    scheme: auth.scheme,
                    credentials: auth.credentials.unwrap_or_default(),
                }
            }),
        }),
    }
}

fn proto_push_notification_to_common(
    config: proto::PushNotificationConfig,
) -> common::PushNotificationConfig {
    common::PushNotificationConfig {
        id: empty_to_none(config.id),
        url: config.url,
        token: empty_to_none(config.token),
        authentication: config
            .authentication
            .map(|auth| common::AuthenticationInfo {
                scheme: auth.scheme,
                credentials: empty_to_none(auth.credentials),
            }),
    }
}

fn proto_push_config_to_common(
    config: proto::TaskPushNotificationConfig,
) -> Result<common::TaskPushNotificationConfig, Status> {
    let inner = config
        .push_notification_config
        .ok_or_else(|| Status::invalid_argument("push_notification_config is required"))?;
    Ok(common::TaskPushNotificationConfig {
        id: config.id,
        task_id: config.task_id,
        push_notification_config: proto_push_notification_to_common(inner),
    })
}

fn common_list_push_response_to_proto(
    response: common::ListTaskPushNotificationConfigResponse,
) -> proto::ListTaskPushNotificationConfigResponse {
    let configs: Vec<_> = response
        .configs
        .into_iter()
        .map(common_push_config_to_proto)
        .collect();
    proto::ListTaskPushNotificationConfigResponse {
        configs,
        next_page_token: response.next_page_token,
    }
}

fn proto_get_extended_card_request_to_common(
    request: proto::GetExtendedAgentCardRequest,
) -> common::GetExtendedAgentCardRequest {
    common::GetExtendedAgentCardRequest {
        tenant: empty_to_none(request.tenant),
    }
}

fn common_agent_card_to_proto(card: common::AgentCard) -> proto::AgentCard {
    proto::AgentCard {
        name: card.name,
        description: card.description,
        version: card.version,
        supported_interfaces: card
            .supported_interfaces
            .into_iter()
            .map(|iface| proto::AgentInterface {
                protocol_version: iface.protocol_version,
                url: iface.url,
                protocol_binding: iface.protocol_binding,
                tenant: iface.tenant.unwrap_or_default(),
            })
            .collect(),
        provider: card.provider.map(|provider| proto::AgentProvider {
            url: provider.url,
            organization: provider.organization,
        }),
        icon_url: card.icon_url.unwrap_or_default(),
        documentation_url: card.documentation_url.unwrap_or_default(),
        capabilities: Some(proto::AgentCapabilities {
            streaming: card.capabilities.streaming.unwrap_or(false),
            push_notifications: card.capabilities.push_notifications.unwrap_or(false),
            extended_agent_card: card.capabilities.extended_agent_card.unwrap_or(false),
            extensions: card
                .capabilities
                .extensions
                .into_iter()
                .map(|ext| proto::AgentExtension {
                    uri: ext.uri,
                    description: ext.description.unwrap_or_default(),
                    required: ext.required,
                    params: ext
                        .params
                        .map(|params| json_map_to_proto(&params))
                        .unwrap_or_default(),
                })
                .collect(),
            tos_on_chain_settlement: card.capabilities.tos_on_chain_settlement.unwrap_or(false),
        }),
        security_schemes: card
            .security_schemes
            .into_iter()
            .map(|(key, scheme)| (key, common_security_scheme_to_proto(scheme)))
            .collect(),
        security_requirements: card
            .security_requirements
            .into_iter()
            .map(common_security_requirement_to_proto)
            .collect(),
        default_input_modes: card.default_input_modes,
        default_output_modes: card.default_output_modes,
        skills: card.skills.into_iter().map(common_skill_to_proto).collect(),
        signatures: card
            .signatures
            .into_iter()
            .map(common_signature_to_proto)
            .collect(),
        tos_identity: card.tos_identity.map(common_identity_to_proto),
        arbitration: card.arbitration.map(common_arbitration_extension_to_proto),
    }
}

fn common_arbitration_extension_to_proto(
    extension: common::ArbitrationExtension,
) -> proto::ArbitrationExtension {
    proto::ArbitrationExtension {
        expertise_domains: extension.expertise_domains,
        fee_basis_points: extension.fee_basis_points.into(),
        min_escrow_value: extension.min_escrow_value,
        max_escrow_value: extension.max_escrow_value,
        committee_ids: extension.committee_ids,
        avg_resolution_hours: extension.avg_resolution_hours.unwrap_or_default(),
        languages: extension.languages,
        contact_preferences: Some(common_contact_preferences_to_proto(
            extension.contact_preferences,
        )),
    }
}

fn common_contact_preferences_to_proto(
    preferences: common::ContactPreferences,
) -> proto::ContactPreferences {
    proto::ContactPreferences {
        preferred_method: preferences.preferred_method,
        response_time_hours: preferences.response_time_hours,
        availability: preferences.availability.unwrap_or_default(),
    }
}

fn common_security_requirement_to_proto(
    security: common::SecurityRequirement,
) -> proto::SecurityRequirement {
    proto::SecurityRequirement {
        schemes: security
            .schemes
            .into_iter()
            .map(|(key, value)| (key, proto::StringList { list: value.list }))
            .collect(),
    }
}

fn common_security_scheme_to_proto(scheme: common::SecurityScheme) -> proto::SecurityScheme {
    let scheme = match scheme {
        common::SecurityScheme::ApiKey {
            api_key_security_scheme,
        } => proto::security_scheme::Scheme::ApiKeySecurityScheme(proto::ApiKeySecurityScheme {
            description: api_key_security_scheme.description.unwrap_or_default(),
            location: api_key_security_scheme.location,
            name: api_key_security_scheme.name,
        }),
        common::SecurityScheme::HttpAuth {
            http_auth_security_scheme,
        } => {
            proto::security_scheme::Scheme::HttpAuthSecurityScheme(proto::HttpAuthSecurityScheme {
                description: http_auth_security_scheme.description.unwrap_or_default(),
                scheme: http_auth_security_scheme.scheme,
                bearer_format: http_auth_security_scheme.bearer_format.unwrap_or_default(),
            })
        }
        common::SecurityScheme::OAuth2 {
            oauth2_security_scheme,
        } => proto::security_scheme::Scheme::Oauth2SecurityScheme(proto::OAuth2SecurityScheme {
            description: oauth2_security_scheme.description.unwrap_or_default(),
            flows: Some(common_oauth_flows_to_proto(oauth2_security_scheme.flows)),
            oauth2_metadata_url: oauth2_security_scheme
                .oauth2_metadata_url
                .unwrap_or_default(),
        }),
        common::SecurityScheme::OpenIdConnect {
            open_id_connect_security_scheme,
        } => proto::security_scheme::Scheme::OpenIdConnectSecurityScheme(
            proto::OpenIdConnectSecurityScheme {
                description: open_id_connect_security_scheme
                    .description
                    .unwrap_or_default(),
                open_id_connect_url: open_id_connect_security_scheme.open_id_connect_url,
            },
        ),
        common::SecurityScheme::MutualTls {
            mutual_tls_security_scheme,
        } => proto::security_scheme::Scheme::MutualTlsSecurityScheme(
            proto::MutualTlsSecurityScheme {
                description: mutual_tls_security_scheme.description.unwrap_or_default(),
            },
        ),
        common::SecurityScheme::TosSignature {
            tos_signature_security_scheme,
        } => proto::security_scheme::Scheme::TosSignatureSecurityScheme(
            proto::TosSignatureSecurityScheme {
                description: tos_signature_security_scheme
                    .description
                    .unwrap_or_default(),
                chain_id: tos_signature_security_scheme.chain_id,
                allowed_signers: tos_signature_security_scheme
                    .allowed_signers
                    .into_iter()
                    .map(|signer| format!("{signer:?}").to_lowercase())
                    .collect(),
            },
        ),
    };
    proto::SecurityScheme {
        scheme: Some(scheme),
    }
}

fn common_oauth_flows_to_proto(flows: common::OAuthFlows) -> proto::OAuthFlows {
    let flow = match flows {
        common::OAuthFlows::AuthorizationCode { authorization_code } => {
            proto::o_auth_flows::Flow::AuthorizationCode(proto::AuthorizationCodeFlow {
                authorization_url: authorization_code.authorization_url,
                token_url: authorization_code.token_url,
                refresh_url: authorization_code.refresh_url.unwrap_or_default(),
                scopes: authorization_code.scopes,
                pkce_required: authorization_code.pkce_required.unwrap_or(false),
            })
        }
        common::OAuthFlows::ClientCredentials { client_credentials } => {
            proto::o_auth_flows::Flow::ClientCredentials(proto::ClientCredentialsFlow {
                token_url: client_credentials.token_url,
                refresh_url: client_credentials.refresh_url.unwrap_or_default(),
                scopes: client_credentials.scopes,
            })
        }
        common::OAuthFlows::Implicit { implicit } => {
            proto::o_auth_flows::Flow::Implicit(proto::ImplicitFlow {
                authorization_url: implicit.authorization_url,
                refresh_url: implicit.refresh_url.unwrap_or_default(),
                scopes: implicit.scopes,
            })
        }
        common::OAuthFlows::Password { password } => {
            proto::o_auth_flows::Flow::Password(proto::PasswordFlow {
                token_url: password.token_url,
                refresh_url: password.refresh_url.unwrap_or_default(),
                scopes: password.scopes,
            })
        }
        common::OAuthFlows::DeviceCode { device_code } => {
            proto::o_auth_flows::Flow::DeviceCode(proto::DeviceCodeOAuthFlow {
                device_authorization_url: device_code.device_authorization_url,
                token_url: device_code.token_url,
                scopes: device_code.scopes,
            })
        }
    };
    proto::OAuthFlows { flow: Some(flow) }
}

fn common_skill_to_proto(skill: common::AgentSkill) -> proto::AgentSkill {
    proto::AgentSkill {
        id: skill.id,
        name: skill.name,
        description: skill.description,
        tags: skill.tags,
        examples: skill.examples,
        input_modes: skill.input_modes,
        output_modes: skill.output_modes,
        security_requirements: skill
            .security_requirements
            .into_iter()
            .map(common_security_requirement_to_proto)
            .collect(),
        tos_base_cost: skill.tos_base_cost.unwrap_or_default(),
    }
}

fn common_signature_to_proto(signature: common::AgentCardSignature) -> proto::AgentCardSignature {
    proto::AgentCardSignature {
        protected: signature.protected,
        signature: signature.signature,
        header: signature
            .header
            .map(|header| json_map_to_proto(&header))
            .unwrap_or_default(),
    }
}

fn common_identity_to_proto(identity: common::TosAgentIdentity) -> proto::TosAgentIdentity {
    proto::TosAgentIdentity {
        agent_account: identity.agent_account.as_bytes().to_vec(),
        controller: identity.controller.as_bytes().to_vec(),
        reputation_score_bps: identity.reputation_score_bps.unwrap_or_default(),
        identity_proof: identity
            .identity_proof
            .map(|proof| proto::TosIdentityProof {
                proof_type: proof.proof_type,
                signature: proof.signature,
                created_at_block: proof.created_at_block,
                expires_at_block: proof.expires_at_block.unwrap_or_default(),
            }),
    }
}

fn common_task_to_proto(task: common::Task) -> proto::Task {
    proto::Task {
        id: task.id,
        context_id: task.context_id,
        status: Some(common_task_status_to_proto(task.status)),
        artifacts: task
            .artifacts
            .into_iter()
            .map(common_artifact_to_proto)
            .collect(),
        history: task
            .history
            .into_iter()
            .map(common_message_to_proto)
            .collect(),
        metadata: task
            .metadata
            .map(|meta| json_map_to_proto(&meta))
            .unwrap_or_default(),
        tos_task_anchor: task.tos_task_anchor.map(common_anchor_to_proto),
    }
}

fn common_task_status_to_proto(status: common::TaskStatus) -> proto::TaskStatus {
    proto::TaskStatus {
        state: common_task_state_to_proto(status.state) as i32,
        message: status.message.map(common_message_to_proto),
        timestamp: status.timestamp.unwrap_or_default(),
    }
}

fn common_task_state_to_proto(state: common::TaskState) -> proto::TaskState {
    match state {
        common::TaskState::Unspecified => proto::TaskState::Unspecified,
        common::TaskState::Submitted => proto::TaskState::Submitted,
        common::TaskState::Working => proto::TaskState::Working,
        common::TaskState::Completed => proto::TaskState::Completed,
        common::TaskState::Failed => proto::TaskState::Failed,
        common::TaskState::Canceled => proto::TaskState::Canceled,
        common::TaskState::InputRequired => proto::TaskState::InputRequired,
        common::TaskState::Rejected => proto::TaskState::Rejected,
        common::TaskState::AuthRequired => proto::TaskState::AuthRequired,
    }
}

fn proto_task_state_to_common(state: i32) -> Option<common::TaskState> {
    match proto::TaskState::try_from(state).ok() {
        Some(proto::TaskState::Unspecified) => Some(common::TaskState::Unspecified),
        Some(proto::TaskState::Submitted) => Some(common::TaskState::Submitted),
        Some(proto::TaskState::Working) => Some(common::TaskState::Working),
        Some(proto::TaskState::Completed) => Some(common::TaskState::Completed),
        Some(proto::TaskState::Failed) => Some(common::TaskState::Failed),
        Some(proto::TaskState::Canceled) => Some(common::TaskState::Canceled),
        Some(proto::TaskState::InputRequired) => Some(common::TaskState::InputRequired),
        Some(proto::TaskState::Rejected) => Some(common::TaskState::Rejected),
        Some(proto::TaskState::AuthRequired) => Some(common::TaskState::AuthRequired),
        _ => None,
    }
}

fn common_anchor_to_proto(anchor: common::TosTaskAnchor) -> proto::TosTaskAnchor {
    proto::TosTaskAnchor {
        escrow_id: anchor.escrow_id,
        agent_account: anchor.agent_account.as_bytes().to_vec(),
        settlement_status: match anchor.settlement_status {
            common::SettlementStatus::None => proto::SettlementStatus::None,
            common::SettlementStatus::EscrowLocked => proto::SettlementStatus::EscrowLocked,
            common::SettlementStatus::Claimed => proto::SettlementStatus::Claimed,
            common::SettlementStatus::Refunded => proto::SettlementStatus::Refunded,
            common::SettlementStatus::Disputed => proto::SettlementStatus::Disputed,
        } as i32,
    }
}

fn common_message_to_proto(message: common::Message) -> proto::Message {
    proto::Message {
        message_id: message.message_id,
        context_id: message.context_id.unwrap_or_default(),
        task_id: message.task_id.unwrap_or_default(),
        role: match message.role {
            common::Role::User => proto::Role::User,
            common::Role::Agent => proto::Role::Agent,
            common::Role::Unspecified => proto::Role::Unspecified,
        } as i32,
        parts: message
            .parts
            .into_iter()
            .map(common_part_to_proto)
            .collect(),
        metadata: message
            .metadata
            .map(|meta| json_map_to_proto(&meta))
            .unwrap_or_default(),
        extensions: message.extensions,
        reference_task_ids: message.reference_task_ids,
    }
}

fn common_part_to_proto(part: common::Part) -> proto::Part {
    let content = match part.content {
        common::PartContent::Text { text } => proto::part::Content::Text(text),
        common::PartContent::Bytes { raw } => proto::part::Content::Raw(raw),
        common::PartContent::Url { url } => proto::part::Content::Url(url),
        common::PartContent::Data { data } => {
            proto::part::Content::Data(json_to_proto_value(&data))
        }
    };
    proto::Part {
        content: Some(content),
        filename: part.filename.unwrap_or_default(),
        media_type: part.media_type.unwrap_or_default(),
        metadata: part
            .metadata
            .map(|meta| json_map_to_proto(&meta))
            .unwrap_or_default(),
    }
}

fn common_artifact_to_proto(artifact: common::Artifact) -> proto::Artifact {
    proto::Artifact {
        artifact_id: artifact.artifact_id,
        name: artifact.name.unwrap_or_default(),
        description: artifact.description.unwrap_or_default(),
        parts: artifact
            .parts
            .into_iter()
            .map(common_part_to_proto)
            .collect(),
        metadata: artifact
            .metadata
            .map(|meta| json_map_to_proto(&meta))
            .unwrap_or_default(),
        extensions: artifact.extensions,
    }
}

fn common_stream_response_to_proto(response: common::StreamResponse) -> proto::StreamResponse {
    let response = match response {
        common::StreamResponse::Task { task } => {
            proto::stream_response::Response::Task(common_task_to_proto(task))
        }
        common::StreamResponse::Message { message } => {
            proto::stream_response::Response::Message(common_message_to_proto(message))
        }
        common::StreamResponse::StatusUpdate { status_update } => {
            proto::stream_response::Response::StatusUpdate(common_status_update_to_proto(
                status_update,
            ))
        }
        common::StreamResponse::ArtifactUpdate { artifact_update } => {
            proto::stream_response::Response::ArtifactUpdate(common_artifact_update_to_proto(
                artifact_update,
            ))
        }
    };
    proto::StreamResponse {
        response: Some(response),
    }
}

fn common_status_update_to_proto(
    event: common::TaskStatusUpdateEvent,
) -> proto::TaskStatusUpdateEvent {
    proto::TaskStatusUpdateEvent {
        task_id: event.task_id,
        context_id: event.context_id,
        status: Some(common_task_status_to_proto(event.status)),
        metadata: event
            .metadata
            .map(|meta| json_map_to_proto(&meta))
            .unwrap_or_default(),
    }
}

fn common_artifact_update_to_proto(
    event: common::TaskArtifactUpdateEvent,
) -> proto::TaskArtifactUpdateEvent {
    proto::TaskArtifactUpdateEvent {
        task_id: event.task_id,
        context_id: event.context_id,
        artifact: Some(common_artifact_to_proto(event.artifact)),
        append: event.append,
        last_chunk: event.last_chunk,
        metadata: event
            .metadata
            .map(|meta| json_map_to_proto(&meta))
            .unwrap_or_default(),
    }
}

fn proto_message_to_common(message: proto::Message) -> Result<common::Message, Status> {
    Ok(common::Message {
        message_id: message.message_id,
        context_id: empty_to_none(message.context_id),
        task_id: empty_to_none(message.task_id),
        role: proto_role_to_common(message.role),
        parts: message
            .parts
            .into_iter()
            .map(proto_part_to_common)
            .collect::<Result<Vec<_>, _>>()?,
        metadata: proto_map_to_json(&message.metadata),
        extensions: message.extensions,
        reference_task_ids: message.reference_task_ids,
    })
}

fn proto_part_to_common(part: proto::Part) -> Result<common::Part, Status> {
    let content = match part.content {
        Some(proto::part::Content::Text(text)) => common::PartContent::Text { text },
        Some(proto::part::Content::Raw(raw)) => common::PartContent::Bytes { raw },
        Some(proto::part::Content::Url(url)) => common::PartContent::Url { url },
        Some(proto::part::Content::Data(data)) => common::PartContent::Data {
            data: proto_to_json_value(&data),
        },
        None => return Err(Status::invalid_argument("part content missing")),
    };
    Ok(common::Part {
        content,
        filename: empty_to_none(part.filename),
        media_type: empty_to_none(part.media_type),
        metadata: proto_map_to_json(&part.metadata),
    })
}

fn proto_role_to_common(role: i32) -> common::Role {
    match proto::Role::try_from(role).ok() {
        Some(proto::Role::User) => common::Role::User,
        Some(proto::Role::Agent) => common::Role::Agent,
        _ => common::Role::Unspecified,
    }
}

fn json_map_to_proto(map: &HashMap<String, serde_json::Value>) -> HashMap<String, Value> {
    map.iter()
        .map(|(k, v)| (k.clone(), json_to_proto_value(v)))
        .collect()
}

fn proto_map_to_json(map: &HashMap<String, Value>) -> Option<HashMap<String, serde_json::Value>> {
    if map.is_empty() {
        None
    } else {
        Some(
            map.iter()
                .map(|(k, v)| (k.clone(), proto_to_json_value(v)))
                .collect(),
        )
    }
}

fn json_to_proto_value(value: &serde_json::Value) -> Value {
    let kind = match value {
        serde_json::Value::Null => value::Kind::NullValue(0),
        serde_json::Value::Bool(v) => value::Kind::BoolValue(*v),
        serde_json::Value::Number(n) => value::Kind::NumberValue(n.as_f64().unwrap_or_default()),
        serde_json::Value::String(s) => value::Kind::StringValue(s.clone()),
        serde_json::Value::Array(arr) => value::Kind::ListValue(prost_types::ListValue {
            values: arr.iter().map(json_to_proto_value).collect(),
        }),
        serde_json::Value::Object(obj) => value::Kind::StructValue(Struct {
            fields: obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_proto_value(v)))
                .collect(),
        }),
    };
    Value { kind: Some(kind) }
}

fn proto_to_json_value(value: &Value) -> serde_json::Value {
    match value.kind.as_ref() {
        Some(value::Kind::NullValue(_)) => serde_json::Value::Null,
        Some(value::Kind::BoolValue(v)) => serde_json::Value::Bool(*v),
        Some(value::Kind::NumberValue(v)) => serde_json::Number::from_f64(*v)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(value::Kind::StringValue(v)) => serde_json::Value::String(v.clone()),
        Some(value::Kind::ListValue(list)) => {
            serde_json::Value::Array(list.values.iter().map(proto_to_json_value).collect())
        }
        Some(value::Kind::StructValue(struct_value)) => serde_json::Value::Object(
            struct_value
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), proto_to_json_value(v)))
                .collect(),
        ),
        None => serde_json::Value::Null,
    }
}

fn empty_to_none(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[allow(dead_code)]
fn bytes_to_public_key(bytes: &[u8]) -> Option<CompressedPublicKey> {
    let compressed = CompressedRistretto::from_slice(bytes).ok()?;
    Some(CompressedPublicKey::new(compressed))
}
