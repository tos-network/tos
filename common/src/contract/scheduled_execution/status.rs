// ScheduledExecutionStatus - tracks the lifecycle of a scheduled execution

use serde::{Deserialize, Serialize};

use crate::serializer::*;

/// Status of a scheduled execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledExecutionStatus {
    /// Pending execution - waiting for target topoheight
    #[default]
    Pending,
    /// Successfully executed
    Executed,
    /// Cancelled by the scheduler contract
    Cancelled,
    /// Execution failed (contract error, out of gas, etc.)
    Failed,
    /// Expired - exceeded MAX_DEFER_COUNT without execution
    Expired,
}

impl ScheduledExecutionStatus {
    /// Get the ID for serialization
    pub fn id(&self) -> u8 {
        match self {
            ScheduledExecutionStatus::Pending => 0,
            ScheduledExecutionStatus::Executed => 1,
            ScheduledExecutionStatus::Cancelled => 2,
            ScheduledExecutionStatus::Failed => 3,
            ScheduledExecutionStatus::Expired => 4,
        }
    }

    /// Check if this status represents a terminal state
    pub fn is_terminal(&self) -> bool {
        !matches!(self, ScheduledExecutionStatus::Pending)
    }

    /// Check if this execution was successful
    pub fn is_success(&self) -> bool {
        matches!(self, ScheduledExecutionStatus::Executed)
    }
}

impl Serializer for ScheduledExecutionStatus {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let tag = reader.read_u8()?;
        match tag {
            0 => Ok(ScheduledExecutionStatus::Pending),
            1 => Ok(ScheduledExecutionStatus::Executed),
            2 => Ok(ScheduledExecutionStatus::Cancelled),
            3 => Ok(ScheduledExecutionStatus::Failed),
            4 => Ok(ScheduledExecutionStatus::Expired),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn write(&self, writer: &mut Writer) {
        writer.write_u8(self.id());
    }

    fn size(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_default() {
        let status = ScheduledExecutionStatus::default();
        assert_eq!(status, ScheduledExecutionStatus::Pending);
    }

    #[test]
    fn test_status_is_terminal() {
        assert!(!ScheduledExecutionStatus::Pending.is_terminal());
        assert!(ScheduledExecutionStatus::Executed.is_terminal());
        assert!(ScheduledExecutionStatus::Cancelled.is_terminal());
        assert!(ScheduledExecutionStatus::Failed.is_terminal());
        assert!(ScheduledExecutionStatus::Expired.is_terminal());
    }

    #[test]
    fn test_status_serialization() {
        for status in [
            ScheduledExecutionStatus::Pending,
            ScheduledExecutionStatus::Executed,
            ScheduledExecutionStatus::Cancelled,
            ScheduledExecutionStatus::Failed,
            ScheduledExecutionStatus::Expired,
        ] {
            let mut bytes = Vec::new();
            {
                let mut writer = Writer::new(&mut bytes);
                status.write(&mut writer);
            }

            let mut reader = Reader::new(&bytes);
            let decoded = ScheduledExecutionStatus::read(&mut reader).unwrap();
            assert_eq!(status, decoded);
        }
    }
}
