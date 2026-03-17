# Role-Based Multi-Schema Configuration Guide

## Overview

This guide explains how to configure the Apollo MCP Server with role-based routing for multi-schema GraphQL backends.

## Quick Start

### Minimal Configuration Example

```yaml
# config/roles-test-config.yaml

# Standard endpoint (used when roles are not configured)
endpoint: "https://api.example.com/graphql"

# Role-based routing configuration
roles:
  graphql_base_url: "https://api.example.com"
  available_roles:
    - reader
    - creator
    - approver
    - admin
  default_role: reader

# Auth0 Phase 2 configuration
auth0:
  domain: "mycompany.auth0.com"
  client_id: "your-client-id"
  audience: "https://api.example.com"
  per_session_auth:
    enabled: true
    device_flow_client_id: "your-device-flow-client-id"
    session_storage: memory

# Transport configuration
transport:
  type: streamable_http
  address: 0.0.0.0
  port: 5000

# Schema source (can use local or uplink)
schema:
  source: local
  path: /config/schema.graphql

# Operations (optional - will use introspection if not specified)
operations:
  source: introspect
```

## Configuration Options

### roles (Role-Based Routing)

Enables role-based routing and multi-schema support.

**Type**: `object` (optional)

#### Properties

##### `graphql_base_url`
**Type**: `string` (required)
**Format**: URL

Base URL for your GraphQL backend. The MCP server will append `/graphql/{role}` to this URL.

**Example**: `"https://api.it-ops.ai"`

##### `available_roles`
**Type**: `array of strings` (required)

List of roles to load schemas for. Each role will have its schema fetched from the GraphQL backend during startup.

**Example**:
```yaml
available_roles:
  - reader      # Read-only access
  - creator     # Can create resources
  - approver    # Can approve changes
  - admin       # Full administrative access
```

##### `default_role`
**Type**: `string` (required)

Default role to use when the client requests `/mcp` without specifying a role.

**Example**: `"reader"`

## Path-Based Role Routing

### How It Works

Clients specify the role in the URL path:

```
https://prod1.it-ops.ai/mcp/approver  → Uses "approver" role
https://prod1.it-ops.ai/mcp/admin     → Uses "admin" role
https://prod1.it-ops.ai/mcp           → Uses default role (reader)
```

The MCP server:
1. Extracts the role from the request path
2. Retrieves the cached schema for that role
3. Routes GraphQL requests to `/graphql/{role}` on your backend

### Backend Endpoint Mapping

```
Client Request Path          →  GraphQL Backend Endpoint
─────────────────────────────────────────────────────────
/mcp/reader                  →  /graphql/reader
/mcp/creator                 →  /graphql/creator
/mcp/approver                →  /graphql/approver
/mcp/admin                   →  /graphql/admin
/mcp                         →  /graphql/reader (default)
```

## Schema Loading

### Startup Behavior

When the MCP server starts with role-based configuration:

1. **Fetches schemas**: Executes introspection query against each role endpoint
2. **Caches schemas**: Stores schemas in memory for fast access
3. **Logs results**: Reports success/failure for each role
4. **Graceful fallback**: Continues if some schemas fail to load

### Example Startup Logs

```
INFO Loading role-based schemas from GraphQL backend
INFO   Base URL: https://api.it-ops.ai
INFO   Available roles: ["reader", "creator", "approver", "admin"]
INFO   Default role: reader
INFO Successfully loaded schemas for 4 roles
INFO   ✓ Schema loaded for role: reader
INFO   ✓ Schema loaded for role: creator
INFO   ✓ Schema loaded for role: approver
INFO   ✓ Schema loaded for role: admin
```

### Schema Refresh

- **No automatic refresh**: Schemas are loaded only on startup
- **To update schemas**: Restart the Docker container
- **Reasoning**: Schemas typically don't change frequently; restart provides predictable behavior

## Complete Configuration Example

