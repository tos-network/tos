// TNS Name Format Validation
//
// This module provides name format validation that can be used by both
// RPC endpoints and wallet commands without depending on VerificationError.

use super::{
    is_confusing_name, is_reserved_name, normalize_name, MAX_NAME_LENGTH, MIN_NAME_LENGTH,
};

/// Result of name format validation
#[derive(Debug, Clone)]
pub struct NameValidationResult {
    /// Whether the name format is valid
    pub valid: bool,
    /// Error message if invalid
    pub error: Option<String>,
    /// Normalized name (lowercase) if valid
    pub normalized: Option<String>,
}

impl NameValidationResult {
    fn valid(normalized: String) -> Self {
        Self {
            valid: true,
            error: None,
            normalized: Some(normalized),
        }
    }

    fn invalid(error: impl Into<String>) -> Self {
        Self {
            valid: false,
            error: Some(error.into()),
            normalized: None,
        }
    }
}

/// Validate TNS name format according to RFC 5321 dot-atom aligned rules:
/// - Length: 3-64 characters
/// - Must start with a letter
/// - Cannot end with separator (. - _)
/// - Only lowercase letters, digits, and separators allowed
/// - No consecutive separators
/// - Not a reserved name
/// - Not a confusing name (phishing protection)
///
/// Returns a NameValidationResult containing validation status and normalized name if valid.
pub fn validate_name_format(name: &str) -> NameValidationResult {
    // 0. Normalize first (reject spaces, non-ASCII, convert to lowercase)
    let normalized = match normalize_name(name) {
        Ok(n) => n,
        Err(e) => {
            return NameValidationResult::invalid(format!("Invalid character: {:?}", e));
        }
    };

    // 1. Length check: 3-64 characters
    if normalized.len() < MIN_NAME_LENGTH {
        return NameValidationResult::invalid(format!(
            "Name too short (min {} characters)",
            MIN_NAME_LENGTH
        ));
    }

    if normalized.len() > MAX_NAME_LENGTH {
        return NameValidationResult::invalid(format!(
            "Name too long (max {} characters)",
            MAX_NAME_LENGTH
        ));
    }

    // 2. Must start with letter
    let first_char = match normalized.chars().next() {
        Some(c) => c,
        None => return NameValidationResult::invalid("Name cannot be empty"),
    };

    if !first_char.is_ascii_lowercase() {
        return NameValidationResult::invalid("Name must start with a letter (a-z)");
    }

    // 3. Cannot end with separator
    if let Some(last) = normalized.chars().last() {
        if matches!(last, '.' | '-' | '_') {
            return NameValidationResult::invalid("Name cannot end with separator (. - _)");
        }
    }

    // 4. Character set & consecutive separator check
    let mut prev_is_separator = false;
    for c in normalized.chars() {
        let is_separator = matches!(c, '.' | '-' | '_');

        // Only allow a-z, 0-9, ., -, _
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && !is_separator {
            return NameValidationResult::invalid(format!(
                "Invalid character '{}'. Only a-z, 0-9, ., -, _ allowed",
                c
            ));
        }

        // No consecutive separators
        if is_separator && prev_is_separator {
            return NameValidationResult::invalid(
                "Consecutive separators not allowed (e.g., .., --, __)",
            );
        }
        prev_is_separator = is_separator;
    }

    // 5. Reserved name check
    if is_reserved_name(&normalized) {
        return NameValidationResult::invalid(format!("'{}' is a reserved name", normalized));
    }

    // 6. Confusing name check (phishing protection)
    if is_confusing_name(&normalized) {
        return NameValidationResult::invalid(format!(
            "'{}' is considered confusing (phishing protection)",
            normalized
        ));
    }

    NameValidationResult::valid(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_names() {
        assert!(validate_name_format("alice").valid);
        assert!(validate_name_format("bob123").valid);
        assert!(validate_name_format("john.doe").valid);
        assert!(validate_name_format("alice-wang").valid);
        assert!(validate_name_format("user_name").valid);
    }

    #[test]
    fn test_invalid_start() {
        assert!(!validate_name_format("123abc").valid);
        assert!(!validate_name_format("_alice").valid);
        assert!(!validate_name_format(".bob").valid);
    }

    #[test]
    fn test_invalid_end() {
        assert!(!validate_name_format("alice.").valid);
        assert!(!validate_name_format("bob-").valid);
        assert!(!validate_name_format("charlie_").valid);
    }

    #[test]
    fn test_consecutive_separators() {
        assert!(!validate_name_format("alice..bob").valid);
        assert!(!validate_name_format("alice--bob").valid);
        assert!(!validate_name_format("alice.-bob").valid);
    }

    #[test]
    fn test_length_limits() {
        assert!(!validate_name_format("ab").valid); // Too short
        assert!(validate_name_format("abc").valid); // Min length
        let long_name = "a".repeat(64);
        assert!(validate_name_format(&long_name).valid); // Max length
        let too_long = "a".repeat(65);
        assert!(!validate_name_format(&too_long).valid); // Too long
    }
}
