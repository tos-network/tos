// Bootstrap sync types

use tos_common::{block::TopoHeight, contract::ScheduledExecution, serializer::*};

/// Metadata for scheduled execution during bootstrap sync.
/// Contains the execution data along with timing information.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScheduledExecutionMetadata {
    /// The scheduled execution data
    pub execution: ScheduledExecution,
    /// The topoheight when the execution is planned to run
    pub execution_topoheight: TopoHeight,
    /// The topoheight when this execution was registered
    pub registration_topoheight: TopoHeight,
}

impl Serializer for ScheduledExecutionMetadata {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let execution = ScheduledExecution::read(reader)?;
        let execution_topoheight = TopoHeight::read(reader)?;
        let registration_topoheight = TopoHeight::read(reader)?;

        Ok(Self {
            execution,
            execution_topoheight,
            registration_topoheight,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.execution.write(writer);
        self.execution_topoheight.write(writer);
        self.registration_topoheight.write(writer);
    }

    fn size(&self) -> usize {
        self.execution.size()
            + self.execution_topoheight.size()
            + self.registration_topoheight.size()
    }
}
