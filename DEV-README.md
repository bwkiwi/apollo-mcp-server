# Developer Notes - Apollo MCP Server Extensions

## Overview

This document captures key learnings and implementation notes from extending the Apollo MCP Server with Test Manager integration, Auth0 Phase 2, and Role-based routing.

## Recent Changes (v0.8.0-itops-testmgr)

### 1. Test Manager Integration

**Purpose**: Enables IT-Ops backend testing support through MCP tools for snapshot and test management.

**Architecture**:
- Thin Rust HTTP client wrapper in `crates/apollo-mcp-server/src/test_manager.rs`
- Business logic resides in IT-Ops Node.js backend at `localhost:5000`
- 10 MCP tools exposed for snapshot/test operations

**Key Components**:
```
src/test_manager.rs              # HTTP client + tool definitions
src/runtime/config.rs             # TestManagerConfig with fallback support
src/main.rs                       # Feature detection and initialization
src/server/states/running.rs     # Tool handlers and registration
```

**Configuration**:
```yaml
test_manager:
  enabled: true
  backend_url: "http://localhost:5000"  # Optional - auto-derived from endpoint
  fallback_description: "Fallback text when backend unavailable"
  timeout_ms: 5000
```

**Fallback Strategy**:
1. Check backend availability via `/api/test-mgr/enabled`
2. If available: Load MCP description from backend
3. If unavailable + fallback configured: Use config-based description
4. If unavailable + no fallback: Disable test manager entirely

**10 MCP Tools**:
- Snapshot: `snapshot_clear`, `snapshot_load`, `snapshot_save`, `snapshot_list`
- Tests: `test_get`, `test_save`, `test_update`, `test_save_result`
- MCP Control: `mcp_description_get`, `mcp_description_set`

### 2. Auth0 Phase 2 - Per-Session Authentication

