// Balance simplification: RangeProof removed from plaintext balance system
#![allow(dead_code)]

use std::hash::Hasher;

use bulletproofs::RangeProof;
use serde::{Deserialize, Serialize};
use tos_vm::{impl_opaque, traits::{DynEq, DynHash, Serializable}};
use crate::{
    contract::opaque::RANGE_PROOF_OPAQUE_ID,
    serializer::*
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeProofWrapper(pub RangeProof);

impl_opaque!("RangeProof", RangeProofWrapper, json);
impl_opaque!("RangeProof", RangeProofWrapper);

impl DynEq for RangeProofWrapper {
    fn is_equal(&self, _: &dyn DynEq) -> bool {
        false
    }

    fn as_eq(&self) -> &dyn DynEq {
        self
    }
}

impl DynHash for RangeProofWrapper {
    fn dyn_hash(&self, _: &mut dyn Hasher) {}
}

impl Serializable for RangeProofWrapper {
    fn get_size(&self) -> usize {
        // Balance simplification: RangeProof size removed
        // self.0.size()
        0
    }

    fn is_serializable(&self) -> bool {
        false // Not serializable in plaintext system
    }

    fn serialize(&self, _buffer: &mut Vec<u8>) -> usize {
        // Balance simplification: RangeProof serialization removed
        // let mut writer = Writer::new(buffer);
        // writer.write_u8(RANGE_PROOF_OPAQUE_ID);
        // self.0.write(&mut writer);
        // writer.total_write()
        panic!("RangeProof serialization removed in plaintext balance system")
    }
}