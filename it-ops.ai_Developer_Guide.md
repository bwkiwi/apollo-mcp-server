# Apollo MCP Server Phase 2 Auth0 - Developer Guide

## Overview

This guide provides comprehensive information for developers working with the Apollo MCP Server Phase 2 Auth0 implementation. It covers architecture, development setup, configuration, troubleshooting, and extension points.

## Architecture Deep Dive

### System Architecture

```
┌─────────────────┐    HTTP/MCP    ┌─────────────────────────────────┐
│  Claude Desktop │ ──────────────▶│     Apollo MCP Container        │
│                 │                │  ┌─────────────────────────────┐ │
│  MCP Client     │                │  │       MCP Protocol          │ │
│  (mcp-remote)   │                │  │    ┌─────────────────────┐  │ │
└─────────────────┘                │  │    │   Running State     │  │ │
                                   │  │    │  ┌─────────────────┐ │  │ │
┌─────────────────┐                │  │    │  │ Session Manager │ │  │ │
│     Auth0       │◀──────────────▶│  │    │  │ Device Flow Mgr │ │  │ │
│                 │  Device Flow   │  │    │  │ Auth Tools      │ │  │ │
│  Device Code    │                │  │    │  │ GraphQL Tools   │ │  │ │
│  Token Exchange │                │  │    │  └─────────────────┘ │  │ │
└─────────────────┘                │  │    └─────────────────────┘  │ │
                                   │  │                             │ │
┌─────────────────┐                │  └─────────────────────────────┘ │
│  GraphQL API    │◀──────────────▶│            Port 5000              │
│                 │  Auth'd Requests└─────────────────────────────────┘
│  Your Backend   │
└─────────────────┘
```

### Core Components

#### 1. **Auth Module (`crates/apollo-mcp-server/src/auth/`)**

```rust
// Module structure
auth/
├── mod.rs              // Main auth module exports
├── config.rs           // Auth0 configuration types
├── session_manager.rs  // Session storage and lifecycle
├── device_flow.rs      // Auth0 device authorization grant
├── auth_tools.rs       // MCP tools (login, whoami, logout, getGraphQLToken)
├── networked_token_validator.rs  // Token validation (existing)
└── protected_resource.rs         // OAuth resource metadata (existing)
```

#### 2. **Configuration Types**

```rust
// auth/config.rs
pub struct Auth0Config {
    pub domain: String,                     // Auth0 tenant domain
    pub client_id: String,                  // Client ID for GraphQL API
    pub audience: String,                   // GraphQL API audience
    pub refresh_token: Option<String>,      // Phase 1 compatibility
    pub per_session_auth: Option<PerSessionAuthConfig>,
}

pub struct PerSessionAuthConfig {
    pub enabled: bool,                      // Enable Phase 2
    pub device_flow_client_id: String,      // Native app client ID
    pub session_storage: SessionStorageType,
    pub token_refresh_buffer_seconds: Option<u64>,
    pub device_flow_poll_interval_seconds: Option<u64>,
    pub device_flow_timeout_seconds: Option<u64>,
}

pub enum SessionStorageType {
    Memory,                                 // In-memory storage
    File { path: String },                  // File-based storage
}
```

#### 3. **Session Management**

```rust
// auth/session_manager.rs
pub struct SessionManager {
    config: Auth0Config,
    storage: Box<dyn SessionStorage>,
}

#[async_trait]
pub trait SessionStorage: Send + Sync + Debug {
    async fn get_session(&self, session_id: &SessionId) -> Result<Option<SessionTokenState>, SessionStorageError>;
    async fn set_session(&self, session_id: &SessionId, state: SessionTokenState) -> Result<(), SessionStorageError>;
    async fn remove_session(&self, session_id: &SessionId) -> Result<(), SessionStorageError>;
    async fn list_sessions(&self) -> Result<Vec<SessionId>, SessionStorageError>;
}
```

#### 4. **Device Flow Implementation**

```rust
// auth/device_flow.rs
pub struct DeviceFlowManager {
    config: Auth0Config,
    active_flows: Arc<RwLock<HashMap<SessionId, DeviceFlowState>>>,
}

impl DeviceFlowManager {
    // Initiate device authorization flow
    pub async fn initiate_device_flow(&self, session_id: SessionId) -> Result<DeviceCodeResponse, DeviceFlowError>;
    
    // Start background polling for token exchange
    pub async fn start_device_flow_polling(&self, session_id: SessionId, session_manager: Arc<SessionManager>) -> Result<(), DeviceFlowError>;
    
    // Get current flow status
    pub async fn get_flow_status(&self, session_id: &SessionId) -> Option<DeviceFlowStatus>;
}
```

