use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::auth::config::Auth0Config;

pub type SessionId = String;

/// Session token state for a specific user session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTokenState {
    /// Auth0 user subject identifier
    pub user_sub: String,
    
    /// Current access token
    pub access_token: String,
    
    /// Refresh token for obtaining new access tokens
    pub refresh_token: String,
    
    /// When the access token expires
    pub expires_at: DateTime<Utc>,
    
    /// User information from Auth0
    pub user_info: Option<UserInfo>,
    
    /// When this session was created
    pub created_at: DateTime<Utc>,
    
    /// Last time this session was accessed
    pub last_accessed: DateTime<Utc>,
}

/// User information from Auth0 ID token claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub nickname: Option<String>,
    pub picture: Option<String>,
    pub groups: Vec<String>,
    pub permissions: Vec<String>,
}

/// Trait for session storage backends
#[async_trait::async_trait]
pub trait SessionStorage: Send + Sync + std::fmt::Debug {
    async fn get_session(&self, session_id: &SessionId) -> Result<Option<SessionTokenState>, SessionStorageError>;
    async fn set_session(&self, session_id: &SessionId, state: SessionTokenState) -> Result<(), SessionStorageError>;
    async fn remove_session(&self, session_id: &SessionId) -> Result<(), SessionStorageError>;
    async fn list_sessions(&self) -> Result<Vec<SessionId>, SessionStorageError>;
    async fn cleanup_expired_sessions(&self) -> Result<usize, SessionStorageError>;
}

/// In-memory session storage implementation
#[derive(Debug)]
pub struct MemorySessionStorage {
    sessions: Arc<RwLock<HashMap<SessionId, SessionTokenState>>>,
}

impl MemorySessionStorage {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl SessionStorage for MemorySessionStorage {
    async fn get_session(&self, session_id: &SessionId) -> Result<Option<SessionTokenState>, SessionStorageError> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }

    async fn set_session(&self, session_id: &SessionId, mut state: SessionTokenState) -> Result<(), SessionStorageError> {
        state.last_accessed = Utc::now();
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), state);
        Ok(())
    }

    async fn remove_session(&self, session_id: &SessionId) -> Result<(), SessionStorageError> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }

    async fn list_sessions(&self) -> Result<Vec<SessionId>, SessionStorageError> {
        let sessions = self.sessions.read().await;
        Ok(sessions.keys().cloned().collect())
    }

    async fn cleanup_expired_sessions(&self) -> Result<usize, SessionStorageError> {
        let mut sessions = self.sessions.write().await;
        let now = Utc::now();
        let initial_count = sessions.len();
        
        sessions.retain(|_, state| {
            // Keep sessions that haven't expired and were accessed within last 24 hours
            let not_expired = state.expires_at > now;
            let recently_accessed = now.signed_duration_since(state.last_accessed).num_hours() < 24;
            not_expired && recently_accessed
        });
        
        let removed_count = initial_count - sessions.len();
        if removed_count > 0 {
            debug!("Cleaned up {} expired sessions", removed_count);
        }
        
        Ok(removed_count)
    }
}

/// Session manager handles Auth0 authentication for multiple sessions
#[derive(Debug)]
pub struct SessionManager {
    storage: Box<dyn SessionStorage>,
    auth0_config: Auth0Config,
    client: reqwest::Client,
}

impl SessionManager {
    pub fn new(auth0_config: Auth0Config, storage: Box<dyn SessionStorage>) -> Self {
        Self {
            storage,
            auth0_config,
            client: reqwest::Client::new(),
        }
    }

    /// Get session information if it exists
    pub async fn get_session_info(&self, session_id: &SessionId) -> Result<Option<UserInfo>, SessionManagerError> {
        match self.storage.get_session(session_id).await? {
            Some(state) => Ok(state.user_info),
            None => Ok(None),
        }
    }

    /// Check if a session is authenticated
    pub async fn is_authenticated(&self, session_id: &SessionId) -> Result<bool, SessionManagerError> {
        match self.storage.get_session(session_id).await? {
            Some(state) => {
                // Check if token is still valid (with buffer)
                let buffer = self.auth0_config.token_refresh_buffer();
                let expires_with_buffer = state.expires_at - chrono::Duration::from_std(buffer).unwrap();
                Ok(Utc::now() < expires_with_buffer)
            }
            None => Ok(false),
        }
    }

    /// Get a valid bearer token for the session, refreshing if necessary
    pub async fn get_valid_token(&self, session_id: &SessionId) -> Result<String, SessionManagerError> {
        let mut state = self.storage.get_session(session_id).await?
            .ok_or(SessionManagerError::NotAuthenticated)?;

        // Check if token needs refresh
        let buffer = self.auth0_config.token_refresh_buffer();
        let expires_with_buffer = state.expires_at - chrono::Duration::from_std(buffer).unwrap();
        
        if Utc::now() >= expires_with_buffer {
            debug!("Refreshing access token for session {}", session_id);
            state = self.refresh_access_token(state).await?;
            self.storage.set_session(session_id, state.clone()).await?;
        }

        Ok(format!("Bearer {}", state.access_token))
    }

    /// Store a new session after successful authentication
    pub async fn store_session(
        &self,
        session_id: SessionId,
        access_token: String,
        refresh_token: String,
        expires_in: u64,
        user_info: Option<UserInfo>,
    ) -> Result<(), SessionManagerError> {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(expires_in as i64);
        
        let state = SessionTokenState {
            user_sub: user_info.as_ref().map(|u| u.sub.clone()).unwrap_or_default(),
            access_token,
            refresh_token,
            expires_at,
            user_info,
            created_at: now,
            last_accessed: now,
        };

        self.storage.set_session(&session_id, state).await?;
        debug!("Stored new session {} with expiry {}", session_id, expires_at);
        Ok(())
    }

