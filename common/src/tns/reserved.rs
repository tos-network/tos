// TNS Reserved Names and Phishing Detection

/// Reserved names that cannot be registered
pub const RESERVED_NAMES: &[&str] = &[
    // Original reserved names
    "admin",
    "administrator",
    "system",
    "root",
    "null",
    "undefined",
    "tos",
    "tosnetwork",
    "test",
    "example",
    "localhost",
    "postmaster",
    "webmaster",
    "hostmaster",
    "abuse",
    "support",
    "info",
    "contact",
    // Protocol related
    "validator",
    "node",
    "daemon",
    "rpc",
    "api",
    "wallet",
    "bridge",
    "oracle",
    "governance",
    "treasury",
    "foundation",
    "network",
    "mainnet",
    "testnet",
    "devnet",
    "stagenet",
    "block",
    "transaction",
    "tx",
    "hash",
    "address",
    // Security related
    "security",
    "cert",
    "ssl",
    "tls",
    "www",
    "ftp",
    "mail",
    "smtp",
    "imap",
    "pop",
    "dns",
    "ntp",
    "ssh",
    "telnet",
    "ldap",
    // User confusion risk
    "official",
    "verified",
    "authentic",
    "real",
    "true",
    "team",
    "staff",
    "mod",
    "moderator",
    "developer",
    "dev",
    // Special strings
    "anonymous",
    "unknown",
    "nobody",
    "anyone",
    "everyone",
    "all",
    "none",
    "default",
    "guest",
    "user",
];

/// Check if a name is reserved
pub fn is_reserved_name(name: &str) -> bool {
    RESERVED_NAMES.contains(&name)
}

/// Phishing keywords that indicate a confusing name
const PHISHING_KEYWORDS: &[&str] = &["official", "verified", "authentic", "support", "help"];

/// Check if a name is potentially confusing (phishing risk)
pub fn is_confusing_name(name: &str) -> bool {
    // 1. Looks like an address prefix
    if name.starts_with("tos1") || name.starts_with("tst1") || name.starts_with("0x") {
        return true;
    }

    // 2. Pure numeric (easily confused with ID)
    if name.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    // 3. Contains phishing keywords
    for keyword in PHISHING_KEYWORDS {
        if name.contains(keyword) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reserved_names() {
        assert!(is_reserved_name("admin"));
        assert!(is_reserved_name("system"));
        assert!(is_reserved_name("validator"));
        assert!(!is_reserved_name("alice"));
        assert!(!is_reserved_name("bob123"));
    }

    #[test]
    fn test_confusing_names() {
        // Address prefixes
        assert!(is_confusing_name("tos1abc"));
        assert!(is_confusing_name("tst1xyz"));
        assert!(is_confusing_name("0x1234"));

        // Pure numeric
        assert!(is_confusing_name("123456"));
        assert!(is_confusing_name("000"));

        // Phishing keywords
        assert!(is_confusing_name("alice_official"));
        assert!(is_confusing_name("verified_bob"));
        assert!(is_confusing_name("support_team"));

        // Valid names
        assert!(!is_confusing_name("alice"));
        assert!(!is_confusing_name("bob123"));
        assert!(!is_confusing_name("john.doe"));
    }
}
