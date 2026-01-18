use serde::{Deserialize, Serialize};

use crate::{
    arbitration::{ArbiterStatus, ExpertiseDomain},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// UpdateArbiterPayload defines updates to an arbiter account.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdateArbiterPayload {
    /// Optional updated display name.
    name: Option<String>,
    /// Optional expertise domains update.
    expertise: Option<Vec<ExpertiseDomain>>,
    /// Optional fee in basis points.
    fee_basis_points: Option<u16>,
    /// Optional minimum escrow value.
    min_escrow_value: Option<u64>,
    /// Optional maximum escrow value.
    max_escrow_value: Option<u64>,
    /// Additional stake to add.
    add_stake: Option<u64>,
    /// Optional status update (self-suspension).
    status: Option<ArbiterStatus>,
    /// Request exit (starts cooldown for withdrawal).
    deactivate: bool,
}

impl UpdateArbiterPayload {
    /// Create a new UpdateArbiter payload.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: Option<String>,
        expertise: Option<Vec<ExpertiseDomain>>,
        fee_basis_points: Option<u16>,
        min_escrow_value: Option<u64>,
        max_escrow_value: Option<u64>,
        add_stake: Option<u64>,
        status: Option<ArbiterStatus>,
        deactivate: bool,
    ) -> Self {
        Self {
            name,
            expertise,
            fee_basis_points,
            min_escrow_value,
            max_escrow_value,
            add_stake,
            status,
            deactivate,
        }
    }

    /// Get updated name.
    #[inline]
    pub fn get_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Get updated expertise domains.
    #[inline]
    pub fn get_expertise(&self) -> Option<&[ExpertiseDomain]> {
        self.expertise.as_deref()
    }

    /// Get updated fee basis points.
    #[inline]
    pub fn get_fee_basis_points(&self) -> Option<u16> {
        self.fee_basis_points
    }

    /// Get updated min escrow value.
    #[inline]
    pub fn get_min_escrow_value(&self) -> Option<u64> {
        self.min_escrow_value
    }

    /// Get updated max escrow value.
    #[inline]
    pub fn get_max_escrow_value(&self) -> Option<u64> {
        self.max_escrow_value
    }

    /// Get additional stake amount.
    #[inline]
    pub fn get_add_stake(&self) -> Option<u64> {
        self.add_stake
    }

    /// Get updated status.
    #[inline]
    pub fn get_status(&self) -> Option<ArbiterStatus> {
        self.status.clone()
    }

    /// Check whether the arbiter is deactivating.
    #[inline]
    pub fn is_deactivate(&self) -> bool {
        self.deactivate
    }

    /// Consume payload and return inner values.
    #[allow(clippy::type_complexity)]
    pub fn consume(
        self,
    ) -> (
        Option<String>,
        Option<Vec<ExpertiseDomain>>,
        Option<u16>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<ArbiterStatus>,
        bool,
    ) {
        (
            self.name,
            self.expertise,
            self.fee_basis_points,
            self.min_escrow_value,
            self.max_escrow_value,
            self.add_stake,
            self.status,
            self.deactivate,
        )
    }
}

impl Serializer for UpdateArbiterPayload {
    fn write(&self, writer: &mut Writer) {
        self.name.write(writer);
        self.expertise.write(writer);
        self.fee_basis_points.write(writer);
        self.min_escrow_value.write(writer);
        self.max_escrow_value.write(writer);
        self.add_stake.write(writer);
        self.status.write(writer);
        self.deactivate.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            name: Option::read(reader)?,
            expertise: Option::read(reader)?,
            fee_basis_points: Option::read(reader)?,
            min_escrow_value: Option::read(reader)?,
            max_escrow_value: Option::read(reader)?,
            add_stake: Option::read(reader)?,
            status: Option::read(reader)?,
            deactivate: bool::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.name.size()
            + self.expertise.size()
            + self.fee_basis_points.size()
            + self.min_escrow_value.size()
            + self.max_escrow_value.size()
            + self.add_stake.size()
            + self.status.size()
            + self.deactivate.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_arbiter_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = UpdateArbiterPayload::new(
            Some("arbiter-2".to_string()),
            Some(vec![ExpertiseDomain::DeFi]),
            Some(300),
            Some(100),
            Some(1_000_000),
            Some(500_000),
            Some(ArbiterStatus::Suspended),
            false,
        );
        let data = serde_json::to_vec(&payload)?;
        let decoded: UpdateArbiterPayload = serde_json::from_slice(&data)?;
        assert_eq!(payload.get_name(), decoded.get_name());
        assert_eq!(payload.get_expertise(), decoded.get_expertise());
        assert_eq!(
            payload.get_fee_basis_points(),
            decoded.get_fee_basis_points()
        );
        assert_eq!(payload.get_add_stake(), decoded.get_add_stake());
        assert_eq!(payload.is_deactivate(), decoded.is_deactivate());
        Ok(())
    }
}
