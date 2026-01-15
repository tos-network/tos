use crate::{
    account::{AgentAccountMeta, SessionKey},
    config::TOS_ASSET,
    crypto::{Hash, PublicKey},
    transaction::payload::AgentAccountPayload,
};

use super::{state::BlockchainVerificationState, VerificationError};

const MAX_ALLOWED_TARGETS: usize = 64;
const MAX_ALLOWED_ASSETS: usize = 64;
const MAX_SESSION_KEYS_PER_ACCOUNT: usize = 1024;

pub async fn verify_agent_account_payload<'a, E, B: BlockchainVerificationState<'a, E> + Send>(
    payload: &'a AgentAccountPayload,
    source: &'a PublicKey,
    state: &mut B,
) -> Result<(), VerificationError<E>> {
    let current_topoheight = state.get_verification_topoheight();
    let existing_meta = state
        .get_agent_account_meta(source)
        .await
        .map_err(VerificationError::State)?;

    match payload {
        AgentAccountPayload::Register {
            controller,
            policy_hash,
            energy_pool,
            session_key_root,
        } => {
            if existing_meta.is_some() {
                return Err(VerificationError::AgentAccountAlreadyRegistered);
            }

            // Reject zero owner key (source is the owner)
            if is_zero_key(source) {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }

            if is_zero_key(controller) || controller == source {
                return Err(VerificationError::AgentAccountInvalidController);
            }

            if is_zero_hash(policy_hash) {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }

            if let Some(energy_pool) = energy_pool.as_ref() {
                if is_zero_key(energy_pool)
                    || !state
                        .account_exists(energy_pool)
                        .await
                        .map_err(VerificationError::State)?
                    || (energy_pool != source && energy_pool != controller)
                {
                    return Err(VerificationError::AgentAccountInvalidParameter);
                }
            }

            if let Some(session_key_root) = session_key_root.as_ref() {
                if is_zero_hash(session_key_root) {
                    return Err(VerificationError::AgentAccountInvalidParameter);
                }
            }

            let meta = AgentAccountMeta {
                owner: source.clone(),
                controller: controller.clone(),
                policy_hash: policy_hash.clone(),
                status: 0,
                energy_pool: energy_pool.clone(),
                session_key_root: session_key_root.clone(),
            };

            state
                .set_agent_account_meta(source, &meta)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::UpdatePolicy { policy_hash } => {
            let Some(mut meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if is_zero_hash(policy_hash) {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            meta.policy_hash = policy_hash.clone();
            state
                .set_agent_account_meta(source, &meta)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::RotateController { new_controller } => {
            let Some(mut meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if is_zero_key(new_controller)
                || new_controller == source
                || &meta.controller == new_controller
            {
                return Err(VerificationError::AgentAccountInvalidController);
            }
            // Clear energy_pool if it was set to the old controller
            // (energy_pool must be owner or controller per spec Section 2.5)
            if meta.energy_pool.as_ref() == Some(&meta.controller) {
                meta.energy_pool = None;
            }
            meta.controller = new_controller.clone();
            state
                .set_agent_account_meta(source, &meta)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::SetStatus { status } => {
            let Some(mut meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if *status > 1 {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            meta.status = *status;
            state
                .set_agent_account_meta(source, &meta)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::SetEnergyPool { energy_pool } => {
            let Some(mut meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if let Some(energy_pool) = energy_pool.as_ref() {
                if is_zero_key(energy_pool)
                    || !state
                        .account_exists(energy_pool)
                        .await
                        .map_err(VerificationError::State)?
                    || (energy_pool != source && energy_pool != &meta.controller)
                {
                    return Err(VerificationError::AgentAccountInvalidParameter);
                }
            }
            meta.energy_pool = energy_pool.clone();
            state
                .set_agent_account_meta(source, &meta)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::SetSessionKeyRoot { session_key_root } => {
            let Some(mut meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if let Some(session_key_root) = session_key_root.as_ref() {
                if is_zero_hash(session_key_root) {
                    return Err(VerificationError::AgentAccountInvalidParameter);
                }
                let existing = state
                    .get_session_keys_for_account(source)
                    .await
                    .map_err(VerificationError::State)?;
                if existing
                    .iter()
                    .any(|key| key.expiry_topoheight > current_topoheight)
                {
                    return Err(VerificationError::AgentAccountInvalidParameter);
                }
            }
            meta.session_key_root = session_key_root.clone();
            state
                .set_agent_account_meta(source, &meta)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::AddSessionKey { key } => {
            let Some(meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if meta.session_key_root.is_some() {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            let existing = state
                .get_session_keys_for_account(source)
                .await
                .map_err(VerificationError::State)?;
            let active_keys = existing
                .iter()
                .filter(|key| key.expiry_topoheight > current_topoheight)
                .count();
            if active_keys >= MAX_SESSION_KEYS_PER_ACCOUNT {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            if existing
                .iter()
                .any(|existing_key| existing_key.public_key == key.public_key)
            {
                return Err(VerificationError::AgentAccountSessionKeyExists);
            }
            if is_zero_key(&key.public_key) {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            if key.expiry_topoheight <= current_topoheight {
                return Err(VerificationError::AgentAccountSessionKeyExpired);
            }
            if key.max_value_per_window == 0 {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            if key.allowed_targets.len() > MAX_ALLOWED_TARGETS
                || key.allowed_assets.len() > MAX_ALLOWED_ASSETS
            {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            if key.allowed_targets.iter().any(is_zero_key) {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            if key
                .allowed_assets
                .iter()
                .any(|asset| is_zero_hash(asset) && asset != &TOS_ASSET)
            {
                return Err(VerificationError::AgentAccountInvalidParameter);
            }
            if state
                .get_session_key(source, key.key_id)
                .await
                .map_err(VerificationError::State)?
                .is_some()
            {
                return Err(VerificationError::AgentAccountSessionKeyExists);
            }
            state
                .set_session_key(source, key)
                .await
                .map_err(VerificationError::State)?;
        }
        AgentAccountPayload::RevokeSessionKey { key_id } => {
            let Some(_meta) = existing_meta else {
                return Err(VerificationError::AgentAccountInvalidParameter);
            };
            if state
                .get_session_key(source, *key_id)
                .await
                .map_err(VerificationError::State)?
                .is_none()
            {
                return Err(VerificationError::AgentAccountSessionKeyNotFound);
            }
            state
                .delete_session_key(source, *key_id)
                .await
                .map_err(VerificationError::State)?;
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub fn is_zero_key(key: &PublicKey) -> bool {
    key.as_bytes().iter().all(|b| *b == 0)
}

#[allow(dead_code)]
pub fn is_zero_hash(hash: &Hash) -> bool {
    *hash == Hash::zero()
}

#[allow(dead_code)]
pub fn is_agent_account(meta: &Option<AgentAccountMeta>) -> bool {
    meta.is_some()
}

#[allow(dead_code)]
pub fn session_key_is_expired(session_key: &SessionKey, current_topoheight: u64) -> bool {
    current_topoheight >= session_key.expiry_topoheight
}
