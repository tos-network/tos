/// Error types for TAKO VM execution
///
/// Provides detailed, structured error types for better debugging and user feedback.
/// Each error variant includes context and actionable information.
use thiserror::Error;
use tos_tbpf::error::EbpfError;

/// Main error type for TAKO VM execution
#[derive(Error, Debug)]
pub enum TakoExecutionError {
    /// Compute budget validation failed
    #[error("Compute budget {requested} exceeds maximum allowed {maximum}")]
    ComputeBudgetExceeded { requested: u64, maximum: u64 },

    /// Invalid ELF bytecode
    #[error("Invalid contract bytecode: {reason}")]
    InvalidBytecode {
        reason: String,
        #[source]
        source: Option<anyhow::Error>,
    },

    /// Syscall registration failed
    #[error("Failed to register syscalls: {reason}")]
    SyscallRegistrationFailed {
        reason: String,
        error_details: String,
    },

    /// Executable loading failed
    #[error("Failed to load executable: {reason}")]
    ExecutableLoadFailed {
        reason: String,
        bytecode_size: usize,
        error_details: String,
    },

    /// Memory mapping creation failed
    #[error("Failed to create memory mapping: {reason}")]
    MemoryMappingFailed {
        reason: String,
        stack_size: usize,
        error_details: String,
    },

    /// Contract execution failed
    #[error("Contract execution failed at instruction {instruction_count}: {reason}")]
    ExecutionFailed {
        reason: String,
        instruction_count: u64,
        compute_units_used: u64,
        error_code: Option<u64>,
    },

    /// Out of compute units during execution
    #[error("Out of compute units: used {used} of {budget} available")]
    OutOfComputeUnits {
        used: u64,
        budget: u64,
        instruction_count: u64,
    },

    /// Memory access violation
    #[error("Memory access violation: {reason}")]
    MemoryAccessViolation {
        reason: String,
        address: Option<u64>,
        size: Option<usize>,
    },

    /// Stack overflow during execution
    #[error("Stack overflow: depth {depth} exceeds maximum {max_depth}")]
    StackOverflow { depth: usize, max_depth: usize },

    /// Invalid instruction encountered
    #[error("Invalid instruction at PC {program_counter}: opcode {opcode:#x}")]
    InvalidInstruction { program_counter: u64, opcode: u8 },

    /// CPI (Cross-Program Invocation) failed
    #[error("CPI invocation failed: {reason}")]
    CpiInvocationFailed {
        reason: String,
        callee_address: Option<String>,
        #[source]
        source: Option<Box<TakoExecutionError>>,
    },
}

impl TakoExecutionError {
    /// Create an InvalidBytecode error from validation error
    pub fn invalid_bytecode(reason: impl Into<String>, source: Option<anyhow::Error>) -> Self {
        Self::InvalidBytecode {
            reason: reason.into(),
            source,
        }
    }

