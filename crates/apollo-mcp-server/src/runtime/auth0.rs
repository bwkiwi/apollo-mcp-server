use schemars::JsonSchema;
use serde::Deserialize;

/// Auth0 configuration for outbound GraphQL authentication
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Auth0Config {
    /// Auth0 domain (e.g., "your-tenant.eu.auth0.com")
    pub domain: String,

    /// Auth0 client ID
    pub client_id: String,

    /// Auth0 audience (e.g., "https://api.example.com/graphql")
    pub audience: String,

    /// Auth0 refresh token for obtaining access tokens
    pub refresh_token: String,
}
