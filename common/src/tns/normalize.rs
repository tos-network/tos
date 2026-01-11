// TNS Name Normalization

use thiserror::Error;

/// Name normalization errors
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum NormalizeError {
    #[error("Name contains leading or trailing whitespace")]
    HasWhitespace,

    #[error("Name contains non-ASCII character: {0}")]
    NonAsciiCharacter(char),
}

/// Normalize a TNS name
/// 1. Reject names with leading/trailing whitespace (don't trim, reject)
/// 2. Reject non-ASCII characters (prevent Unicode homoglyph attacks)
/// 3. Convert to lowercase
pub fn normalize_name(name: &str) -> Result<String, NormalizeError> {
    // 1. Check for leading/trailing whitespace (reject, don't trim)
    if name != name.trim() {
        return Err(NormalizeError::HasWhitespace);
    }

    // 2. Must be pure ASCII (prevent Cyrillic 'а' vs ASCII 'a' attacks)
    if !name.is_ascii() {
        if let Some(bad_char) = name.chars().find(|c| !c.is_ascii()) {
            return Err(NormalizeError::NonAsciiCharacter(bad_char));
        }
    }

    // 3. Convert to lowercase
    Ok(name.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_valid_names() {
        assert_eq!(normalize_name("alice").unwrap(), "alice");
        assert_eq!(normalize_name("Alice").unwrap(), "alice");
        assert_eq!(normalize_name("ALICE").unwrap(), "alice");
        assert_eq!(normalize_name("Bob123").unwrap(), "bob123");
        assert_eq!(normalize_name("john.doe").unwrap(), "john.doe");
    }

    #[test]
    fn test_reject_whitespace() {
        assert!(matches!(
            normalize_name(" alice"),
            Err(NormalizeError::HasWhitespace)
        ));
        assert!(matches!(
            normalize_name("alice "),
            Err(NormalizeError::HasWhitespace)
        ));
        assert!(matches!(
            normalize_name(" alice "),
            Err(NormalizeError::HasWhitespace)
        ));
    }

    #[test]
    fn test_reject_non_ascii() {
        // Cyrillic 'а' (U+0430) looks like ASCII 'a'
        assert!(matches!(
            normalize_name("аlice"), // Cyrillic 'а'
            Err(NormalizeError::NonAsciiCharacter(_))
        ));

        // Japanese characters
        assert!(matches!(
            normalize_name("日本語"),
            Err(NormalizeError::NonAsciiCharacter(_))
        ));

        // Emoji
        assert!(matches!(
            normalize_name("alice😀"),
            Err(NormalizeError::NonAsciiCharacter(_))
        ));
    }
}
