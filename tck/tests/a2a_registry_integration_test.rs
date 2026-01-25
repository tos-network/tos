use tos_common::a2a::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill, TosAgentIdentity};
use tos_common::crypto::{KeyPair, PublicKey};
use tos_daemon::a2a::registry::{AgentRegistry, AgentStatus, RegistryError};

fn make_public_key() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

fn basic_card(name: &str, tos_identity: Option<TosAgentIdentity>) -> AgentCard {
    AgentCard {
        protocol_version: "v1".to_string(),
        name: name.to_string(),
        description: "test agent".to_string(),
        version: "0.1.0".to_string(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/a2a".to_string(),
            protocol_binding: "https".to_string(),
            tenant: None,
        }],
        provider: None,
        icon_url: None,
        documentation_url: None,
        capabilities: AgentCapabilities {
            streaming: None,
            push_notifications: None,
            state_transition_history: None,
            extensions: Vec::new(),
            tos_on_chain_settlement: None,
        },
        security_schemes: Default::default(),
        security: Vec::new(),
        default_input_modes: Vec::new(),
        default_output_modes: Vec::new(),
        skills: Vec::<AgentSkill>::new(),
        supports_extended_agent_card: None,
        signatures: Vec::new(),
        tos_identity,
        arbitration: None,
    }
}

#[tokio::test]
async fn test_register_get_update_unregister() {
    let registry = AgentRegistry::new();
    let card = basic_card("agent-one", None);

    let registered = registry
        .register(card, "https://agent.example.com/a2a".to_string())
        .await
        .expect("register agent");

    let fetched = registry.get(&registered.agent_id).await;
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.status, AgentStatus::Active);

    let updated_card = basic_card("agent-one-updated", None);
    let updated = registry
        .update(&registered.agent_id, updated_card)
        .await
        .expect("update agent");
    assert_eq!(updated.agent_id, registered.agent_id);
    assert_eq!(updated.agent_card.name, "agent-one-updated");

    registry
        .unregister(&registered.agent_id)
        .await
        .expect("unregister agent");
    assert!(registry.get(&registered.agent_id).await.is_none());
}

#[tokio::test]
async fn test_duplicate_registration_rejected() {
    let registry = AgentRegistry::new();
    let card = basic_card("agent-one", None);

    let _ = registry
        .register(card.clone(), "https://agent.example.com/a2a".to_string())
        .await
        .expect("first registration");

    let err = registry
        .register(card, "https://agent.example.com/a2a".to_string())
        .await
        .expect_err("duplicate registration should fail");

    assert!(matches!(err, RegistryError::AgentAlreadyRegistered));
}

#[tokio::test]
async fn test_agent_account_conflict_rejected() {
    let registry = AgentRegistry::new();
    let agent_account = make_public_key();
    let controller = make_public_key();

    let card_one = basic_card(
        "agent-one",
        Some(TosAgentIdentity {
            agent_account: agent_account.clone(),
            controller: controller.clone(),
            reputation_score_bps: None,
            identity_proof: None,
        }),
    );

    let card_two = basic_card(
        "agent-two",
        Some(TosAgentIdentity {
            agent_account: agent_account.clone(),
            controller,
            reputation_score_bps: None,
            identity_proof: None,
        }),
    );

    let _ = registry
        .register(card_one, "https://agent.example.com/a2a".to_string())
        .await
        .expect("first registration");

    let err = registry
        .register(card_two, "https://agent2.example.com/a2a".to_string())
        .await
        .expect_err("account conflict should fail");

    assert!(matches!(err, RegistryError::AgentAccountAlreadyRegistered));
}

#[tokio::test]
async fn test_invalid_endpoint_rejected() {
    let registry = AgentRegistry::new();
    let card = basic_card("agent-one", None);

    let err = registry
        .register(card.clone(), "http://example.com/a2a".to_string())
        .await
        .expect_err("http endpoints should be rejected");

    assert!(matches!(err, RegistryError::InvalidEndpointUrl));

    let err = registry
        .register(card, "https://localhost/a2a".to_string())
        .await
        .expect_err("localhost endpoints should be rejected");

    assert!(matches!(err, RegistryError::EndpointUrlBlocked(_)));
}

#[tokio::test]
async fn test_get_by_account_returns_agent() {
    let registry = AgentRegistry::new();
    let agent_account = make_public_key();
    let controller = make_public_key();

    let card = basic_card(
        "agent-one",
        Some(TosAgentIdentity {
            agent_account: agent_account.clone(),
            controller,
            reputation_score_bps: None,
            identity_proof: None,
        }),
    );

    let registered = registry
        .register(card, "https://agent.example.com/a2a".to_string())
        .await
        .expect("register agent");

    let fetched = registry.get_by_account(&agent_account).await;
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().agent_id, registered.agent_id);
}

#[tokio::test]
async fn test_list_active_filters_status() {
    let registry = AgentRegistry::new();

    let first = registry
        .register(
            basic_card("agent-one", None),
            "https://agent1.example.com/a2a".to_string(),
        )
        .await
        .expect("register agent 1");

    let second = registry
        .register(
            basic_card("agent-two", None),
            "https://agent2.example.com/a2a".to_string(),
        )
        .await
        .expect("register agent 2");

    // Mark one agent inactive
    registry
        .mark_inactive(&second.agent_id)
        .await
        .expect("mark inactive");

    let active = registry.list_active().await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].agent_id, first.agent_id);
}
