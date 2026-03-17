//! Role-based routing configuration

use schemars::JsonSchema;
use serde::Deserialize;
use url::Url;

/// Configuration for role-based GraphQL endpoint routing
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RoleConfig {
    /// Base URL for the GraphQL backend (e.g., "https://graphql-backend")
    #[schemars(schema_with = "Url::json_schema")]
    pub graphql_base_url: Url,

    /// List of available roles (e.g., ["reader", "creator", "approver", "admin"])
    pub available_roles: Vec<String>,

    /// Default role to use when no role is specified in the path (e.g., "reader")
    pub default_role: String,
}