    /// Create an ExecutionFailed error from EbpfError
    pub fn from_ebpf_error(
        err: EbpfError,
        instruction_count: u64,
        compute_units_used: u64,
    ) -> Self {
        // Parse the error and extract meaningful information
        let reason = format!("{:?}", err);

        // Check if it's an out-of-compute error
        if reason.contains("ExceededMaxInstructions") || reason.contains("compute") {
            return Self::OutOfComputeUnits {
                used: compute_units_used,
                budget: compute_units_used, // We've hit the limit
                instruction_count,
            };
        }

        // Check if it's a memory error
        if reason.contains("memory") || reason.contains("Memory") {
            return Self::MemoryAccessViolation {
                reason: reason.clone(),
                address: None,
                size: None,
            };
        }

        // Check if it's a stack overflow
        if reason.contains("CallDepthExceeded") || reason.contains("stack") {
            return Self::StackOverflow {
                depth: 0,      // Would need to extract from error
                max_depth: 64, // Default max depth
            };
        }

        // Generic execution failure
        Self::ExecutionFailed {
            reason,
            instruction_count,
            compute_units_used,
            error_code: None,
        }
    }

    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            Self::ComputeBudgetExceeded { requested, maximum } => {
                format!(
                    "Transaction requires {} compute units but maximum allowed is {}. \
                     Consider optimizing your contract or requesting a higher compute budget.",
                    requested, maximum
                )
            }
            Self::InvalidBytecode { reason, .. } => {
                format!(
                    "Invalid contract bytecode: {}. \
                     Ensure your contract is compiled with the TOS toolchain.",
                    reason
                )
            }
            Self::ExecutionFailed {
                reason,
                instruction_count,
                compute_units_used,
                ..
            } => {
                format!(
                    "Contract execution failed after {} instructions ({} compute units): {}",
                    instruction_count, compute_units_used, reason
                )
            }
            Self::OutOfComputeUnits {
                used,
                budget,
                instruction_count,
            } => {
                format!(
                    "Contract ran out of compute units. Used {} of {} available after {} instructions. \
                     Consider optimizing your contract or requesting more compute units.",
                    used, budget, instruction_count
                )
            }
            Self::MemoryAccessViolation { reason, .. } => {
                format!(
                    "Invalid memory access: {}. \
                     This usually indicates a bug in the contract code.",
                    reason
                )
            }
            Self::StackOverflow { depth, max_depth } => {
                format!(
                    "Stack overflow: call depth {} exceeds maximum {}. \
                     Reduce the depth of nested function calls.",
                    depth, max_depth
                )
            }
            Self::CpiInvocationFailed {
                reason,
                callee_address,
                ..
            } => {
                if let Some(addr) = callee_address {
                    format!("Failed to invoke contract at {}: {}", addr, reason)
                } else {
                    format!("Cross-program invocation failed: {}", reason)
                }
            }
            _ => self.to_string(),
        }
    }

    /// Get error category for metrics/logging
    pub fn category(&self) -> &'static str {
        match self {
            Self::ComputeBudgetExceeded { .. } => "validation",
            Self::InvalidBytecode { .. } => "validation",
            Self::SyscallRegistrationFailed { .. } => "initialization",
            Self::ExecutableLoadFailed { .. } => "initialization",
            Self::MemoryMappingFailed { .. } => "initialization",
            Self::ExecutionFailed { .. } => "execution",
            Self::OutOfComputeUnits { .. } => "execution",
            Self::MemoryAccessViolation { .. } => "execution",
            Self::StackOverflow { .. } => "execution",
            Self::InvalidInstruction { .. } => "execution",
            Self::CpiInvocationFailed { .. } => "execution",
        }
    }

    /// Check if this is a recoverable error
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ComputeBudgetExceeded { .. } | Self::OutOfComputeUnits { .. }
        )
    }
}

// Convert from anyhow::Error for compatibility
impl From<anyhow::Error> for TakoExecutionError {
    fn from(err: anyhow::Error) -> Self {
        Self::ExecutionFailed {
            reason: err.to_string(),
            instruction_count: 0,
            compute_units_used: 0,
            error_code: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_budget_exceeded_message() {
        let err = TakoExecutionError::ComputeBudgetExceeded {
            requested: 20_000_000,
            maximum: 10_000_000,
        };
        let msg = err.user_message();
        assert!(msg.contains("20000000"));
        assert!(msg.contains("10000000"));
        assert!(msg.contains("compute units"));
    }

    #[test]
    fn test_error_category() {
        let err = TakoExecutionError::InvalidBytecode {
            reason: "test".to_string(),
            source: None,
        };
        assert_eq!(err.category(), "validation");

        let err = TakoExecutionError::ExecutionFailed {
            reason: "test".to_string(),
            instruction_count: 100,
            compute_units_used: 50,
            error_code: None,
        };
        assert_eq!(err.category(), "execution");
    }

    #[test]
    fn test_recoverable_errors() {
        let err = TakoExecutionError::ComputeBudgetExceeded {
            requested: 20_000_000,
            maximum: 10_000_000,
        };
        assert!(err.is_recoverable());

        let err = TakoExecutionError::InvalidBytecode {
            reason: "test".to_string(),
            source: None,
        };
        assert!(!err.is_recoverable());
    }
}
