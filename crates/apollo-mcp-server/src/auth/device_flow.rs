use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::{sleep, timeout, Instant};
use tracing::{debug, error, info, warn};

use crate::auth::session_manager::{SessionManager, SessionManagerError, UserInfo};
use crate::auth::config::Auth0Config;
use crate::auth::secure_logging::sensitive;

pub type SessionId = String;

/// Device code response from Auth0 /oauth/device/code endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    /// Device code for polling
    pub device_code: String,
    
    /// User code to display to the user
    pub user_code: String,
    
    /// URL where user should go to authorize
    pub verification_uri: String,
    
    /// Complete URL with user code embedded
    pub verification_uri_complete: String,
    
    /// How long the device code is valid (seconds)
    pub expires_in: u64,
    
    /// How often to poll (seconds)
    pub interval: u64,
}

/// Device token response from Auth0 /oauth/token endpoint
#[derive(Debug, Deserialize)]
pub struct DeviceTokenResponse {
    /// Access token
    pub access_token: String,
    
    /// Refresh token
    pub refresh_token: String,
    
    /// Token expiry in seconds
    pub expires_in: u64,
    
    /// Token type (usually "Bearer")
    pub token_type: String,
    
    /// ID token (contains user info)
    pub id_token: Option<String>,
}

/// Error response from Auth0 during device flow
#[derive(Debug, Deserialize)]
pub struct DeviceFlowErrorResponse {
    pub error: String,
    pub error_description: Option<String>,
}

/// Device flow manager handles the Auth0 device authorization flow
#[derive(Debug)]
pub struct DeviceFlowManager {
    auth0_config: Auth0Config,
    client: reqwest::Client,
    active_flows: Arc<RwLock<HashMap<SessionId, ActiveDeviceFlow>>>,
}

/// An active device flow for a session
#[derive(Debug, Clone)]
struct ActiveDeviceFlow {
    device_code: String,
    user_code: String,
    verification_uri_complete: String,
    expires_at: Instant,
    poll_interval: Duration,
}

