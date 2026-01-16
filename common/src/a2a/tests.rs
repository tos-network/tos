use std::collections::HashMap;

use serde_json::json;

use super::*;

#[test]
fn test_message_roundtrip() {
    let mut meta = HashMap::new();
    meta.insert("traceId".to_string(), json!("abc-123"));

    let mut data = HashMap::new();
    data.insert("key".to_string(), json!("value"));

    let message = Message {
        message_id: "msg-001".to_string(),
        context_id: Some("ctx-001".to_string()),
        task_id: Some("task-001".to_string()),
        role: Role::User,
        parts: vec![
            Part {
                content: PartContent::Text {
                    text: "hello".to_string(),
                },
                metadata: None,
            },
            Part {
                content: PartContent::File {
                    file: FilePart {
                        file: FileContent::Uri {
                            file_with_uri: "https://example.com/file.txt".to_string(),
                        },
                        media_type: Some("text/plain".to_string()),
                        name: Some("file.txt".to_string()),
                    },
                },
                metadata: None,
            },
            Part {
                content: PartContent::Data {
                    data: DataPart { data },
                },
                metadata: Some(meta.clone()),
            },
        ],
        metadata: Some(meta),
        extensions: vec!["urn:example:ext".to_string()],
        reference_task_ids: vec!["task-002".to_string()],
    };

    let value = serde_json::to_value(&message).expect("serialize message");
    assert_eq!(value["messageId"], "msg-001");
    assert_eq!(value["role"], "user");
    assert_eq!(value["parts"][0]["text"], "hello");
    assert_eq!(
        value["parts"][1]["file"]["fileWithUri"],
        "https://example.com/file.txt"
    );
    assert_eq!(value["parts"][2]["data"]["data"]["key"], "value");

    let parsed: Message = serde_json::from_value(value.clone()).expect("deserialize message");
    let value2 = serde_json::to_value(&parsed).expect("serialize message again");
    assert_eq!(value, value2);
}

#[test]
fn test_agent_card_roundtrip() {
    let mut schemes = HashMap::new();
    schemes.insert(
        "apiKeyAuth".to_string(),
        SecurityScheme::ApiKey {
            api_key_security_scheme: ApiKeySecurityScheme {
                description: Some("api key".to_string()),
                location: "header".to_string(),
                name: "x-api-key".to_string(),
            },
        },
    );
    schemes.insert(
        "bearer".to_string(),
        SecurityScheme::HttpAuth {
            http_auth_security_scheme: HttpAuthSecurityScheme {
                description: None,
                scheme: "Bearer".to_string(),
                bearer_format: Some("JWT".to_string()),
            },
        },
    );

    let mut scopes = HashMap::new();
    scopes.insert("agent:read".to_string(), "Read access".to_string());

    schemes.insert(
        "oauth2".to_string(),
        SecurityScheme::OAuth2 {
            oauth2_security_scheme: OAuth2SecurityScheme {
                description: None,
                flows: OAuthFlows::ClientCredentials {
                    client_credentials: ClientCredentialsFlow {
                        token_url: "https://auth.example.com/token".to_string(),
                        refresh_url: None,
                        scopes,
                    },
                },
                oauth2_metadata_url: None,
            },
        },
    );

    let agent_card = AgentCard {
        protocol_version: "1.0".to_string(),
        name: "Test Agent".to_string(),
        description: "Agent description".to_string(),
        version: "0.1.0".to_string(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/a2a".to_string(),
            protocol_binding: "JSONRPC".to_string(),
            tenant: None,
        }],
        provider: Some(AgentProvider {
            url: "https://example.com".to_string(),
            organization: "Example Org".to_string(),
        }),
        icon_url: None,
        documentation_url: None,
        capabilities: AgentCapabilities {
            streaming: Some(true),
            push_notifications: Some(true),
            state_transition_history: None,
            extensions: vec![],
            tos_on_chain_settlement: None,
        },
        security_schemes: schemes,
        security: vec![Security {
            schemes: HashMap::new(),
        }],
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills: vec![AgentSkill {
            id: "skill-1".to_string(),
            name: "Skill".to_string(),
            description: "Skill desc".to_string(),
            tags: vec!["tag".to_string()],
            examples: vec!["example".to_string()],
            input_modes: vec![],
            output_modes: vec![],
            security: vec![],
            tos_base_cost: None,
        }],
        supports_extended_agent_card: Some(true),
        signatures: vec![],
        tos_identity: None,
    };

    let value = serde_json::to_value(&agent_card).expect("serialize agent card");
    assert_eq!(value["protocolVersion"], "1.0");
    assert_eq!(
        value["supportedInterfaces"][0]["protocolBinding"],
        "JSONRPC"
    );
    assert_eq!(
        value["securitySchemes"]["apiKeyAuth"]["apiKeySecurityScheme"]["name"],
        "x-api-key"
    );

    let parsed: AgentCard = serde_json::from_value(value.clone()).expect("deserialize agent card");
    let value2 = serde_json::to_value(&parsed).expect("serialize agent card again");
    assert_eq!(value, value2);
}

