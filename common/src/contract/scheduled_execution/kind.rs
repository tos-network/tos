// ScheduledExecutionKind - defines when a scheduled execution should run

use serde::{Deserialize, Serialize};
use tos_kernel::ValueCell;

use crate::{block::TopoHeight, serializer::*};

/// Kind of scheduled execution - when it should be executed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledExecutionKind {
    /// Execute at a specific topoheight
    TopoHeight(TopoHeight),
    /// Execute at the end of the current block
    BlockEnd,
}

impl ScheduledExecutionKind {
    /// Get the ID for serialization
    pub fn id(&self) -> u8 {
        match self {
            ScheduledExecutionKind::TopoHeight(_) => 0,
            ScheduledExecutionKind::BlockEnd => 1,
        }
    }
}

/// Log variant for scheduled execution kind (includes execution details for BlockEnd)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledExecutionKindLog {
    /// Execution scheduled for a specific topoheight
    TopoHeight { topoheight: TopoHeight },
    /// Execution scheduled for block end with inlined parameters
    BlockEnd {
        chunk_id: u16,
        max_gas: u64,
        params: Vec<ValueCell>,
    },
}

impl Serializer for ScheduledExecutionKind {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let tag = reader.read_u8()?;
        match tag {
            0 => Ok(ScheduledExecutionKind::TopoHeight(u64::read(reader)?)),
            1 => Ok(ScheduledExecutionKind::BlockEnd),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn write(&self, writer: &mut Writer) {
        match self {
            ScheduledExecutionKind::TopoHeight(topoheight) => {
                writer.write_u8(0);
                topoheight.write(writer);
            }
            ScheduledExecutionKind::BlockEnd => {
                writer.write_u8(1);
            }
        }
    }

    fn size(&self) -> usize {
        1 + match self {
            ScheduledExecutionKind::TopoHeight(topoheight) => topoheight.size(),
            ScheduledExecutionKind::BlockEnd => 0,
        }
    }
}

impl Serializer for ScheduledExecutionKindLog {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let tag = reader.read_u8()?;
        match tag {
            0 => Ok(ScheduledExecutionKindLog::TopoHeight {
                topoheight: TopoHeight::read(reader)?,
            }),
            1 => Ok(ScheduledExecutionKindLog::BlockEnd {
                chunk_id: u16::read(reader)?,
                max_gas: u64::read(reader)?,
                params: Vec::read(reader)?,
            }),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn write(&self, writer: &mut Writer) {
        match self {
            ScheduledExecutionKindLog::TopoHeight { topoheight } => {
                writer.write_u8(0);
                topoheight.write(writer);
            }
            ScheduledExecutionKindLog::BlockEnd {
                chunk_id,
                max_gas,
                params,
            } => {
                writer.write_u8(1);
                chunk_id.write(writer);
                max_gas.write(writer);
                params.write(writer);
            }
        }
    }

    fn size(&self) -> usize {
        1 + match self {
            ScheduledExecutionKindLog::TopoHeight { topoheight } => topoheight.size(),
            ScheduledExecutionKindLog::BlockEnd {
                chunk_id,
                max_gas,
                params,
            } => chunk_id.size() + max_gas.size() + params.size(),
        }
    }
}
