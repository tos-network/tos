use serde::{Deserialize, Serialize};

use crate::{
    arbitration::ExpertiseDomain,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// RegisterArbiterPayload defines initial arbiter registration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisterArbiterPayload {
    /// Display name.
    name: String,
    /// Expertise domains.
    expertise: Vec<ExpertiseDomain>,
    /// Initial stake amount.
    stake_amount: u64,
    /// Minimum escrow value willing to handle.
    min_escrow_value: u64,
    /// Maximum escrow value willing to handle.
    max_escrow_value: u64,
    /// Fee in basis points.
    fee_basis_points: u16,
}

impl RegisterArbiterPayload {
    /// Create a new RegisterArbiter payload.
    pub fn new(
        name: String,
        expertise: Vec<ExpertiseDomain>,
        stake_amount: u64,
        min_escrow_value: u64,
        max_escrow_value: u64,
        fee_basis_points: u16,
    ) -> Self {
        Self {
            name,
            expertise,
            stake_amount,
            min_escrow_value,
            max_escrow_value,
            fee_basis_points,
        }
    }

    /// Get arbiter name.
    #[inline]
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get expertise domains.
    #[inline]
    pub fn get_expertise(&self) -> &[ExpertiseDomain] {
        &self.expertise
    }

    /// Get stake amount.
    #[inline]
    pub fn get_stake_amount(&self) -> u64 {
        self.stake_amount
    }

    /// Get min escrow value.
    #[inline]
    pub fn get_min_escrow_value(&self) -> u64 {
        self.min_escrow_value
    }

    /// Get max escrow value.
    #[inline]
    pub fn get_max_escrow_value(&self) -> u64 {
        self.max_escrow_value
    }

    /// Get fee basis points.
    #[inline]
    pub fn get_fee_basis_points(&self) -> u16 {
        self.fee_basis_points
    }

    /// Consume payload and return inner values.
    pub fn consume(self) -> (String, Vec<ExpertiseDomain>, u64, u64, u64, u16) {
        (
            self.name,
            self.expertise,
            self.stake_amount,
            self.min_escrow_value,
            self.max_escrow_value,
            self.fee_basis_points,
        )
    }
}

impl Serializer for RegisterArbiterPayload {
    fn write(&self, writer: &mut Writer) {
        self.name.write(writer);
        self.expertise.write(writer);
        self.stake_amount.write(writer);
        self.min_escrow_value.write(writer);
        self.max_escrow_value.write(writer);
        self.fee_basis_points.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            name: String::read(reader)?,
            expertise: Vec::read(reader)?,
            stake_amount: u64::read(reader)?,
            min_escrow_value: u64::read(reader)?,
            max_escrow_value: u64::read(reader)?,
            fee_basis_points: u16::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.name.size()
            + self.expertise.size()
            + self.stake_amount.size()
            + self.min_escrow_value.size()
            + self.max_escrow_value.size()
            + self.fee_basis_points.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_arbiter_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = RegisterArbiterPayload::new(
            "arbiter-1".to_string(),
            vec![ExpertiseDomain::General, ExpertiseDomain::Payment],
            1_000_000,
            10,
            1_000_000,
            200,
        );
        let data = serde_json::to_vec(&payload)?;
        let decoded: RegisterArbiterPayload = serde_json::from_slice(&data)?;
        assert_eq!(payload.get_name(), decoded.get_name());
        assert_eq!(payload.get_expertise(), decoded.get_expertise());
        assert_eq!(payload.get_stake_amount(), decoded.get_stake_amount());
        Ok(())
    }
}
