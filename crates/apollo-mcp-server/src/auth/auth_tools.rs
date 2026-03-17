use std::sync::Arc;

use rmcp::model::{CallToolResult, Content, ErrorCode, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::auth::{
    DeviceFlowManager, SessionManager, SessionManagerError, sensitive
};
use crate::errors::McpError;
use crate::schema_from_type;

pub type SessionId = String;

/// Input for authentication tools that don't require additional parameters
#[derive(JsonSchema, Deserialize)]
pub struct EmptyInput {}

/// Login tool for initiating Auth0 device flow authentication
#[derive(Debug, Clone)]
pub struct LoginTool {
    pub tool: Tool,
    device_flow_manager: Arc<DeviceFlowManager>,
    session_manager: Arc<SessionManager>,
}

impl LoginTool {
    pub fn new(
        device_flow_manager: Arc<DeviceFlowManager>,
        session_manager: Arc<SessionManager>,
    ) -> Self {
        Self {
            tool: Tool::new(
                "login",
                "Initiate Auth0 device flow authentication for this session. Returns instructions to complete authentication in your browser.",
                schema_from_type!(EmptyInput),
            ),
            device_flow_manager,
            session_manager,
        }
    }

    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        debug!("Starting login process for session {}", session_id);

        // Check if already authenticated
        match self.session_manager.is_authenticated(&session_id).await {
            Ok(true) => {
                return Ok(CallToolResult {
                    content: vec![Content::text(
                        "Already authenticated. Use 'logout' to sign out and login as a different user."
                    )],
                    is_error: Some(false),
                });
            }
            Ok(false) => {
                // Not authenticated, proceed with login
            }
            Err(e) => {
                warn!("Error checking authentication status: {}", e);
                // Continue with login attempt
            }
        }

        // Check if there's already an active device flow
        if self.device_flow_manager.has_active_flow(&session_id).await {
            if let Some(status) = self.device_flow_manager.get_flow_status(&session_id).await {
                if status.is_expired {
                    // Flow expired, cancel and start new one
                    self.device_flow_manager.cancel_flow(&session_id).await;
                } else {
                    // Return existing flow status
                    let content = format!(
                        "Login in progress. Please visit {} and enter code: {}\n\nOr visit: {}\n\nExpires in {} seconds.",
                        "https://your-domain.auth0.com/activate", // You might want to make this configurable
                        status.user_code,
                        status.verification_uri_complete,
                        status.expires_in_seconds
                    );

                    return Ok(CallToolResult {
                        content: vec![Content::text(content)],
                        is_error: Some(false),
                    });
                }
            }
        }

        // Initiate new device flow
        match self.device_flow_manager.initiate_device_flow(session_id.clone()).await {
            Ok(device_response) => {
                // Start background polling
                let session_manager = self.session_manager.clone();
                if let Err(e) = self.device_flow_manager
                    .start_device_flow_polling(session_id.clone(), session_manager)
                    .await 
                {
                    warn!("Failed to start device flow polling: {}", e);
                    return Err(McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to start authentication polling: {}", e),
                        None,
                    ));
                }

                info!("Device flow initiated for session {}: code={}", session_id, sensitive(&device_response.user_code));

                // Return instructions to user
                let response = json!({
                    "status": "login_initiated",
                    "message": format!(
                        "🔐 Authentication Required\n\nPlease visit:\n{}\n\nAnd enter code: {}\n\nOr click this direct link:\n{}\n\nThis code expires in {} seconds.\n\nWait for authentication to complete...",
                        device_response.verification_uri,
                        device_response.user_code,
                        device_response.verification_uri_complete,
                        device_response.expires_in
                    ),
                    "verification_uri": device_response.verification_uri,
                    "verification_uri_complete": device_response.verification_uri_complete,
                    "user_code": device_response.user_code,
                    "expires_in": device_response.expires_in,
                    "instructions": [
                        format!("1. Visit {}", device_response.verification_uri),
                        format!("2. Enter code: {}", device_response.user_code),
                        "3. Complete the authentication in your browser",
                        "4. Return here - authentication will complete automatically"
                    ]
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text(format!(
                            "Please visit {} and enter code: {}\n\nOr visit: {}",
                            device_response.verification_uri,
                            device_response.user_code,
                            device_response.verification_uri_complete
                        ))
                    })],
                    is_error: Some(false),
                })
            }
            Err(e) => {
                warn!("Device flow initiation failed for session {}: {}", session_id, e);
                Err(McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Authentication initiation failed: {}", e),
                    None,
                ))
            }
        }
    }
}

/// WhoAmI tool for displaying current session's user information
#[derive(Debug, Clone)]
pub struct WhoAmITool {
    pub tool: Tool,
    session_manager: Arc<SessionManager>,
}

impl WhoAmITool {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self {
            tool: Tool::new(
                "whoami",
                "Display information about the currently authenticated user in this session.",
                schema_from_type!(EmptyInput),
            ),
            session_manager,
        }
    }

    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        debug!("Getting user info for session {}", session_id);

        match self.session_manager.get_session_info(&session_id).await {
            Ok(Some(user_info)) => {
                let response = json!({
                    "authenticated": true,
                    "session_id": session_id,
                    "user": {
                        "sub": user_info.sub,
                        "email": user_info.email,
                        "name": user_info.name,
                        "nickname": user_info.nickname,
                        "picture": user_info.picture,
                        "groups": user_info.groups,
                        "permissions": user_info.permissions
                    },
                    "status": "✅ Authenticated"
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text(format!(
                            "Authenticated as: {}\nEmail: {}\nGroups: {:?}",
                            user_info.name.as_deref().unwrap_or("Unknown"),
                            user_info.email.as_deref().unwrap_or("Not provided"),
                            user_info.groups
                        ))
                    })],
                    is_error: Some(false),
                })
            }
            Ok(None) => {
                let response = json!({
                    "authenticated": false,
                    "session_id": session_id,
                    "message": "Not authenticated. Use the 'login' tool to authenticate.",
                    "status": "❌ Not Authenticated"
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text("Not authenticated. Use the 'login' tool to authenticate.")
                    })],
                    is_error: Some(false),
                })
            }
            Err(e) => {
                warn!("Error getting session info for {}: {}", session_id, e);
                Err(McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to get session information: {}", e),
                    None,
                ))
            }
        }
    }
}