```yaml
# config/production-roles-config.yaml

# GraphQL endpoint (fallback when roles not used)
endpoint: "https://api.it-ops.ai/graphql"

# Role-based multi-schema routing
roles:
  graphql_base_url: "https://api.it-ops.ai"
  available_roles:
    - reader      # Lowest privilege level
    - creator     # Can create resources
    - approver    # Can approve changes
    - admin       # Full administrative access
  default_role: reader

# Auth0 Phase 2 per-session authentication
auth0:
  domain: "${AUTH0_DOMAIN}"
  client_id: "${AUTH0_CLIENT_ID}"
  audience: "${AUTH0_AUDIENCE}"
  per_session_auth:
    enabled: true
    device_flow_client_id: "${AUTH0_DEVICE_FLOW_CLIENT_ID}"
    session_storage: memory
    token_refresh_buffer_seconds: 300
    device_flow_poll_interval_seconds: 5

# HTTP transport for web access
transport:
  type: streamable_http
  address: 0.0.0.0
  port: 5000
  auth:
    enabled: false  # Auth handled by Auth0, not HTTP basic auth

# Schema source
schema:
  source: local
  path: /config/default-schema.graphql

# Operations source
operations:
  source: introspect

# Health check configuration
health_check:
  enabled: true
  path: /health

# Logging configuration
logging:
  level: info
  format: json
  output:
    - stdout

# Introspection tools
introspection:
  execute:
    enabled: true
  validate:
    enabled: true
  introspect:
    enabled: true
    minify: false
  search:
    enabled: true
    minify: false
    leaf_depth: 3
    index_memory_bytes: 10485760  # 10 MB

# Overrides
overrides:
  mutation_mode: all
  disable_type_description: false
  disable_schema_description: false
  enable_explorer: false
```

## Docker Deployment

### Using Docker Compose

```yaml
# docker-compose.yml
version: '3.8'

services:
  apollo-mcp-roles-test:
    image: apollo-mcp-server:roles-test
    container_name: apollo-mcp-roles-test
    ports:
      - "5001:5000"  # Different port for testing
    volumes:
      - ./config/roles-test-config.yaml:/config/server-config.yaml:ro
    environment:
      # Auth0 credentials
      AUTH0_DOMAIN: "${AUTH0_DOMAIN}"
      AUTH0_CLIENT_ID: "${AUTH0_CLIENT_ID}"
      AUTH0_AUDIENCE: "${AUTH0_AUDIENCE}"
      AUTH0_DEVICE_FLOW_CLIENT_ID: "${AUTH0_DEVICE_FLOW_CLIENT_ID}"

      # Logging
      RUST_LOG: "info,apollo_mcp_server=debug"

    extra_hosts:
      - "host.docker.internal:host-gateway"
    restart: unless-stopped
    networks:
      - it-ops-network

networks:
  it-ops-network:
    external: true
```

### Running the Container

```bash
# Start the test server
docker-compose up -d apollo-mcp-roles-test

# View logs
docker-compose logs -f apollo-mcp-roles-test

# Check status
docker-compose ps apollo-mcp-roles-test

# Stop the server
docker-compose stop apollo-mcp-roles-test
```

## Environment Variables

All configuration options can be overridden with environment variables using double underscore (`__`) as a separator:

```bash
# Role configuration
APOLLO_MCP_ROLES__GRAPHQL_BASE_URL=https://api.example.com
APOLLO_MCP_ROLES__DEFAULT_ROLE=reader

# Auth0 configuration
APOLLO_MCP_AUTH0__DOMAIN=mycompany.auth0.com
APOLLO_MCP_AUTH0__CLIENT_ID=abc123
APOLLO_MCP_AUTH0__AUDIENCE=https://api.example.com
APOLLO_MCP_AUTH0__PER_SESSION_AUTH__ENABLED=true
APOLLO_MCP_AUTH0__PER_SESSION_AUTH__DEVICE_FLOW_CLIENT_ID=def456

# Transport
APOLLO_MCP_TRANSPORT__TYPE=streamable_http
APOLLO_MCP_TRANSPORT__PORT=5000
```

## Testing the Configuration

### 1. Verify Schema Loading

Check startup logs to ensure schemas loaded successfully:

```bash
docker logs apollo-mcp-roles-test 2>&1 | grep -A 10 "Loading role-based schemas"
```

Expected output:
```
INFO Loading role-based schemas from GraphQL backend
INFO Successfully loaded schemas for 4 roles
INFO   ✓ Schema loaded for role: reader
INFO   ✓ Schema loaded for role: creator
INFO   ✓ Schema loaded for role: approver
INFO   ✓ Schema loaded for role: admin
```

