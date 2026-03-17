//! Role-based routing utilities for multi-schema support

use url::Url;

/// Extract role from HTTP request path
///
/// Expected path formats:
/// - `/mcp/approver` → `Some("approver")`
/// - `/mcp/admin` → `Some("admin")`
/// - `/mcp` → `None` (use default)
/// - `/` → `None` (use default)
pub fn extract_role_from_path(path: &str) -> Option<String> {
    path.strip_prefix("/mcp/")
        .and_then(|s| s.split('/').next())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Get the role to use, falling back to default if none specified
pub fn get_role(path: &str, default_role: &str) -> String {
    extract_role_from_path(path).unwrap_or_else(|| default_role.to_string())
}

/// Build GraphQL endpoint URL for a specific role
pub fn build_endpoint_for_role(base_url: &Url, role: &str) -> Url {
    let endpoint_str = format!("{}/graphql/{}", base_url, role);
    Url::parse(&endpoint_str).expect("valid URL from base and role")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_role_from_path() {
        assert_eq!(extract_role_from_path("/mcp/approver"), Some("approver".to_string()));
        assert_eq!(extract_role_from_path("/mcp/admin"), Some("admin".to_string()));
        assert_eq!(extract_role_from_path("/mcp/reader"), Some("reader".to_string()));
        assert_eq!(extract_role_from_path("/mcp/creator"), Some("creator".to_string()));
        assert_eq!(extract_role_from_path("/mcp"), None);
        assert_eq!(extract_role_from_path("/"), None);
        assert_eq!(extract_role_from_path(""), None);
        assert_eq!(extract_role_from_path("/mcp/approver/extra/path"), Some("approver".to_string()));
    }

    #[test]
    fn test_get_role() {
        assert_eq!(get_role("/mcp/approver", "reader"), "approver");
        assert_eq!(get_role("/mcp/admin", "reader"), "admin");
        assert_eq!(get_role("/mcp", "reader"), "reader");
        assert_eq!(get_role("/", "reader"), "reader");
    }

    #[test]
    fn test_build_endpoint_for_role() {
        let base = Url::parse("https://api.example.com").unwrap();
        assert_eq!(
            build_endpoint_for_role(&base, "approver").as_str(),
            "https://api.example.com/graphql/approver"
        );
        assert_eq!(
            build_endpoint_for_role(&base, "admin").as_str(),
            "https://api.example.com/graphql/admin"
        );
    }
}