use tokio::time::{sleep, Duration};
use tos_common::a2a::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill, MAX_SKILLS};
use tos_daemon::a2a::registry::{AgentRegistry, RegistryError};

fn basic_card() -> AgentCard {
    AgentCard {
        protocol_version: "v1".to_string(),
        name: "edge-agent".to_string(),
        description: "edge case".to_string(),
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
        skills: Vec::new(),
        supports_extended_agent_card: None,
        signatures: Vec::new(),
        tos_identity: None,
        arbitration: None,
    }
}

#[tokio::test]
async fn test_validate_agent_card_skill_limit() {
    let registry = AgentRegistry::new();
    let mut card = basic_card();

    for i in 0..=MAX_SKILLS {
        card.skills.push(AgentSkill {
            id: format!("skill-{}", i),
            name: format!("Skill {}", i),
            description: "edge".to_string(),
            examples: Vec::new(),
            tags: Vec::new(),
            input_modes: Vec::new(),
            output_modes: Vec::new(),
            security: Vec::new(),
            tos_base_cost: None,
        });
    }

    let err = registry
        .register(card, "https://agent.example.com/a2a".to_string())
        .await
        .expect_err("too many skills should be rejected");

    assert!(matches!(err, RegistryError::InvalidAgentCard(_)));
}

#[tokio::test]
async fn test_endpoint_too_long_rejected() {
    let registry = AgentRegistry::new();
    let card = basic_card();

    let long_host = "a".repeat(2100);
    let url = format!("https://{}.example.com/a2a", long_host);

    let err = registry
        .register(card, url)
        .await
        .expect_err("endpoint URL too long should be rejected");

    assert!(matches!(err, RegistryError::EndpointUrlBlocked(_)));
}

#[tokio::test]
async fn test_filter_by_any_skill_limit() {
    let registry = AgentRegistry::new();

    let mut skills = Vec::new();
    for i in 0..33 {
        skills.push(format!("skill-{}", i));
    }

    let err = registry
        .filter_by_any_skill(&skills)
        .await
        .expect_err("filter should fail when skill list exceeds limit");

    assert!(matches!(err, RegistryError::FilterInputTooLarge));
}

#[tokio::test]
async fn test_health_checks_mark_inactive() {
    let registry = AgentRegistry::new();
    let card = basic_card();

    let registered = registry
        .register(card, "https://agent.example.com/a2a".to_string())
        .await
        .expect("register agent");

    // Simulate inactivity by waiting past timeout and running health checks.
    sleep(Duration::from_secs(1)).await;
    let updated = registry.run_health_checks(0, 1).await.unwrap();
    assert_eq!(updated, 1);

    let fetched = registry.get(&registered.agent_id).await.unwrap();
    assert_eq!(
        fetched.status,
        tos_daemon::a2a::registry::AgentStatus::Inactive
    );
}

#[tokio::test]
async fn test_heartbeat_reactivates_inactive() {
    let registry = AgentRegistry::new();
    let card = basic_card();

    let registered = registry
        .register(card, "https://agent.example.com/a2a".to_string())
        .await
        .expect("register agent");

    registry.mark_inactive(&registered.agent_id).await.unwrap();

    let before = registry.get(&registered.agent_id).await.unwrap();
    assert_eq!(
        before.status,
        tos_daemon::a2a::registry::AgentStatus::Inactive
    );

    registry
        .heartbeat(&registered.agent_id, None)
        .await
        .unwrap();

    let after = registry.get(&registered.agent_id).await.unwrap();
    assert_eq!(after.status, tos_daemon::a2a::registry::AgentStatus::Active);
}