#### 5. **MCP Tools**

```rust
// auth/auth_tools.rs
pub struct LoginTool {
    pub tool: Tool,  // MCP tool definition
    device_flow_manager: Arc<DeviceFlowManager>,
    session_manager: Arc<SessionManager>,
}

// Similar structure for WhoAmITool, LogoutTool, GetGraphQLTokenTool
```

## Development Setup

### Prerequisites

```bash
# Required tools
rust 1.75+
docker
node.js (for mcp-remote)
curl (for testing)

# Install MCP remote client
npm install -g mcp-remote
```

### Build Environment

```bash
# Clone repository
git clone <repository-url>
cd apollo-mcp-server

# Build development version
cargo build

# Build release version
cargo build --release

# Build Docker image
docker build -t apollo-mcp-server:dev .
```

### Development Configuration

Create a development config file `config/dev-config.yaml`:

```yaml
# Development Configuration
endpoint: "http://localhost:4000/graphql"  # Your local GraphQL server

transport:
  type: streamable_http
  address: "127.0.0.1"  # Localhost only for dev
  port: 5000

schema:
  local:
    path: "/config/schema.graphql"

operations:
  introspect: {}

# Auth0 Development Configuration
auth0:
  domain: "dev-tenant.auth0.com"
  client_id: "dev-client-id"
  audience: "https://dev-api.example.com"
  per_session_auth:
    enabled: true
    device_flow_client_id: "dev-device-client-id"
    session_storage:
      type: memory  # Use memory for development
    token_refresh_buffer_seconds: 60   # Shorter for testing
    device_flow_poll_interval_seconds: 2
    device_flow_timeout_seconds: 300

introspection:
  execute: { enabled: true }
  introspect: { enabled: true }
  search: { enabled: true }

health_check:
  enabled: true
  path: "/health"
```

### Running in Development

```bash
# Run directly
RUST_LOG=debug ./target/debug/apollo-mcp-server config/dev-config.yaml

# Run with Docker
docker run -it --rm \
  -p 5000:5000 \
  -v $(pwd)/config:/config:ro \
  -e RUST_LOG=debug \
  apollo-mcp-server:dev \
  /config/dev-config.yaml
```

## Testing and Debugging

### Unit Testing

```bash
# Run all tests
cargo test

# Run specific module tests
cargo test auth::session_manager
cargo test auth::device_flow
cargo test auth::auth_tools

# Run with output
cargo test -- --nocapture
```

### Integration Testing

```bash
# Test MCP protocol manually
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | ./target/debug/apollo-mcp-server config/dev-config.yaml

# Test HTTP transport
curl -X POST http://localhost:5000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'

# Test tools listing
curl -X POST http://localhost:5000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

### Debug Logging

```bash
# Enable all debug logs
RUST_LOG=debug

# Enable specific module logs
RUST_LOG=apollo_mcp_server::auth=debug

# Enable trace level for detailed debugging
RUST_LOG=apollo_mcp_server::auth::device_flow=trace
```

### Common Debug Scenarios

#### 1. **Tools Not Appearing**

```bash
# Check tool registration
RUST_LOG=apollo_mcp_server::server::states::starting=debug ./target/debug/apollo-mcp-server config.yaml

# Look for logs like:
# "Phase 2 Auth0 authentication tools created"
# "Tools registered: login, whoami, logout, getGraphQLToken"
```

#### 2. **Auth0 Configuration Issues**

```bash
# Test Auth0 device code endpoint
curl -X POST https://YOUR-DOMAIN.auth0.com/oauth/device/code \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "client_id=YOUR-CLIENT-ID&scope=openid profile email&audience=YOUR-AUDIENCE"

# Test token endpoint
curl -X POST https://YOUR-DOMAIN.auth0.com/oauth/token \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=urn:ietf:params:oauth:grant-type:device_code&device_code=DEVICE-CODE&client_id=YOUR-CLIENT-ID"
```

#### 3. **Session Storage Issues**

```bash
# Check file storage permissions
ls -la /data/sessions.json

# Check memory storage (debug logs)
RUST_LOG=apollo_mcp_server::auth::session_manager=trace
```

## Configuration Reference

### Complete Configuration Schema

```yaml
# Transport configuration
transport:
  type: streamable_http | sse | stdio
  address: "0.0.0.0"     # Listen address
  port: 5000             # Listen port
  auth:                  # Optional transport-level auth
    servers: ["https://auth-server.com"]
    audiences: ["mcp-server"]
    resource: "https://this-server.com"

