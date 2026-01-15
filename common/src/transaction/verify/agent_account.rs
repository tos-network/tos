use crate::{
    account::{AgentAccountMeta, SessionKey},
    crypto::{Hash, PublicKey},
    transaction::payload::AgentAccountPayload,
};

use super::{state::BlockchainVerificationState, VerificationError};

pub async fn verify_agent_account_payload<'a, E, B: BlockchainVerificationState<'a, E>>(
    _payload: &'a AgentAccountPayload,
    _state: &mut B,
) -> Result<(), VerificationError<E>> {
    // Skeleton validation hook.
    // Full checks are defined in AGENT-ACCOUNT-PROTOCOL.md.
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