impl DeviceFlowManager {
    pub fn new(auth0_config: Auth0Config) -> Self {
        Self {
            auth0_config,
            client: reqwest::Client::new(),
            active_flows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initiate a device flow for a session
    pub async fn initiate_device_flow(&self, session_id: SessionId) -> Result<DeviceCodeResponse, DeviceFlowError> {
        let device_code_url = format!("https://{}/oauth/device/code", self.auth0_config.domain);
        
        let body = serde_json::json!({
            "client_id": self.auth0_config.device_flow_client_id().unwrap(),
            "audience": self.auth0_config.audience,
            "scope": "openid profile email offline_access",
        });

        debug!("Initiating device flow for session {}", session_id);
        
        let response = self.client
            .post(&device_code_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(DeviceFlowError::RequestFailed)?;

        if response.status().is_success() {
            let device_response: DeviceCodeResponse = response
                .json()
                .await
                .map_err(DeviceFlowError::ResponseParseFailed)?;

            // Store the active flow
            let active_flow = ActiveDeviceFlow {
                device_code: device_response.device_code.clone(),
                user_code: device_response.user_code.clone(),
                verification_uri_complete: device_response.verification_uri_complete.clone(),
                expires_at: Instant::now() + Duration::from_secs(device_response.expires_in),
                poll_interval: Duration::from_secs(device_response.interval),
            };
            
            let mut flows = self.active_flows.write().await;
            flows.insert(session_id.clone(), active_flow);

            info!("Device flow initiated for session {}: user_code={}, expires_in={}s", 
                session_id, sensitive(&device_response.user_code), device_response.expires_in);

            Ok(device_response)
        } else {
            let error_text = response.text().await.unwrap_or_default();
            error!("Device flow initiation failed: {}", error_text);
            Err(DeviceFlowError::InitiationFailed(error_text))
        }
    }

    /// Start polling for device flow completion in the background
    pub async fn start_device_flow_polling(
        &self,
        session_id: SessionId,
        session_manager: Arc<SessionManager>,
    ) -> Result<(), DeviceFlowError> {
        let active_flow = {
            let flows = self.active_flows.read().await;
            flows.get(&session_id).cloned()
                .ok_or(DeviceFlowError::FlowNotFound)?
        };

        let auth0_config = self.auth0_config.clone();
        let client = self.client.clone();
        let active_flows = self.active_flows.clone();

        // Spawn background task for polling
        tokio::spawn(async move {
            let result = Self::poll_for_completion(
                session_id.clone(),
                active_flow,
                auth0_config,
                client,
                session_manager,
            ).await;

            // Remove from active flows when done
            let mut flows = active_flows.write().await;
            flows.remove(&session_id);

            match result {
                Ok(()) => {
                    info!("Device flow completed successfully for session {}", session_id);
                }
                Err(e) => {
                    warn!("Device flow failed for session {}: {}", session_id, e);
                }
            }
        });

        Ok(())
    }

    /// Check if a session has an active device flow
    pub async fn has_active_flow(&self, session_id: &SessionId) -> bool {
        let flows = self.active_flows.read().await;
        flows.contains_key(session_id)
    }

    /// Get device flow status for a session
    pub async fn get_flow_status(&self, session_id: &SessionId) -> Option<DeviceFlowStatus> {
        let flows = self.active_flows.read().await;
        flows.get(session_id).map(|flow| {
            let remaining = flow.expires_at.saturating_duration_since(Instant::now());
            DeviceFlowStatus {
                user_code: flow.user_code.clone(),
                verification_uri_complete: flow.verification_uri_complete.clone(),
                expires_in_seconds: remaining.as_secs(),
                is_expired: remaining.is_zero(),
            }
        })
    }

    /// Cancel an active device flow
    pub async fn cancel_flow(&self, session_id: &SessionId) -> bool {
        let mut flows = self.active_flows.write().await;
        flows.remove(session_id).is_some()
    }

    /// Poll Auth0 for device flow completion
    async fn poll_for_completion(
        session_id: SessionId,
        flow: ActiveDeviceFlow,
        auth0_config: Auth0Config,
        client: reqwest::Client,
        session_manager: Arc<SessionManager>,
    ) -> Result<(), DeviceFlowError> {
        let token_url = format!("https://{}/oauth/token", auth0_config.domain);
        let timeout_duration = auth0_config.device_flow_timeout();
        let poll_interval = flow.poll_interval;

        let polling_result = timeout(timeout_duration, async {
            loop {
                debug!("Polling for device flow completion for session {}", session_id);
                
                let body = serde_json::json!({
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    "device_code": flow.device_code,
                    "client_id": auth0_config.device_flow_client_id().unwrap(),
                });

                let response = client
                    .post(&token_url)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(DeviceFlowError::RequestFailed)?;

                if response.status().is_success() {
                    // Success! Parse the token response
                    let token_response: DeviceTokenResponse = response
                        .json()
                        .await
                        .map_err(DeviceFlowError::ResponseParseFailed)?;

                    // Extract user info from ID token if available
                    let user_info = if let Some(id_token) = &token_response.id_token {
                        Self::parse_user_info_from_id_token(id_token).unwrap_or_else(|e| {
                            warn!("Failed to parse user info from ID token: {}", e);
                            None
                        })
                    } else {
                        None
                    };

                    // Store the session
                    session_manager.store_session(
                        session_id.clone(),
                        token_response.access_token,
                        token_response.refresh_token,
                        token_response.expires_in,
                        user_info,
                    ).await.map_err(DeviceFlowError::SessionError)?;

                    return Ok(());
                } else {
                    // Get response text first for error handling
                    let response_text = response.text().await.unwrap_or_default();
                    
                    // Try to parse as error response
                    let error_response: Result<DeviceFlowErrorResponse, _> = 
                        serde_json::from_str(&response_text);
                    
                    match error_response {
                        Ok(err) => {
                            match err.error.as_str() {
                                "authorization_pending" => {
                                    // User hasn't authorized yet, continue polling
                                    debug!("Authorization pending for session {}", session_id);
                                }
                                "slow_down" => {
                                    // Auth0 wants us to slow down polling
                                    debug!("Slowing down polling for session {}", session_id);
                                    sleep(poll_interval * 2).await;
                                    continue;
                                }
                                "expired_token" => {
                                    return Err(DeviceFlowError::DeviceCodeExpired);
                                }
                                "access_denied" => {
                                    return Err(DeviceFlowError::UserDeniedAccess);
                                }
                                _ => {
                                    return Err(DeviceFlowError::AuthorizationFailed(
                                        err.error_description.unwrap_or(err.error)
                                    ));
                                }
                            }
                        }
                        Err(_) => {
                            return Err(DeviceFlowError::UnexpectedResponse(response_text));
                        }
                    }
                }

                // Check if flow has expired
                if Instant::now() > flow.expires_at {
                    return Err(DeviceFlowError::DeviceCodeExpired);
                }

                // Wait before next poll
                sleep(poll_interval).await;
            }
        }).await;

        match polling_result {
            Ok(result) => result,
            Err(_) => Err(DeviceFlowError::PollingTimeout),
        }
    }

    /// Parse user information from ID token (basic JWT parsing)
    fn parse_user_info_from_id_token(id_token: &str) -> Result<Option<UserInfo>, DeviceFlowError> {
        // Simple JWT parsing - split by dots and decode payload
        let parts: Vec<&str> = id_token.split('.').collect();
        if parts.len() != 3 {
            return Err(DeviceFlowError::InvalidIdToken("Invalid JWT format".to_string()));
        }

        // Decode the payload (second part)
        let payload = parts[1];
        
        // Add padding if needed for base64 decoding
        let padded_payload = match payload.len() % 4 {
            0 => payload.to_string(),
            n => format!("{}{}", payload, "=".repeat(4 - n)),
        };

        let decoded = base64::engine::general_purpose::URL_SAFE.decode(&padded_payload)
            .map_err(|e| DeviceFlowError::InvalidIdToken(format!("Base64 decode failed: {}", e)))?;

        let claims: serde_json::Value = serde_json::from_slice(&decoded)
            .map_err(|e| DeviceFlowError::InvalidIdToken(format!("JSON parse failed: {}", e)))?;

        // Extract user information from claims
        let user_info = UserInfo {
            sub: claims.get("sub")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            email: claims.get("email")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            name: claims.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            nickname: claims.get("nickname")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            picture: claims.get("picture")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            groups: claims.get("groups")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default(),
            permissions: claims.get("permissions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default(),
        };

        Ok(Some(user_info))
    }
}

/// Status of an active device flow
#[derive(Debug, Clone, Serialize)]
pub struct DeviceFlowStatus {
    pub user_code: String,
    pub verification_uri_complete: String,
    pub expires_in_seconds: u64,
    pub is_expired: bool,
}

/// Device flow specific errors
#[derive(Debug, Error)]
pub enum DeviceFlowError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    
    #[error("Failed to parse response: {0}")]
    ResponseParseFailed(reqwest::Error),
    
    #[error("Device flow initiation failed: {0}")]
    InitiationFailed(String),
    
    #[error("Device flow not found for session")]
    FlowNotFound,
    
    #[error("Device code has expired")]
    DeviceCodeExpired,
    
    #[error("User denied access")]
    UserDeniedAccess,
    
    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),
    
    #[error("Unexpected response: {0}")]
    UnexpectedResponse(String),
    
    #[error("Polling timeout")]
    PollingTimeout,
    
    #[error("Invalid ID token: {0}")]
    InvalidIdToken(String),
    
    #[error("Session management error: {0}")]
    SessionError(#[from] SessionManagerError),
}

// Add base64 dependency
use base64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_id_token() {
        // Create a simple test JWT payload
        let claims = serde_json::json!({
            "sub": "auth0|123456789",
            "email": "test@example.com",
            "name": "Test User",
            "nickname": "tester"
        });
        
        let payload = base64::encode(claims.to_string());
        let fake_jwt = format!("header.{}.signature", payload);
        
        let auth0_config = Auth0Config {
            domain: "test.auth0.com".to_string(),
            client_id: "test_client".to_string(),
            audience: "test_audience".to_string(),
            refresh_token: None,
            per_session_auth: None,
        };
        
        let result = DeviceFlowManager::parse_user_info_from_id_token(&fake_jwt);
        assert!(result.is_ok());
        
        let user_info = result.unwrap().unwrap();
        assert_eq!(user_info.sub, "auth0|123456789");
        assert_eq!(user_info.email, Some("test@example.com".to_string()));
        assert_eq!(user_info.name, Some("Test User".to_string()));
    }
}