# GraphQL endpoint
endpoint: "https://api.example.com/graphql"

# Schema source (choose one)
schema:
  local: { path: "/config/schema.graphql" }
  # OR
  uplink: {}
  # OR
  introspect: { endpoint: "https://api.example.com/graphql" }

# Operations source (choose one)
operations:
  introspect: {}
  # OR
  collection: { id: "default" }
  # OR
  local: { paths: ["/config/operations"] }

# GraphOS configuration (if using uplink/collections)
graphos:
  api_key: "service:graph-name:key"
  graph_ref: "graph-name@variant"

# Auth0 Phase 2 Configuration
auth0:
  domain: "tenant.auth0.com"
  client_id: "client-id-for-api"
  audience: "https://api.example.com"
  
  # Phase 1 compatibility (optional)
  refresh_token: "refresh-token-string"
  
  # Phase 2 configuration
  per_session_auth:
    enabled: true
    device_flow_client_id: "native-app-client-id"
    
    # Session storage
    session_storage:
      type: memory
      # OR
      type: file
      path: "/data/sessions.json"
    
    # Timing configuration
    token_refresh_buffer_seconds: 300      # Refresh 5 min before expiry
    device_flow_poll_interval_seconds: 5   # Poll every 5 seconds
    device_flow_timeout_seconds: 600       # 10 minute timeout

# Introspection tools
introspection:
  execute: { enabled: true }
  introspect: { enabled: true }
  search: { enabled: true }
  validate: { enabled: true }

# Health check
health_check:
  enabled: true
  path: "/health"

# Optional headers for GraphQL requests
headers:
  User-Agent: "Apollo-MCP-Server/1.0"
  Custom-Header: "value"

# Explorer integration
overrides:
  enable_explorer: true
```

### Environment Variable Overrides

```bash
# Transport configuration
APOLLO_MCP_TRANSPORT__TYPE=streamable_http
APOLLO_MCP_TRANSPORT__ADDRESS=0.0.0.0
APOLLO_MCP_TRANSPORT__PORT=5000

# Auth0 configuration
APOLLO_MCP_AUTH0__DOMAIN=tenant.auth0.com
APOLLO_MCP_AUTH0__CLIENT_ID=client-id
APOLLO_MCP_AUTH0__AUDIENCE=https://api.example.com

# Logging
RUST_LOG=info
NO_COLOR=1
```

## Extension Points

### Custom Session Storage Backend

```rust
// Implement custom storage backend
pub struct RedisSessionStorage {
    client: redis::Client,
}

#[async_trait]
impl SessionStorage for RedisSessionStorage {
    async fn get_session(&self, session_id: &SessionId) -> Result<Option<SessionTokenState>, SessionStorageError> {
        // Redis implementation
    }
    
    async fn set_session(&self, session_id: &SessionId, state: SessionTokenState) -> Result<(), SessionStorageError> {
        // Redis implementation
    }
    
    // ... other methods
}

// Register in main.rs
let storage: Box<dyn SessionStorage> = match &config.session_storage {
    SessionStorageType::Memory => Box::new(MemorySessionStorage::new()),
    SessionStorageType::File { path } => Box::new(FileSessionStorage::new(path)),
    SessionStorageType::Redis { url } => Box::new(RedisSessionStorage::new(url)?),
};
```

### Custom Authentication Tools

```rust
// Create custom MCP tool
#[derive(Debug, Clone)]
pub struct CustomAuthTool {
    pub tool: Tool,
    session_manager: Arc<SessionManager>,
}

impl CustomAuthTool {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self {
            tool: Tool::new(
                "custom-auth",
                "Custom authentication functionality",
                schema_from_type!(CustomInput),
            ),
            session_manager,
        }
    }
    
    pub async fn execute(&self, session_id: SessionId, input: CustomInput) -> Result<CallToolResult, McpError> {
        // Custom implementation
    }
}

// Register in starting.rs
let custom_tool = if session_manager.is_some() {
    Some(CustomAuthTool::new(session_manager.clone().unwrap()))
} else {
    None
};
```

### Custom Error Handling

```rust
// Extend error types
#[derive(Error, Debug)]
pub enum CustomSessionError {
    #[error("Custom session error: {0}")]
    Custom(String),
    
    #[error("Session validation failed: {0}")]
    ValidationFailed(String),
}

// Implement From conversions
impl From<CustomSessionError> for SessionManagerError {
    fn from(err: CustomSessionError) -> Self {
        SessionManagerError::StorageError(err.to_string())
    }
}
```

## Performance Optimization

### Session Storage Performance

```rust
// Optimize memory storage with TTL cleanup
impl MemorySessionStorage {
    pub async fn cleanup_expired_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        let now = Utc::now();
        
