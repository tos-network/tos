// Arbitration transaction verification module.

use crate::{
    arbitration::ArbiterStatus,
    config::MIN_ARBITER_STAKE,
    transaction::payload::{RegisterArbiterPayload, SlashArbiterPayload, UpdateArbiterPayload},
};

use super::VerificationError;

/// Maximum arbiter display name length.
pub const MAX_ARBITER_NAME_LEN: usize = 128;

/// Maximum fee basis points.
pub const MAX_FEE_BPS: u16 = 10_000;

/// Verify RegisterArbiter payload.
pub fn verify_register_arbiter<E>(
    payload: &RegisterArbiterPayload,
) -> Result<(), VerificationError<E>> {
    let name_len = payload.get_name().len();
    if name_len == 0 || name_len > MAX_ARBITER_NAME_LEN {
        return Err(VerificationError::ArbiterNameLength {
            len: name_len,
            max: MAX_ARBITER_NAME_LEN,
        });
    }

    if payload.get_fee_basis_points() > MAX_FEE_BPS {
        return Err(VerificationError::ArbiterInvalidFee(
            payload.get_fee_basis_points(),
        ));
    }

    let stake_amount = payload.get_stake_amount();
    if stake_amount < MIN_ARBITER_STAKE {
        return Err(VerificationError::ArbiterStakeTooLow {
            required: MIN_ARBITER_STAKE,
            found: stake_amount,
        });
    }

    let min_value = payload.get_min_escrow_value();
    let max_value = payload.get_max_escrow_value();
    if min_value > max_value {
        return Err(VerificationError::ArbiterEscrowRangeInvalid {
            min: min_value,
            max: max_value,
        });
    }

    Ok(())
}

/// Verify UpdateArbiter payload.
pub fn verify_update_arbiter<E>(
    payload: &UpdateArbiterPayload,
) -> Result<(), VerificationError<E>> {
    if let Some(name) = payload.get_name() {
        let name_len = name.len();
        if name_len == 0 || name_len > MAX_ARBITER_NAME_LEN {
            return Err(VerificationError::ArbiterNameLength {
                len: name_len,
                max: MAX_ARBITER_NAME_LEN,
            });
        }
    }

    if let Some(fee) = payload.get_fee_basis_points() {
        if fee > MAX_FEE_BPS {
            return Err(VerificationError::ArbiterInvalidFee(fee));
        }
    }

    if let (Some(min_value), Some(max_value)) = (
        payload.get_min_escrow_value(),
        payload.get_max_escrow_value(),
    ) {
        if min_value > max_value {
            return Err(VerificationError::ArbiterEscrowRangeInvalid {
                min: min_value,
                max: max_value,
            });
        }
    }

    if let Some(status) = payload.get_status() {
        if status != ArbiterStatus::Suspended {
            return Err(VerificationError::ArbiterInvalidStatus);
        }
    }

    if payload.is_deactivate() {
        if let Some(add_stake) = payload.get_add_stake() {
            if add_stake > 0 {
                return Err(VerificationError::ArbiterDeactivateWithStake);
            }
        }
    }

    Ok(())
}

/// Verify SlashArbiter payload.
pub fn verify_slash_arbiter<E>(payload: &SlashArbiterPayload) -> Result<(), VerificationError<E>> {
    if payload.get_amount() == 0 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Slash amount must be greater than 0"
        )));
    }

    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "SlashArbiter requires at least one approval"
        )));
    }

    if payload.get_reason_hash() == &crate::crypto::Hash::zero() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "SlashArbiter reason hash cannot be empty"
        )));
    }

    Ok(())
}
