use schemars::JsonSchema;
use serde::Deserialize;

/// Configuration for Auth0 outbound authentication
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Auth0Config {
    /// Auth0 domain (e.g., "your-tenant.auth0.com")
    pub domain: String,
    
    /// Auth0 client ID (used for Phase 1 shared auth)
    pub client_id: String,
    
    /// Auth0 audience for the GraphQL API
    pub audience: String,
    
    /// Refresh token for obtaining access tokens (Phase 1 only)
    pub refresh_token: Option<String>,
    
    /// Per-session authentication configuration (Phase 2)
    pub per_session_auth: Option<PerSessionAuthConfig>,
}

/// Configuration for per-session user authentication (Phase 2)
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PerSessionAuthConfig {
    /// Enable per-session authentication
    pub enabled: bool,
    
    /// Auth0 client ID for device flow (should be a Native Application)
    pub device_flow_client_id: String,
    
    /// Session storage type
    pub session_storage: SessionStorageType,
    
    /// Token refresh buffer in seconds (default: 300 = 5 minutes)
    pub token_refresh_buffer_seconds: Option<u64>,
    
    /// Device flow polling interval in seconds (default: 5)
    pub device_flow_poll_interval_seconds: Option<u64>,
    
    /// Device flow timeout in seconds (default: 600 = 10 minutes)
    pub device_flow_timeout_seconds: Option<u64>,
}

/// Session storage configuration
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionStorageType {
    /// In-memory storage (sessions lost on restart)
    Memory,
    
    /// File-based storage
    File {
        /// Path to store session data
        path: String,
    },
}

impl Default for PerSessionAuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device_flow_client_id: String::new(),
            session_storage: SessionStorageType::Memory,
            token_refresh_buffer_seconds: Some(300),
            device_flow_poll_interval_seconds: Some(5),
            device_flow_timeout_seconds: Some(600),
        }
    }
}

impl Auth0Config {
    /// Check if Phase 2 (per-session auth) is enabled
    pub fn is_per_session_enabled(&self) -> bool {
        self.per_session_auth
            .as_ref()
            .map_or(false, |config| config.enabled)
    }
    
    /// Get the device flow client ID if Phase 2 is enabled
    pub fn device_flow_client_id(&self) -> Option<&str> {
        self.per_session_auth
            .as_ref()
            .filter(|config| config.enabled)
            .map(|config| config.device_flow_client_id.as_str())
    }
    
    /// Get token refresh buffer duration
    pub fn token_refresh_buffer(&self) -> std::time::Duration {
        let seconds = self.per_session_auth
            .as_ref()
            .and_then(|config| config.token_refresh_buffer_seconds)
            .unwrap_or(300);
        std::time::Duration::from_secs(seconds)
    }
    
    /// Get device flow poll interval
    pub fn device_flow_poll_interval(&self) -> std::time::Duration {
        let seconds = self.per_session_auth
            .as_ref()
            .and_then(|config| config.device_flow_poll_interval_seconds)
            .unwrap_or(5);
        std::time::Duration::from_secs(seconds)
    }
    
    /// Get device flow timeout
    pub fn device_flow_timeout(&self) -> std::time::Duration {
        let seconds = self.per_session_auth
            .as_ref()
            .and_then(|config| config.device_flow_timeout_seconds)
            .unwrap_or(600);
        std::time::Duration::from_secs(seconds)
    }
}