#[test]
fn test_enum_serialization() {
    let state = TaskState::InputRequired;
    let state_value = serde_json::to_value(state).expect("serialize task state");
    assert_eq!(state_value, json!("input-required"));

    let signer = TosSignerType::SessionKey;
    let signer_value = serde_json::to_value(signer).expect("serialize signer");
    assert_eq!(signer_value, json!("session-key"));
}

#[test]
fn test_task_roundtrip() {
    let task = Task {
        id: "task-123".to_string(),
        context_id: "ctx-123".to_string(),
        status: TaskStatus {
            state: TaskState::Working,
            message: Some(Message {
                message_id: "msg-123".to_string(),
                context_id: Some("ctx-123".to_string()),
                task_id: Some("task-123".to_string()),
                role: Role::Agent,
                parts: vec![Part {
                    content: PartContent::Text {
                        text: "processing".to_string(),
                    },
                    metadata: None,
                }],
                metadata: None,
                extensions: vec![],
                reference_task_ids: vec![],
            }),
            timestamp: Some("2024-01-08T12:00:00Z".to_string()),
        },
        artifacts: vec![Artifact {
            artifact_id: "art-1".to_string(),
            name: Some("result".to_string()),
            description: None,
            parts: vec![Part {
                content: PartContent::Text {
                    text: "output".to_string(),
                },
                metadata: None,
            }],
            metadata: None,
            extensions: vec![],
        }],
        history: vec![],
        metadata: None,
        tos_task_anchor: None,
    };

    let value = serde_json::to_value(&task).expect("serialize task");
    assert_eq!(value["contextId"], "ctx-123");
    assert_eq!(value["status"]["state"], "working");
    assert_eq!(value["artifacts"][0]["artifactId"], "art-1");

    let parsed: Task = serde_json::from_value(value.clone()).expect("deserialize task");
    let value2 = serde_json::to_value(&parsed).expect("serialize task again");
    assert_eq!(value, value2);
}

#[test]
fn test_push_notification_config_roundtrip() {
    let config = PushNotificationConfig {
        id: Some("cfg-1".to_string()),
        url: "https://client.example.com/hook".to_string(),
        token: Some("tok-123".to_string()),
        authentication: Some(AuthenticationInfo {
            schemes: vec!["Bearer".to_string()],
            credentials: Some("secret".to_string()),
        }),
    };

    let value = serde_json::to_value(&config).expect("serialize push config");
    assert_eq!(value["id"], "cfg-1");
    assert_eq!(value["url"], "https://client.example.com/hook");
    assert_eq!(value["authentication"]["schemes"][0], "Bearer");

    let parsed: PushNotificationConfig =
        serde_json::from_value(value.clone()).expect("deserialize push config");
    let value2 = serde_json::to_value(&parsed).expect("serialize push config again");
    assert_eq!(value, value2);
}

#[test]
fn test_stream_response_roundtrip() {
    let status_update = StreamResponse::StatusUpdate {
        status_update: TaskStatusUpdateEvent {
            task_id: "task-123".to_string(),
            context_id: "ctx-123".to_string(),
            status: TaskStatus {
                state: TaskState::Working,
                message: None,
                timestamp: None,
            },
            r#final: false,
            metadata: None,
        },
    };

    let value = serde_json::to_value(&status_update).expect("serialize stream response");
    assert!(value.get("statusUpdate").is_some());
    assert_eq!(value["statusUpdate"]["taskId"], "task-123");

    let parsed: StreamResponse =
        serde_json::from_value(value.clone()).expect("deserialize stream response");
    let value2 = serde_json::to_value(&parsed).expect("serialize stream response again");
    assert_eq!(value, value2);
}

#[test]
fn test_stream_artifact_update_roundtrip() {
    let update = StreamResponse::ArtifactUpdate {
        artifact_update: TaskArtifactUpdateEvent {
            task_id: "task-777".to_string(),
            context_id: "ctx-777".to_string(),
            artifact: Artifact {
                artifact_id: "art-777".to_string(),
                name: Some("chunk".to_string()),
                description: None,
                parts: vec![Part {
                    content: PartContent::Text {
                        text: "partial".to_string(),
                    },
                    metadata: None,
                }],
                metadata: None,
                extensions: vec![],
            },
            append: true,
            last_chunk: false,
            metadata: None,
        },
    };

    let value = serde_json::to_value(&update).expect("serialize artifact update");
    assert!(value.get("artifactUpdate").is_some());
    assert_eq!(value["artifactUpdate"]["artifact"]["artifactId"], "art-777");

    let parsed: StreamResponse =
        serde_json::from_value(value.clone()).expect("deserialize artifact update");
    let value2 = serde_json::to_value(&parsed).expect("serialize artifact update again");
    assert_eq!(value, value2);
}