**Purpose**: OAuth 2.0 device flow authentication per MCP session (replaces Phase 1's single-token approach).

**Key Changes**:
- Session-based token management (in-memory storage)
- Device flow for browser-based authentication
- 4 new MCP tools: `login`, `whoami`, `logout`, `getGraphQLToken`

**Architecture**:
```
src/auth/
├── session_manager.rs     # Session lifecycle + token storage
├── device_flow.rs         # OAuth device flow coordinator
├── auth_tools.rs          # MCP tool implementations
└── config.rs              # Auth0 configuration
```

**New Crate**: `itops-ai-auth` - Shared Auth0 client library

**Session Flow**:
1. Client calls `login` tool → Device flow initiated
2. User authenticates in browser → Session receives token
3. Subsequent requests use session token via `mcp-session-id` header
4. Token auto-refreshes when expired
5. Client calls `logout` to revoke session

### 3. Role-Based Routing

**Purpose**: Route GraphQL operations to role-specific endpoints (e.g., `/admin/graphql`, `/user/graphql`).

**Key Components**:
```
src/role_router.rs          # Endpoint construction
src/schema_loader.rs        # Multi-schema caching
src/server/role_config.rs   # Role configuration
```

**Configuration**:
```yaml
role_based_routing:
  enabled: true
  graphql_base_url: "http://localhost:5000"
  schema_dir: "./graphql/schemas"
```

**How It Works**:
1. Client includes role in request path or header
2. `role_router` constructs role-specific endpoint
3. `schema_loader` caches schemas per role
4. Request routed to appropriate backend endpoint

## Key Technical Learnings

### 1. MCP Tool Return Types

**Critical Pattern**: MCP tools must return `CallToolResult` with `Content` objects, not raw JSON.

❌ **Wrong**:
```rust
Ok(CallToolResult::from(vec![result]))  // Type mismatch!
```

✅ **Correct**:
```rust
Ok(CallToolResult {
    content: vec![Content::json(&result)
        .unwrap_or_else(|_| Content::text("Fallback message"))],
    is_error: Some(false),
})
```

**Import Required**:
```rust
use rmcp::model::{CallToolResult, Content, ErrorCode};
```

### 2. Endpoint Type Handling

The `Endpoint` newtype implements `Deref<Target = Url>`, so you can access URL methods directly:

❌ **Wrong**:
```rust
config.endpoint.as_url().origin()  // Method doesn't exist!
```

✅ **Correct**:
```rust
config.endpoint.origin()  // Deref to Url automatically
```

### 3. JSON Schema for MCP Tools

All MCP tool input types need `JsonSchema` derive for proper validation:

```rust
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SaveSnapshotParams {
    #[serde(rename = "snapshotName")]
    pub snapshot_name: String,
    pub description: String,
    // ... other fields
}
```

**Required Derives**: `Debug`, `Serialize`, `Deserialize`, `JsonSchema`

### 4. Async HTTP Client Pattern

For external service integration (like Test Manager):

```rust
pub struct TestManagerTools {
    client: Client,           // reqwest::Client
    base_url: String,
    fallback_description: Option<String>,
}

impl TestManagerTools {
    pub async fn detect_features(&self) -> bool {
        // Check if backend is available
        match self.client.get(&url).send().await {
            Ok(response) => /* parse response */,
            Err(_) => false  // Graceful degradation
        }
    }
}
```

**Pattern**: Always provide fallback behavior for external dependencies.

### 5. State Machine Integration

The server uses a state machine architecture. New features integrate through:

1. **Config State**: Add fields to `Config` struct
2. **Starting State**: Initialize components, pass to Running
3. **Running State**: Handle tool calls, register tools in `list_tools()`

**Example**:
```rust
// In server/states.rs
pub(super) struct Config {
    pub(super) test_manager: Option<Arc<TestManagerTools>>,
    // ... other fields
}

// In server/states/running.rs
impl Running {
    async fn call_tool(&self, request: CallToolRequestParam) -> Result<CallToolResult> {
        match request.name.as_ref() {
            SNAPSHOT_CLEAR_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                // Handle tool call
            }
            // ... other tools
        }
    }
}
```

## Docker Build Notes

### Multi-Stage Build

The Dockerfile uses a two-stage build:
1. **Builder stage**: Rust 1.83 on Debian Bookworm
2. **Runtime stage**: Minimal Debian Bookworm Slim (174MB final image)

### Build Command
```bash
docker build \
  -t apollo-mcp-server:v0.8.0-itops-testmgr \
  -t apollo-mcp-server:latest-itops \
  -t apollo-mcp-server:test-manager \
  .
```

### Common Build Issues

**Issue 1: Type Mismatches with CallToolResult**
- **Error**: `expected CallToolResult, found Vec<Value>`
- **Fix**: Use `Content::json()` wrapper (see "MCP Tool Return Types" above)

**Issue 2: Missing Trait Bounds**
- **Error**: `the trait bound 'Foo: Deserialize' is not satisfied`
- **Fix**: Add `#[derive(Deserialize, JsonSchema)]` to all parameter structs

**Issue 3: Endpoint Method Not Found**
- **Error**: `no method named 'as_url' found for struct 'Endpoint'`
- **Fix**: Use `Deref` trait - access URL methods directly

## Configuration Patterns

### Feature Toggle Pattern

```yaml
# Feature with fallback
test_manager:
  enabled: true
  backend_url: "http://localhost:5000"
  fallback_description: |
    Fallback description when backend unavailable
```

### Initialization Pattern

```rust
// In main.rs
let test_manager = if config.test_manager.enabled {
    let tools = TestManagerTools::new(/* ... */);
    if tools.detect_features().await {
        Some(Arc::new(tools))  // Backend available
    } else if config.test_manager.fallback_description.is_some() {
        Some(Arc::new(tools))  // Use fallback
    } else {
        None  // Disable entirely
    }
} else {
    None
};
```

## Testing Recommendations

### Local Testing Setup

1. **Start IT-Ops Backend**:
   ```bash
   cd /path/to/it-ops/server
   npm run dev  # Listens on localhost:5000
   ```

2. **Configure MCP Server**:
   ```yaml
   # config/test-config.yaml
   endpoint: http://localhost:5000/graphql
   test_manager:
     enabled: true
   ```

3. **Run MCP Server**:
   ```bash
   cargo run -- config/test-config.yaml
   ```

### Docker Testing

```bash
# Build
docker build -t apollo-mcp-server:test .

# Run with config
docker run -v $(pwd)/config:/config \
  apollo-mcp-server:test /config/test-config.yaml
```

## Code Quality Standards

### Clippy Lints (Strict)

The workspace enforces strict linting:
```toml
[workspace.lints.clippy]
exit = "deny"
expect_used = "deny"
indexing_slicing = "deny"
unwrap_used = "deny"
panic = "deny"
```

**Pattern**: Always use `Result` types and proper error handling. No `unwrap()` in production code!

### Error Handling Pattern

```rust
// Use .ok_or() for Option to Result conversion
let value = option_value.ok_or(McpError::new(
    ErrorCode::INVALID_PARAMS,
    "Parameter required",
    None::<Value>
))?;

// Use .map_err() for error conversion
let result = external_call().await
    .map_err(|e| format!("Failed to call: {}", e))?;
```

## Workspace Structure

```
crates/
├── apollo-mcp-server/      # Main MCP server
│   ├── src/
│   │   ├── auth/           # Auth0 Phase 2 components
│   │   ├── test_manager.rs # Test Manager integration
│   │   ├── role_router.rs  # Role-based routing
│   │   └── schema_loader.rs # Multi-schema support
├── apollo-mcp-registry/    # GraphOS integration
├── apollo-schema-index/    # Schema search/indexing
└── itops-ai-auth/          # Shared Auth0 library (NEW)
```

## Next Steps for Developers

### Backend Implementation Required

The Test Manager Rust integration is complete but requires the IT-Ops Node.js backend:

**Endpoints to Implement**:
- `GET /api/test-mgr/enabled` - Feature detection
- `GET /api/test-mgr/mcp-description` - Get MCP description
- `POST /api/test-mgr/mcp-description` - Set MCP description
- `POST /api/test-mgr/snapshot/clear` - Clear Layer 2 data
- `POST /api/test-mgr/snapshot/load` - Load snapshot
- `POST /api/test-mgr/snapshot/save` - Save snapshot
- `GET /api/test-mgr/snapshot/list` - List snapshots
- `GET /api/test-mgr/test/:id` - Get test definition
- `POST /api/test-mgr/test` - Create test
- `PUT /api/test-mgr/test/:id` - Update test
- `POST /api/test-mgr/test/:id/result` - Save test result

See `TEST_MANAGER_IMPLEMENTATION.md` for detailed backend requirements.

## Version History

- **v0.8.0-itops-testmgr**: Test Manager + Auth0 Phase 2 + Role-based routing
- **v0.7.1-itops-auth0-roles**: Initial role-based routing
- **v0.7.0**: Base Apollo MCP Server

## References

- [MCP Protocol Spec](https://spec.modelcontextprotocol.io/)
- [Apollo Compiler](https://github.com/apollographql/apollo-rs)
- [Auth0 Device Flow](https://auth0.com/docs/get-started/authentication-and-authorization-flow/device-authorization-flow)

## Questions or Issues?

Check existing documentation:
- `TEST_MANAGER_IMPLEMENTATION.md` - Test Manager details
- `ROLE_BASED_ROUTING_IMPLEMENTATION.md` - Role routing guide
- `docs/ROLE_BASED_CONFIGURATION_GUIDE.md` - Configuration reference
- `it-ops.ai_Developer_Guide.md` - IT-Ops integration guide
