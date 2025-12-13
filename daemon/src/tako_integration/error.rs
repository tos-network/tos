/// Error types for TOS Kernel(TAKO) execution
///
/// Provides detailed, structured error types for better debugging and user feedback.
/// Each error variant includes context and actionable information.
use thiserror::Error;
use tos_tbpf::error::EbpfError;

/// Main error type for TOS Kernel(TAKO) execution
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

    /// Loaded contract data size limit exceeded
    ///
    /// This error occurs when the cumulative size of loaded data during execution
    /// exceeds the configured limit (default: 64 MB). This includes:
    /// - Entry contract bytecode
    /// - CPI contract bytecode
    /// - Storage read values
    /// - CPI input/return data
    #[error("Loaded contract data size {current_size} bytes exceeds limit {limit} bytes (operation: {operation})")]
    LoadedDataLimitExceeded {
        /// Current cumulative loaded data size
        current_size: u64,
        /// Maximum allowed size (from ComputeBudgetLimits)
        limit: u64,
        /// Operation that triggered the limit breach (e.g., "entry_contract_load", "storage_read", "cpi_call")
        operation: String,
        /// Additional details about what caused the limit to be exceeded
        details: String,
    },

    /// Precompile verification failed
    ///
    /// This error occurs when a precompile (signature verification program) fails to verify.
    /// Precompiles are special programs for Ed25519, secp256k1, and secp256r1 signature verification.
    #[error("Precompile verification failed for program {program_id}: {error_details}")]
    PrecompileVerificationFailed {
        /// Precompile program ID (hex encoded)
        program_id: String,
        /// Error details from the verification process
        error_details: String,
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

        // Check if it's a loaded data limit exceeded error
        // This error can come from syscalls (storage reads, CPI calls, return data)
        if reason.contains("LoadedDataLimitExceeded")
            || reason.contains("Loaded contract data size")
        {
            // Helper function to extract first number after a keyword
            let extract_number = |text: &str, keyword: &str| -> Option<u64> {
                text.split(keyword)
                    .nth(1)?
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit())
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .ok()
            };

            // Try to extract the actual values from the error message
            // Format 1: "Loaded contract data size {new_total} exceeds limit {max_size}"
            // Format 2: "LoadedDataLimitExceeded { attempted: ..., current: ..., limit: ... }"

            // Parse the limit value
            let limit = extract_number(&reason, "limit")
                .or_else(|| extract_number(&reason, "limit:"))
                .unwrap_or(64 * 1024 * 1024); // Default 64 MB

            // Parse the current size
            let current_size = extract_number(&reason, "current")
                .or_else(|| extract_number(&reason, "current:"))
                .or_else(|| extract_number(&reason, "data size"))
                .unwrap_or(0);

            // Determine the operation type from the error message
            let operation = if reason.contains("storage") {
                "storage_read"
            } else if reason.contains("CPI") || reason.contains("call") {
                "cpi_call"
            } else if reason.contains("return") {
                "return_data"
            } else {
                "unknown_operation"
            }
            .to_string();

            return Self::LoadedDataLimitExceeded {
                current_size,
                limit,
                operation,
                details: reason.clone(),
            };
        }

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

        // Check if it's a call depth exceeded (true stack overflow from nested calls)
        // Only match CallDepthExceeded, NOT StackAccessViolation (which is a memory error)
        if reason.contains("CallDepthExceeded") {
            return Self::StackOverflow {
                depth: 0,      // Would need to extract from error
                max_depth: 64, // Default max depth
            };
        }

        // Check if it's a stack access violation (memory error in stack region)
        // This is different from call depth exceeded - it's a memory access error
        if reason.contains("StackAccessViolation") {
            return Self::MemoryAccessViolation {
                reason: reason.clone(),
                address: None,
                size: None,
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
            Self::LoadedDataLimitExceeded { .. } => "resource_limit",
            Self::PrecompileVerificationFailed { .. } => "precompile",
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

    #[test]
    fn test_from_ebpf_error_loaded_data_limit() {
        // Test mapping from EbpfError::SyscallError containing LoadedDataLimitExceeded
        // Simulate the error format from invoke_context.rs
        let error_msg = "Loaded contract data size 70000000 exceeds limit 67108864 (tried to add 5000000 bytes)";
        let ebpf_err = EbpfError::SyscallError(error_msg.into());

        let result = TakoExecutionError::from_ebpf_error(ebpf_err, 1000, 500);

        match result {
            TakoExecutionError::LoadedDataLimitExceeded {
                current_size,
                limit,
                operation,
                details,
            } => {
                // Verify the error was parsed correctly
                assert_eq!(limit, 67108864, "Limit should be 64 MB");
                assert_eq!(current_size, 70000000, "Current size should be parsed");
                assert!(!operation.is_empty(), "Operation should be determined");
                assert!(
                    details.contains("exceeds limit"),
                    "Details should contain error message"
                );
            }
            _ => panic!("Expected LoadedDataLimitExceeded error, got: {:?}", result),
        }
    }

    #[test]
    fn test_from_ebpf_error_storage_read_limit() {
        // Test error from storage read syscall (Debug format with field names)
        let error_msg =
            "LoadedDataLimitExceeded { attempted: 10000, current: 67000000, limit: 67108864 }";
        let ebpf_err = EbpfError::SyscallError(error_msg.into());

        let result = TakoExecutionError::from_ebpf_error(ebpf_err, 500, 250);

        match result {
            TakoExecutionError::LoadedDataLimitExceeded {
                current_size,
                limit,
                operation,
                ..
            } => {
                // CRITICAL: Verify actual values are extracted, not defaults
                assert_eq!(
                    limit, 67108864,
                    "Limit should be parsed from 'limit: 67108864'"
                );
                assert_eq!(
                    current_size, 67000000,
                    "Current should be parsed from 'current: 67000000'"
                );
                assert!(!operation.is_empty(), "Operation should be determined");
            }
            _ => panic!("Expected LoadedDataLimitExceeded error, got: {:?}", result),
        }
    }

    #[test]
    fn test_from_ebpf_error_custom_limit() {
        // Test with a non-default limit (128 MB) to ensure we're not falling back to defaults
        let error_msg =
            "LoadedDataLimitExceeded { attempted: 5000000, current: 130000000, limit: 134217728 }";
        let ebpf_err = EbpfError::SyscallError(error_msg.into());

        let result = TakoExecutionError::from_ebpf_error(ebpf_err, 1000, 500);

        match result {
            TakoExecutionError::LoadedDataLimitExceeded {
                current_size,
                limit,
                ..
            } => {
                // If parsing fails, these would be 0 and 64MB (defaults)
                assert_eq!(
                    limit, 134217728,
                    "Limit should be 128 MB, not default 64 MB"
                );
                assert_eq!(
                    current_size, 130000000,
                    "Current should be actual value, not 0"
                );
            }
            _ => panic!("Expected LoadedDataLimitExceeded error, got: {:?}", result),
        }
    }

    #[test]
    fn test_from_ebpf_error_other_errors() {
        // Test that other errors still work correctly
        let ebpf_err = EbpfError::ExceededMaxInstructions;
        let result = TakoExecutionError::from_ebpf_error(ebpf_err, 1000, 500);
        assert!(matches!(
            result,
            TakoExecutionError::OutOfComputeUnits { .. }
        ));

        // Test memory error
        let ebpf_err = EbpfError::SyscallError("Memory access violation at 0x1234".into());
        let result = TakoExecutionError::from_ebpf_error(ebpf_err, 100, 50);
        assert!(matches!(
            result,
            TakoExecutionError::MemoryAccessViolation { .. }
        ));
    }

    #[test]
    fn test_loaded_data_limit_category() {
        let err = TakoExecutionError::LoadedDataLimitExceeded {
            current_size: 70000000,
            limit: 67108864,
            operation: "storage_read".to_string(),
            details: "Test error".to_string(),
        };
        assert_eq!(err.category(), "resource_limit");
    }
}