#[test]
fn test_security_scheme_roundtrip() {
    let variants = vec![
        SecurityScheme::ApiKey {
            api_key_security_scheme: ApiKeySecurityScheme {
                description: Some("api key".to_string()),
                location: "header".to_string(),
                name: "x-api-key".to_string(),
            },
        },
        SecurityScheme::HttpAuth {
            http_auth_security_scheme: HttpAuthSecurityScheme {
                description: Some("bearer auth".to_string()),
                scheme: "Bearer".to_string(),
                bearer_format: Some("JWT".to_string()),
            },
        },
        SecurityScheme::OAuth2 {
            oauth2_security_scheme: OAuth2SecurityScheme {
                description: None,
                flows: OAuthFlows::AuthorizationCode {
                    authorization_code: AuthorizationCodeFlow {
                        authorization_url: "https://auth.example.com/authorize".to_string(),
                        token_url: "https://auth.example.com/token".to_string(),
                        refresh_url: None,
                        scopes: HashMap::new(),
                    },
                },
                oauth2_metadata_url: None,
            },
        },
        SecurityScheme::OpenIdConnect {
            open_id_connect_security_scheme: OpenIdConnectSecurityScheme {
                description: None,
                open_id_connect_url: "https://auth.example.com/.well-known/openid-configuration"
                    .to_string(),
            },
        },
        SecurityScheme::MutualTls {
            mutual_tls_security_scheme: MutualTlsSecurityScheme { description: None },
        },
        SecurityScheme::TosSignature {
            tos_signature_security_scheme: TosSignatureSecurityScheme {
                description: None,
                chain_id: 1337,
                allowed_signers: vec![TosSignerType::Owner],
            },
        },
    ];

    for variant in variants {
        let value = serde_json::to_value(&variant).expect("serialize scheme");
        let parsed: SecurityScheme = serde_json::from_value(value.clone()).expect("deserialize");
        let value2 = serde_json::to_value(&parsed).expect("serialize scheme again");
        assert_eq!(value, value2);
    }
}

#[test]
fn test_send_message_roundtrip() {
    let request = SendMessageRequest {
        tenant: Some("tenant-1".to_string()),
        message: Message {
            message_id: "msg-901".to_string(),
            context_id: None,
            task_id: None,
            role: Role::User,
            parts: vec![Part {
                content: PartContent::Text {
                    text: "hello".to_string(),
                },
                metadata: None,
            }],
            metadata: None,
            extensions: vec![],
            reference_task_ids: vec![],
        },
        configuration: Some(SendMessageConfiguration {
            accepted_output_modes: vec!["text/plain".to_string()],
            push_notification_config: None,
            history_length: Some(10),
            blocking: false,
        }),
        metadata: None,
    };

    let value = serde_json::to_value(&request).expect("serialize request");
    assert_eq!(value["message"]["messageId"], "msg-901");
    assert_eq!(value["configuration"]["historyLength"], 10);

    let parsed: SendMessageRequest =
        serde_json::from_value(value.clone()).expect("deserialize request");
    let value2 = serde_json::to_value(&parsed).expect("serialize request again");
    assert_eq!(value, value2);

    let response = SendMessageResponse::Task {
        task: Box::new(Task {
            id: "task-901".to_string(),
            context_id: "ctx-901".to_string(),
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: None,
            },
            artifacts: vec![],
            history: vec![],
            metadata: None,
            tos_task_anchor: None,
        }),
    };

    let value = serde_json::to_value(&response).expect("serialize response");
    assert_eq!(value["task"]["id"], "task-901");

    let parsed: SendMessageResponse =
        serde_json::from_value(value.clone()).expect("deserialize response");
    let value2 = serde_json::to_value(&parsed).expect("serialize response again");
    assert_eq!(value, value2);
}