    /// Revoke and remove a session
    pub async fn revoke_session(&self, session_id: &SessionId) -> Result<(), SessionManagerError> {
        if let Some(state) = self.storage.get_session(session_id).await? {
            // Attempt to revoke the refresh token with Auth0
            if let Err(e) = self.revoke_refresh_token(&state.refresh_token).await {
                warn!("Failed to revoke refresh token with Auth0: {}", e);
                // Continue with local removal even if Auth0 revocation fails
            }
        }

        self.storage.remove_session(session_id).await?;
        debug!("Revoked session {}", session_id);
        Ok(())
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Result<Vec<SessionId>, SessionManagerError> {
        Ok(self.storage.list_sessions().await?)
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<usize, SessionManagerError> {
        Ok(self.storage.cleanup_expired_sessions().await?)
    }

    /// Refresh an access token using the refresh token
    async fn refresh_access_token(&self, state: SessionTokenState) -> Result<SessionTokenState, SessionManagerError> {
        let token_url = format!("https://{}/oauth/token", self.auth0_config.domain);
        
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": self.auth0_config.client_id,
            "refresh_token": state.refresh_token,
            "audience": self.auth0_config.audience,
        });

        let response = self.client
            .post(&token_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(SessionManagerError::RequestFailed)?;

        if response.status().is_success() {
            let token_response: TokenRefreshResponse = response
                .json()
                .await
                .map_err(SessionManagerError::ResponseParseFailed)?;

            let mut new_state = state;
            new_state.access_token = token_response.access_token;
            new_state.expires_at = Utc::now() + chrono::Duration::seconds(token_response.expires_in as i64);
            
            // Update refresh token if a new one was provided
            if let Some(new_refresh_token) = token_response.refresh_token {
                new_state.refresh_token = new_refresh_token;
            }

            debug!("Successfully refreshed access token, expires at: {:?}", new_state.expires_at);
            Ok(new_state)
        } else {
            let error_text = response.text().await.unwrap_or_default();
            error!("Auth0 token refresh failed: {}", error_text);
            Err(SessionManagerError::TokenRefreshFailed(error_text))
        }
    }

    /// Revoke a refresh token with Auth0
    async fn revoke_refresh_token(&self, refresh_token: &str) -> Result<(), SessionManagerError> {
        let revoke_url = format!("https://{}/oauth/revoke", self.auth0_config.domain);
        
        let body = serde_json::json!({
            "client_id": self.auth0_config.client_id,
            "token": refresh_token,
        });

        let response = self.client
            .post(&revoke_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(SessionManagerError::RequestFailed)?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(SessionManagerError::TokenRevocationFailed(error_text));
        }

        Ok(())
    }
}

/// Token refresh response from Auth0
#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

/// Session manager errors
#[derive(Debug, Error)]
pub enum SessionManagerError {
    #[error("Session not authenticated")]
    NotAuthenticated,
    
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    
    #[error("Failed to parse response: {0}")]
    ResponseParseFailed(reqwest::Error),
    
    #[error("Token refresh failed: {0}")]
    TokenRefreshFailed(String),
    
    #[error("Token revocation failed: {0}")]
    TokenRevocationFailed(String),
    
    #[error("Session storage error: {0}")]
    StorageError(#[from] SessionStorageError),
}

/// Session storage errors
#[derive(Debug, Error)]
pub enum SessionStorageError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("Storage operation failed: {0}")]
    OperationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_storage() {
        let storage = MemorySessionStorage::new();
        let session_id = "test-session".to_string();
        
        // Initially no session
        assert!(storage.get_session(&session_id).await.unwrap().is_none());
        
        // Store a session
        let state = SessionTokenState {
            user_sub: "auth0|123".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            user_info: None,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
        };
        
        storage.set_session(&session_id, state.clone()).await.unwrap();
        
        // Retrieve the session
        let retrieved = storage.get_session(&session_id).await.unwrap().unwrap();
        assert_eq!(retrieved.user_sub, state.user_sub);
        assert_eq!(retrieved.access_token, state.access_token);
        
        // Remove the session
        storage.remove_session(&session_id).await.unwrap();
        assert!(storage.get_session(&session_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_session_cleanup() {
        let storage = MemorySessionStorage::new();
        
        // Create an expired session
        let expired_state = SessionTokenState {
            user_sub: "auth0|expired".to_string(),
            access_token: "expired-token".to_string(),
            refresh_token: "expired-refresh".to_string(),
            expires_at: Utc::now() - chrono::Duration::hours(1), // Expired
            user_info: None,
            created_at: Utc::now() - chrono::Duration::hours(2),
            last_accessed: Utc::now() - chrono::Duration::hours(25), // Old access
        };
        
        storage.set_session("expired-session", expired_state).await.unwrap();
        
        // Create a valid session
        let valid_state = SessionTokenState {
            user_sub: "auth0|valid".to_string(),
            access_token: "valid-token".to_string(),
            refresh_token: "valid-refresh".to_string(),
            expires_at: Utc::now() + chrono::Duration::hours(1), // Valid
            user_info: None,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
        };
        
        storage.set_session("valid-session", valid_state).await.unwrap();
        
        // Cleanup should remove expired session
        let removed_count = storage.cleanup_expired_sessions().await.unwrap();
        assert_eq!(removed_count, 1);
        
        // Valid session should remain
        assert!(storage.get_session("valid-session").await.unwrap().is_some());
        assert!(storage.get_session("expired-session").await.unwrap().is_none());
    }
}