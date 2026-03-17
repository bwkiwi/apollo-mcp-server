# Auth0 Phase 2 Implementation Plan
## Per-Session User Authentication for Apollo MCP Server

### Overview

Phase 2 extends the existing Auth0 integration from server-level shared authentication to per-session user authentication. Each MCP session will have its own Auth0 identity, enabling user-specific GraphQL requests and fine-grained authorization.

## Architecture Changes

### Current State (Phase 1)
```
MCP Client → MCP Server → [Shared Auth0 Token] → GraphQL API
```

### Target State (Phase 2)
```
MCP Client A → MCP Server → [User A Auth0 Token] → GraphQL API
MCP Client B → MCP Server → [User B Auth0 Token] → GraphQL API
```

## Implementation Plan

### Phase 2.1: Foundation & Session Management

#### Task 1: Extend Configuration Schema
**Priority: High | Estimated Time: 4 hours**

Update the configuration to support both Phase 1 (backward compatibility) and Phase 2 modes:

```rust
// src/runtime/auth0.rs
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Auth0Config {
    // Phase 1 fields (existing)
    pub domain: String,
    pub client_id: String,
    pub audience: String,
    pub refresh_token: Option<String>, // Make optional for Phase 2

    // Phase 2 fields (new)
    pub per_session_auth: Option<PerSessionAuthConfig>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PerSessionAuthConfig {
    pub enabled: bool,
    pub device_flow_client_id: String,
    pub session_storage: SessionStorageType,
    pub token_refresh_buffer_seconds: Option<u64>, // Default: 300
    pub device_flow_poll_interval_seconds: Option<u64>, // Default: 5
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub enum SessionStorageType {
    Memory,
    File { path: String },
}
```

#### Task 2: Session Management Infrastructure
**Priority: High | Estimated Time: 8 hours**

Create session tracking and token storage:

```rust
// src/auth/session_manager.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type SessionId = String;

#[derive(Debug, Clone)]
pub struct SessionTokenState {
    pub user_sub: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub user_info: Option<UserInfo>,
}

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub groups: Vec<String>,
}

pub trait SessionStorage: Send + Sync {
    async fn get_session(&self, session_id: &SessionId) -> Option<SessionTokenState>;
    async fn set_session(&self, session_id: &SessionId, state: SessionTokenState);
    async fn remove_session(&self, session_id: &SessionId);
    async fn list_sessions(&self) -> Vec<SessionId>;
}

pub struct MemorySessionStorage {
    sessions: Arc<RwLock<HashMap<SessionId, SessionTokenState>>>,
}

pub struct SessionManager {
    storage: Box<dyn SessionStorage>,
    auth0_config: Auth0Config,
    client: reqwest::Client,
}
```

#### Task 3: Device Flow Implementation
**Priority: High | Estimated Time: 12 hours**

Implement Auth0 Device Authorization Grant:

```rust
// src/auth/device_flow.rs
#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct DeviceTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub token_type: String,
}

impl SessionManager {
    pub async fn initiate_device_flow(&self) -> Result<DeviceCodeResponse, Auth0Error> {
        // POST to /oauth/device/code
    }

    pub async fn poll_device_token(
        &self, 
        device_code: &str
    ) -> Result<DeviceTokenResponse, Auth0Error> {
        // POST to /oauth/token with device_code
    }

    pub async fn complete_device_login(
        &self,
        session_id: SessionId,
        device_code: String,
    ) -> Result<(), Auth0Error> {
        // Poll until success, store tokens
    }
}
```

### Phase 2.2: MCP Tools for Authentication

#### Task 4: Login Tool
**Priority: High | Estimated Time: 6 hours**

```rust
// src/tools/auth_tools.rs
pub struct LoginTool {
    session_manager: Arc<SessionManager>,
}

impl LoginTool {
    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        let device_response = self.session_manager.initiate_device_flow().await?;
        
        // Return instructions to user
        let content = format!(
            "Please visit {} and enter code: {}\n\nOr visit: {}",
            device_response.verification_uri,
            device_response.user_code,
            device_response.verification_uri_complete
        );

        // Start background polling
        let session_manager = self.session_manager.clone();
        let device_code = device_response.device_code.clone();
        tokio::spawn(async move {
            session_manager.complete_device_login(session_id, device_code).await
        });

        Ok(CallToolResult {
            content: vec![Content::text(content)],
            is_error: Some(false),
        })
    }
}
```