/// Logout tool for revoking and clearing session authentication
#[derive(Debug, Clone)]
pub struct LogoutTool {
    pub tool: Tool,
    session_manager: Arc<SessionManager>,
}

impl LogoutTool {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self {
            tool: Tool::new(
                "logout",
                "Sign out and revoke the authentication token for this session.",
                schema_from_type!(EmptyInput),
            ),
            session_manager,
        }
    }

    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        debug!("Logging out session {}", session_id);

        match self.session_manager.revoke_session(&session_id).await {
            Ok(()) => {
                info!("Session {} logged out successfully", session_id);
                
                let response = json!({
                    "status": "logged_out",
                    "message": "✅ Successfully logged out",
                    "session_id": session_id
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text("Successfully logged out.")
                    })],
                    is_error: Some(false),
                })
            }
            Err(SessionManagerError::NotAuthenticated) => {
                // Not an error - user wasn't logged in
                let response = json!({
                    "status": "not_authenticated",
                    "message": "Already logged out",
                    "session_id": session_id
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text("Already logged out.")
                    })],
                    is_error: Some(false),
                })
            }
            Err(e) => {
                warn!("Error during logout for session {}: {}", session_id, e);
                Err(McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Logout failed: {}", e),
                    None,
                ))
            }
        }
    }
}

/// GetGraphQLToken tool for getting current session's access token for use in external tools
#[derive(Debug, Clone)]
pub struct GetGraphQLTokenTool {
    pub tool: Tool,
    session_manager: Arc<SessionManager>,
}

impl GetGraphQLTokenTool {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self {
            tool: Tool::new(
                "getGraphQLToken",
                "Get a valid GraphQL authentication token for this session to use in external tools like Apollo Explorer.",
                schema_from_type!(EmptyInput),
            ),
            session_manager,
        }
    }

    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        debug!("Getting GraphQL token for session {}", session_id);

        match self.session_manager.get_valid_token(&session_id).await {
            Ok(bearer_token) => {
                let response = json!({
                    "token": bearer_token,
                    "session_id": session_id,
                    "usage": {
                        "apollo_explorer": "Add this as 'Authorization' header in Apollo Explorer",
                        "curl": format!("curl -H '{}' https://your-api.example.com/graphql", bearer_token),
                        "headers": {
                            "Authorization": bearer_token
                        }
                    },
                    "warning": "⚠️  This token will expire soon - use it quickly!",
                    "expires_note": "Token automatically refreshes for MCP operations"
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text(format!(
                            "GraphQL Token: {}\n\nUsage: Add as 'Authorization' header\nWarning: This token expires soon!",
                            bearer_token
                        ))
                    })],
                    is_error: Some(false),
                })
            }
            Err(SessionManagerError::NotAuthenticated) => {
                let response = json!({
                    "error": "not_authenticated",
                    "message": "Not authenticated. Use 'login' tool first.",
                    "session_id": session_id
                });

                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap_or_else(|_| {
                        Content::text("Not authenticated. Use 'login' tool first.")
                    })],
                    is_error: Some(true),
                })
            }
            Err(e) => {
                warn!("Error getting token for session {}: {}", session_id, e);
                Err(McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to get authentication token: {}", e),
                    None,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{MemorySessionStorage, SessionManager};
    use crate::runtime::{Auth0Config, PerSessionAuthConfig, SessionStorageType};

    fn create_test_config() -> Auth0Config {
        Auth0Config {
            domain: "test.auth0.com".to_string(),
            client_id: "test_client".to_string(),
            audience: "https://api.test.com".to_string(),
            refresh_token: None,
            per_session_auth: Some(PerSessionAuthConfig {
                enabled: true,
                device_flow_client_id: "device_client".to_string(),
                session_storage: SessionStorageType::Memory,
                token_refresh_buffer_seconds: Some(300),
                device_flow_poll_interval_seconds: Some(5),
                device_flow_timeout_seconds: Some(600),
            }),
        }
    }

    #[tokio::test]
    async fn test_whoami_not_authenticated() {
        let config = create_test_config();
        let storage = Box::new(MemorySessionStorage::new());
        let session_manager = Arc::new(SessionManager::new(config, storage));
        let whoami_tool = WhoAmITool::new(session_manager);

        let result = whoami_tool.execute("test-session".to_string()).await;
        assert!(result.is_ok());

        let tool_result = result.unwrap();
        assert_eq!(tool_result.is_error, Some(false));
    }

    #[tokio::test]
    async fn test_logout_not_authenticated() {
        let config = create_test_config();
        let storage = Box::new(MemorySessionStorage::new());
        let session_manager = Arc::new(SessionManager::new(config, storage));
        let logout_tool = LogoutTool::new(session_manager);

        let result = logout_tool.execute("test-session".to_string()).await;
        assert!(result.is_ok());

        let tool_result = result.unwrap();
        assert_eq!(tool_result.is_error, Some(false));
    }
}