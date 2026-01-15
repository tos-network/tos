use tos_common::{
    account::SessionKey,
    crypto::{Hash, KeyPair},
    serializer::{Reader, Serializer},
    transaction::AgentAccountPayload,
};

#[test]
fn agent_account_register_roundtrip() {
    let controller = KeyPair::new().get_public_key().compress();
    let policy_hash = Hash::new([1u8; 32]);
    let payload = AgentAccountPayload::Register {
        controller,
        policy_hash,
        energy_pool: None,
        session_key_root: None,
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::Register {
            controller: decoded_controller,
            policy_hash: decoded_policy_hash,
            energy_pool,
            session_key_root,
        } => {
            assert_eq!(decoded_controller, controller);
            assert_eq!(decoded_policy_hash, policy_hash);
            assert!(energy_pool.is_none());
            assert!(session_key_root.is_none());
        }
        _ => panic!("unexpected payload variant"),
    }
}

#[test]
fn agent_account_session_key_roundtrip() {
    let session_key = SessionKey {
        key_id: 42,
        public_key: KeyPair::new().get_public_key().compress(),
        expiry_topoheight: 100,
        max_value_per_window: 1000,
        allowed_targets: vec![KeyPair::new().get_public_key().compress()],
        allowed_assets: vec![Hash::new([2u8; 32])],
    };

    let payload = AgentAccountPayload::AddSessionKey {
        key: session_key.clone(),
    };

    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let decoded = AgentAccountPayload::read(&mut reader).expect("decode payload");

    match decoded {
        AgentAccountPayload::AddSessionKey { key } => {
            assert_eq!(key, session_key);
        }
        _ => panic!("unexpected payload variant"),
    }
}