#### Task 5: WhoAmI Tool
**Priority: Medium | Estimated Time: 3 hours**

```rust
pub struct WhoAmITool {
    session_manager: Arc<SessionManager>,
}

impl WhoAmITool {
    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        match self.session_manager.get_session_info(&session_id).await {
            Some(user_info) => {
                let info = serde_json::json!({
                    "authenticated": true,
                    "user": {
                        "sub": user_info.sub,
                        "email": user_info.email,
                        "name": user_info.name,
                        "groups": user_info.groups
                    }
                });
                Ok(CallToolResult {
                    content: vec![Content::json(&info).unwrap()],
                    is_error: Some(false),
                })
            }
            None => {
                Ok(CallToolResult {
                    content: vec![Content::text("Not authenticated. Use the 'login' tool to authenticate.")],
                    is_error: Some(false),
                })
            }
        }
    }
}
```

#### Task 6: Logout Tool
**Priority: Medium | Estimated Time: 4 hours**

```rust
pub struct LogoutTool {
    session_manager: Arc<SessionManager>,
}

impl LogoutTool {
    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        self.session_manager.revoke_session(&session_id).await?;
        Ok(CallToolResult {
            content: vec![Content::text("Successfully logged out.")],
            is_error: Some(false),
        })
    }
}
```

### Phase 2.3: Session-Aware GraphQL Requests

#### Task 7: Enhanced GraphQL Request Processing
**Priority: High | Estimated Time: 10 hours**

Update GraphQL request handling to use session-specific tokens:

```rust
// Modify src/graphql.rs
pub struct Request<'a> {
    pub input: Value,
    pub endpoint: &'a Url,
    pub headers: HeaderMap,
    pub session_id: Option<&'a SessionId>, // New field
    pub auth0_token_provider: Option<&'a Arc<Mutex<Auth0TokenProvider>>>, // Phase 1
    pub session_manager: Option<&'a Arc<SessionManager>>, // Phase 2
}

impl Executable {
    async fn execute(&self, request: Request<'_>) -> Result<CallToolResult, McpError> {
        let mut headers = self.headers(&request.headers);
        
        // Phase 2: Per-session authentication
        if let (Some(session_manager), Some(session_id)) = 
            (request.session_manager, request.session_id) {
            
            match session_manager.get_valid_token(session_id).await {
                Ok(bearer_token) => {
                    headers.insert(AUTHORIZATION, HeaderValue::from_str(&bearer_token)?);
                }
                Err(Auth0Error::NotAuthenticated) => {
                    return Err(McpError::new(
                        ErrorCode::INVALID_REQUEST,
                        "Session not authenticated. Use 'login' tool to authenticate.".to_string(),
                        None,
                    ));
                }
                Err(e) => {
                    return Err(McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Authentication failed: {}", e),
                        None,
                    ));
                }
            }
        }
        // Phase 1: Fallback to shared authentication
        else if let Some(token_provider) = request.auth0_token_provider {
            // Existing Phase 1 logic
        }

        // Continue with GraphQL request...
    }
}
```

#### Task 8: Session ID Extraction
**Priority: High | Estimated Time: 6 hours**

Update server state management to extract and propagate session IDs:

```rust
// Modify src/server/states/running.rs
impl Running {
    async fn call_tool(&self, request: CallToolRequestParam, context: RequestContext) -> Result<CallToolResult, ServiceError> {
        // Extract session ID from MCP headers
        let session_id = extract_session_id(&request, &context)?;

        match request.name.as_str() {
            "login" => {
                self.login_tool.execute(session_id).await
            }
            "whoami" => {
                self.whoami_tool.execute(session_id).await
            }
            "logout" => {
                self.logout_tool.execute(session_id).await
            }
            _ => {
                // For GraphQL operations, pass session context
                let graphql_request = graphql::Request {
                    input: Value::from(request.arguments.clone()),
                    endpoint: &self.endpoint,
                    headers,
                    session_id: Some(&session_id),
                    session_manager: self.session_manager.as_ref(),
                    auth0_token_provider: self.auth0_token_provider.as_ref(), // Fallback
                };
                // Execute operation...
            }
        }
    }
}

fn extract_session_id(request: &CallToolRequestParam, context: &RequestContext) -> Result<SessionId, ServiceError> {
    // Extract from MCP session context or generate new one
    context.session_id
        .clone()
        .or_else(|| {
            // Generate session ID if not provided
            Some(uuid::Uuid::new_v4().to_string())
        })
        .ok_or_else(|| ServiceError::InvalidRequest("No session ID available".to_string()))
}
```

### Phase 2.4: Configuration and Integration

#### Task 9: Update Server Builder
**Priority: Medium | Estimated Time: 4 hours**

```rust
// Modify src/server.rs and src/main.rs
pub struct Server {
    // Existing fields...
    session_manager: Option<Arc<SessionManager>>, // New field
    auth_tools_enabled: bool, // New field
}

// In main.rs
async fn main() -> anyhow::Result<()> {
    let config: runtime::Config = // ... load config

    let (auth0_token_provider, session_manager) = if let Some(auth0_config) = config.auth0 {
        if auth0_config.per_session_auth.as_ref().map_or(false, |c| c.enabled) {
            // Phase 2: Per-session auth
            let storage: Box<dyn SessionStorage> = match &auth0_config.per_session_auth.unwrap().session_storage {
                SessionStorageType::Memory => Box::new(MemorySessionStorage::new()),
                SessionStorageType::File { path } => Box::new(FileSessionStorage::new(path)?),
            };
            
            let session_manager = Arc::new(SessionManager::new(auth0_config, storage));
            (None, Some(session_manager))
        } else {
            // Phase 1: Shared auth (existing logic)
            let provider = Auth0TokenProvider::new(/* ... */);
            (Some(Arc::new(Mutex::new(provider))), None)
        }
    } else {
        (None, None)
    };

    let server = Server::builder()
        // ... existing fields
        .maybe_auth0_token_provider(auth0_token_provider)
        .maybe_session_manager(session_manager)
        .build();
}
```

#### Task 10: Enhanced Error Handling
**Priority: Medium | Estimated Time: 3 hours**

```rust
// Extend src/auth/mod.rs
#[derive(Debug, thiserror::Error)]
pub enum Auth0Error {
    // Existing errors...
    
    #[error("Session not authenticated")]
    NotAuthenticated,
    
    #[error("Device flow expired")]
    DeviceFlowExpired,
    
    #[error("Device flow pending")]
    DeviceFlowPending,
    
    #[error("Session storage error: {0}")]
    StorageError(String),
}
```

### Phase 2.5: File-Based Session Storage (Optional)

#### Task 11: Persistent Session Storage
**Priority: Low | Estimated Time: 6 hours**

```rust
// src/auth/file_storage.rs
pub struct FileSessionStorage {
    file_path: PathBuf,
}

impl SessionStorage for FileSessionStorage {
    async fn get_session(&self, session_id: &SessionId) -> Option<SessionTokenState> {
        // Load from encrypted JSON file
    }

    async fn set_session(&self, session_id: &SessionId, state: SessionTokenState) {
        // Save to encrypted JSON file
    }
    
    // Implement encryption/decryption for refresh tokens
}
```

### Phase 2.6: Developer Tools

#### Task 12: GraphQL Token Tool
**Priority: Low | Estimated Time: 3 hours**