#[test]
fn test_list_tasks_roundtrip() {
    let request = ListTasksRequest {
        tenant: Some("tenant-1".to_string()),
        context_id: Some("ctx-555".to_string()),
        status: Some(TaskState::Completed),
        page_size: Some(25),
        page_token: Some("token-1".to_string()),
        history_length: Some(5),
        last_updated_after: Some(1_700_000_000),
        include_artifacts: Some(false),
    };

    let value = serde_json::to_value(&request).expect("serialize list tasks request");
    assert_eq!(value["contextId"], "ctx-555");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["pageSize"], 25);

    let parsed: ListTasksRequest =
        serde_json::from_value(value.clone()).expect("deserialize list tasks request");
    let value2 = serde_json::to_value(&parsed).expect("serialize list tasks request again");
    assert_eq!(value, value2);

    let response = ListTasksResponse {
        tasks: vec![Task {
            id: "task-1".to_string(),
            context_id: "ctx-555".to_string(),
            status: TaskStatus {
                state: TaskState::Completed,
                message: None,
                timestamp: None,
            },
            artifacts: vec![],
            history: vec![],
            metadata: None,
            tos_task_anchor: None,
        }],
        next_page_token: "next-1".to_string(),
        page_size: 25,
        total_size: 1,
    };

    let value = serde_json::to_value(&response).expect("serialize list tasks response");
    assert_eq!(value["tasks"][0]["id"], "task-1");
    assert_eq!(value["nextPageToken"], "next-1");

    let parsed: ListTasksResponse =
        serde_json::from_value(value.clone()).expect("deserialize list tasks response");
    let value2 = serde_json::to_value(&parsed).expect("serialize list tasks response again");
    assert_eq!(value, value2);
}

#[test]
fn test_push_config_request_wrappers_roundtrip() {
    let config = TaskPushNotificationConfig {
        name: "tasks/task-1/pushNotificationConfigs/cfg-1".to_string(),
        push_notification_config: PushNotificationConfig {
            id: Some("cfg-1".to_string()),
            url: "https://client.example.com/hook".to_string(),
            token: Some("tok-1".to_string()),
            authentication: None,
        },
    };

    let set_request = SetTaskPushNotificationConfigRequest {
        tenant: Some("tenant-1".to_string()),
        parent: "tasks/task-1".to_string(),
        config_id: "cfg-1".to_string(),
        config: config.clone(),
    };

    let value = serde_json::to_value(&set_request).expect("serialize set push config request");
    assert_eq!(value["parent"], "tasks/task-1");
    assert_eq!(value["configId"], "cfg-1");
    assert_eq!(value["config"]["name"], config.name);

    let parsed: SetTaskPushNotificationConfigRequest =
        serde_json::from_value(value.clone()).expect("deserialize set request");
    let value2 = serde_json::to_value(&parsed).expect("serialize set push config request again");
    assert_eq!(value, value2);

    let get_request = GetTaskPushNotificationConfigRequest {
        tenant: Some("tenant-1".to_string()),
        name: config.name.clone(),
    };

    let value = serde_json::to_value(&get_request).expect("serialize get push config request");
    assert_eq!(value["name"], config.name);

    let parsed: GetTaskPushNotificationConfigRequest =
        serde_json::from_value(value.clone()).expect("deserialize get request");
    let value2 = serde_json::to_value(&parsed).expect("serialize get push config request again");
    assert_eq!(value, value2);

    let list_request = ListTaskPushNotificationConfigRequest {
        tenant: Some("tenant-1".to_string()),
        parent: "tasks/task-1".to_string(),
        page_size: Some(10),
        page_token: Some("page-1".to_string()),
    };

    let value = serde_json::to_value(&list_request).expect("serialize list push config request");
    assert_eq!(value["parent"], "tasks/task-1");
    assert_eq!(value["pageSize"], 10);

    let parsed: ListTaskPushNotificationConfigRequest =
        serde_json::from_value(value.clone()).expect("deserialize list request");
    let value2 = serde_json::to_value(&parsed).expect("serialize list push config request again");
    assert_eq!(value, value2);

    let list_response = ListTaskPushNotificationConfigResponse {
        configs: vec![config],
        next_page_token: "next-1".to_string(),
    };

    let value = serde_json::to_value(&list_response).expect("serialize list response");
    assert_eq!(
        value["configs"][0]["name"],
        "tasks/task-1/pushNotificationConfigs/cfg-1"
    );
    assert_eq!(value["nextPageToken"], "next-1");

    let parsed: ListTaskPushNotificationConfigResponse =
        serde_json::from_value(value.clone()).expect("deserialize list response");
    let value2 = serde_json::to_value(&parsed).expect("serialize list response again");
    assert_eq!(value, value2);

    let delete_request = DeleteTaskPushNotificationConfigRequest {
        tenant: Some("tenant-1".to_string()),
        name: "tasks/task-1/pushNotificationConfigs/cfg-1".to_string(),
    };

    let value = serde_json::to_value(&delete_request).expect("serialize delete request");
    assert_eq!(value["name"], "tasks/task-1/pushNotificationConfigs/cfg-1");

    let parsed: DeleteTaskPushNotificationConfigRequest =
        serde_json::from_value(value.clone()).expect("deserialize delete request");
    let value2 = serde_json::to_value(&parsed).expect("serialize delete request again");
    assert_eq!(value, value2);
}