### 2. Test Role Routing

```bash
# Test reader role (default)
curl http://localhost:5001/mcp

# Test specific roles
curl http://localhost:5001/mcp/approver
curl http://localhost:5001/mcp/admin
```

### 3. Test with MCP Inspector

```bash
# Install MCP Inspector
npx @modelcontextprotocol/inspector

# Connect to your server
# Server URL: http://localhost:5001/mcp/approver
```

### 4. Verify Backend Routing

Check MCP server logs for routing messages:

```bash
docker logs apollo-mcp-roles-test 2>&1 | grep "Routing request to role-specific endpoint"
```

Expected output:
```
DEBUG Routing request to role-specific endpoint: role=approver, endpoint=https://api.it-ops.ai/graphql/approver
```

## Troubleshooting

### Schema Loading Failures

**Problem**: Some schemas fail to load

**Check**:
1. GraphQL backend endpoints are accessible
2. Introspection is enabled on the backend
3. No authentication required for introspection (or provide credentials)

**Logs to check**:
```bash
docker logs apollo-mcp-roles-test 2>&1 | grep "Failed to load schema"
```

### Role Not Found

**Problem**: Client requests unknown role

**Behavior**: Falls back to default role (reader)

**Logs**:
```
DEBUG Role 'unknown' not found in cache, using default role: reader
```

### Backend Connectivity Issues

**Problem**: Cannot connect to GraphQL backend

**Check**:
1. `graphql_base_url` is correct
2. Network connectivity from container to backend
3. DNS resolution works
4. Firewall rules allow connection

**Test connectivity**:
```bash
docker exec apollo-mcp-roles-test curl https://api.it-ops.ai/graphql/reader
```

## Security Considerations

### Authorization Model

**Important**: The MCP server does NOT enforce authorization. It only routes requests.

**Authorization Flow**:
1. User authenticates via Auth0 (Device Flow)
2. MCP server attaches Auth0 bearer token to requests
3. GraphQL backend validates token
4. GraphQL backend checks if user has permission for the requested role
5. GraphQL backend enforces access control

**Why this is secure**:
- Users can introspect any schema (harmless - just metadata)
- Actual operations require valid Auth0 token
- GraphQL backend determines if user can execute operations for that role
- Role in path selects which schema validates the query

### Network Security

**Recommendations**:
1. Use HTTPS for `graphql_base_url`
2. Use Auth0 for authentication (already configured)
3. Don't expose MCP server directly - use Cloudflare proxy
4. Configure firewall rules to restrict access
5. Use TLS for all connections

## Migration Guide

### From Single-Schema to Multi-Schema

**Before** (single schema):
```yaml
endpoint: "https://api.example.com/graphql"
```

**After** (multi-schema with roles):
```yaml
endpoint: "https://api.example.com/graphql"  # Keep as fallback

roles:
  graphql_base_url: "https://api.example.com"
  available_roles:
    - reader
    - admin
  default_role: reader
```

**Behavior**:
- Old clients (no role in path): Use fallback endpoint
- New clients (role in path): Use role-specific endpoint
- **Fully backward compatible**

## Advanced Configuration

### Custom Schema Sources

You can still use local schema files or Uplink registry alongside role-based routing:

```yaml
# Use local schema as fallback/default
schema:
  source: local
  path: /config/default-schema.graphql

# Role-based routing fetches schemas from backend
roles:
  graphql_base_url: "https://api.example.com"
  available_roles: [reader, admin]
  default_role: reader
```

### Health Checks with Role Validation

```yaml
health_check:
  enabled: true
  path: /health
  # Future: Add role-specific health checks
```

## Summary

**Key Points**:
- ✅ Configure `roles` section to enable multi-schema routing
- ✅ Schemas are fetched from GraphQL backend on startup
- ✅ Clients specify role in URL path (`/mcp/{role}`)
- ✅ MCP server routes to `/graphql/{role}` on backend
- ✅ Auth0 provides authentication, backend enforces authorization
- ✅ Fully backward compatible with single-schema deployments

**Next Steps**:
1. Create your configuration file
2. Start the Docker container
3. Verify schema loading in logs
4. Test with different roles
5. Deploy to production when ready