```rust
pub struct GetGraphQLTokenTool {
    session_manager: Arc<SessionManager>,
}

impl GetGraphQLTokenTool {
    pub async fn execute(&self, session_id: SessionId) -> Result<CallToolResult, McpError> {
        match self.session_manager.get_valid_token(&session_id).await {
            Ok(bearer_token) => {
                let response = serde_json::json!({
                    "token": bearer_token,
                    "usage": "Add this as 'Authorization' header in Apollo Explorer",
                    "expires_soon": "This token will expire soon - use it quickly"
                });
                Ok(CallToolResult {
                    content: vec![Content::json(&response).unwrap()],
                    is_error: Some(false),
                })
            }
            Err(_) => {
                Err(McpError::new(
                    ErrorCode::INVALID_REQUEST,
                    "Not authenticated. Use 'login' tool first.".to_string(),
                    None,
                ))
            }
        }
    }
}
```

## Testing Strategy

### Phase 2.7: Testing & Validation

#### Task 13: Unit Tests
**Priority: High | Estimated Time: 8 hours**

- Session manager tests
- Device flow mocking
- Token refresh logic
- Storage interface tests

#### Task 14: Integration Tests
**Priority: High | Estimated Time: 12 hours**

- End-to-end authentication flow
- Multi-session scenarios
- Token expiration handling
- Error recovery testing

#### Task 15: Manual Testing
**Priority: Medium | Estimated Time: 6 hours**

- MCP Inspector workflow
- Claude Desktop integration
- Multi-user scenarios
- Auth0 configuration variations

## Migration & Deployment

### Task 16: Documentation Updates
**Priority: Medium | Estimated Time: 6 hours**

- Update Auth0 extension guide
- Add Phase 2 configuration examples
- Migration guide from Phase 1
- Troubleshooting scenarios

### Task 17: Docker Image Updates
**Priority: Medium | Estimated Time: 4 hours**

- Build scripts for new dependencies
- Environment variable documentation
- Docker Compose examples for development

## Timeline Estimate

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| **Phase 2.1: Foundation** | 2-3 weeks | - |
| **Phase 2.2: MCP Tools** | 1-2 weeks | Phase 2.1 |
| **Phase 2.3: GraphQL Integration** | 2-3 weeks | Phase 2.1, 2.2 |
| **Phase 2.4: Configuration** | 1 week | Phase 2.3 |
| **Phase 2.5: File Storage** | 1 week | Phase 2.1 (parallel) |
| **Phase 2.6: Developer Tools** | 1 week | Phase 2.2 (parallel) |
| **Phase 2.7: Testing** | 2 weeks | All previous |

**Total Estimated Time: 8-12 weeks** (depending on team size and complexity)

## Risk Mitigation

### Technical Risks

1. **MCP Session Management Complexity**
   - *Risk*: Session tracking across MCP protocol
   - *Mitigation*: Start with simple in-memory sessions, add persistence later

2. **Device Flow UX in Different MCP Clients**
   - *Risk*: Inconsistent user experience
   - *Mitigation*: Provide clear instructions and fallback mechanisms

3. **Token Storage Security**
   - *Risk*: Refresh token exposure
   - *Mitigation*: Implement encryption, short token lifetimes

4. **Backward Compatibility**
   - *Risk*: Breaking existing Phase 1 deployments
   - *Mitigation*: Feature flags and graceful fallback

### Operational Risks

1. **Auth0 Rate Limiting**
   - *Risk*: Device flow polling overwhelming Auth0
   - *Mitigation*: Implement exponential backoff, configurable intervals

2. **Session Memory Usage**
   - *Risk*: Memory leaks from abandoned sessions
   - *Mitigation*: Implement session cleanup, TTL mechanisms

## Success Criteria

- [ ] Users can authenticate per-session using `login` tool
- [ ] GraphQL requests include user-specific Auth0 tokens
- [ ] Multiple concurrent sessions work independently
- [ ] Backward compatibility with Phase 1 configurations
- [ ] Performance impact < 10% compared to Phase 1
- [ ] Comprehensive test coverage > 80%
- [ ] Documentation covers all use cases

## Future Enhancements (Phase 3+)

- Redis/external session storage
- SSO integration beyond Auth0
- Advanced session management UI
- Audit logging and analytics
- Rate limiting per user
- Custom authorization rules engine