        sessions.retain(|_, state| {
            state.expires_at > now
        });
    }
}

// Background cleanup task
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(300));
    loop {
        interval.tick().await;
        storage.cleanup_expired_sessions().await;
    }
});
```

### Token Refresh Optimization

```rust
// Batch token refresh for multiple sessions
impl SessionManager {
    pub async fn refresh_expiring_tokens(&self) -> Result<(), SessionManagerError> {
        let buffer = self.config.token_refresh_buffer();
        let sessions_to_refresh = self.storage.get_expiring_sessions(buffer).await?;
        
        // Refresh tokens in parallel
        let refresh_futures: Vec<_> = sessions_to_refresh
            .into_iter()
            .map(|session_id| self.refresh_token_if_needed(&session_id))
            .collect();
            
        futures::future::join_all(refresh_futures).await;
        Ok(())
    }
}
```

## Security Considerations

### Token Storage Security

```rust
// Encrypt tokens at rest (file storage)
use aes_gcm::{Aes256Gcm, Key, Nonce};

impl FileSessionStorage {
    fn encrypt_session_data(&self, data: &[u8]) -> Result<Vec<u8>, SessionStorageError> {
        let key = self.get_encryption_key()?;
        let cipher = Aes256Gcm::new(&key);
        let nonce = Nonce::from_slice(&self.generate_nonce());
        
        cipher.encrypt(nonce, data)
            .map_err(|e| SessionStorageError::EncryptionError(e.to_string()))
    }
}
```

### Input Validation

```rust
// Validate session IDs
pub fn validate_session_id(session_id: &str) -> Result<(), SessionManagerError> {
    if session_id.is_empty() || session_id.len() > 256 {
        return Err(SessionManagerError::InvalidSessionId);
    }
    
    if !session_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(SessionManagerError::InvalidSessionId);
    }
    
    Ok(())
}
```

### Rate Limiting

```rust
// Implement rate limiting for authentication attempts
use std::collections::HashMap;
use tokio::time::{Duration, Instant};

pub struct RateLimiter {
    attempts: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_attempts: usize,
    window: Duration,
}

impl RateLimiter {
    pub async fn check_rate_limit(&self, identifier: &str) -> Result<(), DeviceFlowError> {
        let mut attempts = self.attempts.lock().await;
        let now = Instant::now();
        
        let user_attempts = attempts.entry(identifier.to_string()).or_default();
        user_attempts.retain(|&attempt_time| now.duration_since(attempt_time) < self.window);
        
        if user_attempts.len() >= self.max_attempts {
            return Err(DeviceFlowError::RateLimitExceeded);
        }
        
        user_attempts.push(now);
        Ok(())
    }
}
```

## Troubleshooting Guide

### Common Issues and Solutions

#### 1. **Container Won't Start**

```bash
# Check config file syntax
./target/debug/apollo-mcp-server /config/config.yaml --validate

# Check container logs
docker logs <container-id>

# Check file permissions
ls -la /config/
```

#### 2. **Auth0 Device Flow Fails**

```bash
# Test Auth0 connectivity
curl -X POST https://YOUR-DOMAIN.auth0.com/oauth/device/code \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "client_id=YOUR-CLIENT-ID&scope=openid profile email&audience=YOUR-AUDIENCE"

# Check client configuration in Auth0 dashboard
# - Application type must be "Native"
# - Device Code grant must be enabled
# - Audience must be configured
```

#### 3. **Session Storage Issues**

```bash
# File storage permission issues
chown apollo:apollo /data/sessions.json
chmod 644 /data/sessions.json

# Memory storage capacity issues
# Check container memory limits
docker stats <container-id>
```

#### 4. **GraphQL Authentication Fails**

```bash
# Check token validity
curl -H "Authorization: Bearer YOUR-TOKEN" https://api.example.com/graphql

# Check audience configuration
# Token audience must match GraphQL API audience
```

### Debug Logging Levels

```bash
# Error level - production
RUST_LOG=error

# Info level - standard operations
RUST_LOG=info

# Debug level - detailed operations
RUST_LOG=debug

# Trace level - very detailed (performance impact)
RUST_LOG=trace

# Module-specific debugging
RUST_LOG=apollo_mcp_server::auth=debug,apollo_mcp_server::server=info
```

This developer guide provides comprehensive information for working with the Phase 2 Auth0 implementation. For user-facing documentation, see the User Guide.