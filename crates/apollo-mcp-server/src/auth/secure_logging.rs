/// Utilities for secure logging of sensitive data
use std::fmt;

/// Truncates sensitive string values to show only the first 6 characters for logging
pub fn truncate_sensitive(value: &str) -> String {
    if value.is_empty() {
        return "***".to_string();
    }
    
    if value.len() <= 6 {
        // If the value is 6 chars or less, show asterisks instead
        "*".repeat(value.len())
    } else {
        format!("{}***", &value[..6])
    }
}

/// A wrapper for sensitive strings that implements Display with truncation
pub struct SensitiveString<'a>(pub &'a str);

impl<'a> fmt::Display for SensitiveString<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", truncate_sensitive(self.0))
    }
}

/// Helper macro for logging sensitive data safely
#[macro_export]
macro_rules! log_sensitive {
    ($level:ident, $($arg:tt)*) => {
        log::$level!($($arg)*)
    };
}

/// Helper function to create a SensitiveString for logging
pub fn sensitive(value: &str) -> SensitiveString<'_> {
    SensitiveString(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_sensitive() {
        // Empty string
        assert_eq!(truncate_sensitive(""), "***");
        
        // Short strings (6 chars or less)
        assert_eq!(truncate_sensitive("abc"), "***");
        assert_eq!(truncate_sensitive("123456"), "******");
        
        // Longer strings
        assert_eq!(truncate_sensitive("1234567890"), "123456***");
        assert_eq!(truncate_sensitive("abcdefghijklmnop"), "abcdef***");
        
        // Typical JWT token
        let jwt = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiYWRtaW4iOnRydWV9.EkN-DOsnsuRjRO6BxXemmJDm3HbxrbRzXglbN2S4sOkopdU4IsDxTI8jO19W_A4K8ZPJijNLis4EZsHeY559a4DFOd50_OqgHs_z2inWfFdZt6Z9Z7UTS8BRh_KE-_L5C5hg";
        assert_eq!(truncate_sensitive(jwt), "eyJhbG***");
        
        // Auth0 client ID
        let client_id = "9iFHpiJqhQl6KCel7Qe1OlbvWllz2xJj";
        assert_eq!(truncate_sensitive(client_id), "9iFHpi***");
        
        // Device code
        let device_code = "ABCD-EFGH-IJKL-MNOP";
        assert_eq!(truncate_sensitive(device_code), "ABCD-E***");
    }
    
    #[test]
    fn test_sensitive_string_display() {
        let token = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9";
        let sensitive_token = sensitive(token);
        assert_eq!(format!("{}", sensitive_token), "eyJhbG***");
    }
}