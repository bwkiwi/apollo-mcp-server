# Apollo MCP Server Phase 2 Auth0 - User Guide

## Overview

The Apollo MCP Server with Phase 2 Auth0 authentication provides secure, per-session authentication for GraphQL operations within Claude Desktop. This guide walks you through setup, configuration, and daily usage.

## What's New in Phase 2

Phase 2 introduces **per-session authentication**, meaning each Claude Desktop session gets its own secure authentication. This is a significant improvement over Phase 1's shared authentication model.

### Benefits of Phase 2

✅ **Individual Authentication** - Each user authenticates with their own credentials  
✅ **Secure Device Flow** - No shared secrets or tokens  
✅ **Automatic Token Management** - Tokens refresh automatically before expiration  
✅ **Session Persistence** - Authentication survives server restarts  
✅ **User-Friendly** - Simple browser-based authentication flow  

## Prerequisites

### Auth0 Setup Requirements

1. **Auth0 Account** with admin access
2. **Native Application** configured for device flow
3. **GraphQL API** registered in Auth0
4. **Proper Permissions** for users and application

### Technical Requirements

1. **Docker** installed and running
2. **Node.js** for MCP client tools
3. **Claude Desktop** application
4. **Network Access** to Auth0 and your GraphQL API

## Quick Start Guide

### Step 1: Configure Auth0

#### 1.1 Create Native Application

1. **Log into Auth0 Dashboard** → Applications
2. **Click "Create Application"**
3. **Name:** `Apollo MCP Device Flow`
4. **Type:** Select **"Native"**
5. **Click "Create"**

#### 1.2 Configure Application Settings

1. **Copy the Client ID** (save for later use)
2. **Go to Settings tab**
3. **Scroll to "Advanced Settings"** → **"Grant Types"**
4. **Ensure "Device Code" is checked** ✅
5. **Click "Save Changes"**

#### 1.3 Verify API Configuration

1. **Go to APIs section** in Auth0 Dashboard
2. **Find your GraphQL API** (or create one)
3. **Note the Identifier** (this is your audience)
4. **Ensure proper scopes** are configured

### Step 2: Deploy Apollo MCP Server

#### 2.1 Create Configuration File

Create `config.yaml` with your Auth0 settings:

```yaml
# Apollo MCP Server Configuration
endpoint: "https://your-graphql-api.example.com/graphql"

# Transport configuration
transport:
  type: streamable_http
  address: "0.0.0.0"
  port: 5000

# Schema source
schema:
  local:
    path: "/config/schema.graphql"
  # OR use introspection:
  # introspect:
  #   endpoint: "https://your-graphql-api.example.com/graphql"

# Operations source
operations:
  introspect: {}

# Auth0 Configuration - REPLACE WITH YOUR VALUES
auth0:
  domain: "your-tenant.auth0.com"                    # Your Auth0 domain
  client_id: "your-client-id"                        # Application Client ID
  audience: "https://your-graphql-api.example.com"   # API Identifier
  
  per_session_auth:
    enabled: true
    device_flow_client_id: "your-native-app-client-id"  # Native Application Client ID
    session_storage:
      type: file
      path: "/data/sessions.json"
    token_refresh_buffer_seconds: 300
    device_flow_poll_interval_seconds: 5
    device_flow_timeout_seconds: 600

# Enable tools
introspection:
  execute: { enabled: true }
  introspect: { enabled: true }
  search: { enabled: true }

# Health check
health_check:
  enabled: true
  path: "/health"
```

#### 2.2 Create Schema File (if using local schema)

Create `schema.graphql` with your GraphQL schema:

```graphql
type Query {
  # Your actual schema here
  users: [User!]!
  profile: UserProfile
}

type User {
  id: ID!
  name: String!
  email: String!
}

type UserProfile {
  id: ID!
  displayName: String!
  preferences: JSON
}
```

#### 2.3 Deploy with Docker

```bash
# Create directory structure
mkdir -p apollo-mcp/{config,data}
cd apollo-mcp

# Place your config.yaml and schema.graphql in config/
cp config.yaml config/
cp schema.graphql config/

# Run the container
docker run -d \
  --name apollo-mcp \
  -p 5000:5000 \
  -v $(pwd)/config:/config:ro \
  -v apollo-sessions:/data \
  -e RUST_LOG=info \
  -e NO_COLOR=1 \
  apollo-mcp-server:latest \
  /config/config.yaml

# Check if running
docker ps
docker logs apollo-mcp
```

#### 2.4 Verify Deployment

```bash
# Test health endpoint
curl http://localhost:5000/health

# Test MCP endpoint
curl -X POST http://localhost:5000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'
```

### Step 3: Configure Claude Desktop

#### 3.1 Install MCP Client

```bash
# Install the MCP remote client
npm install -g mcp-remote
```

#### 3.2 Configure Claude Desktop

Update your Claude Desktop configuration file:

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`  
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "apollo-graphql": {
      "command": "npx",
      "args": [
        "mcp-remote",
        "http://localhost:5000/mcp",
        "--allow-http",
        "--debug"
      ]
    }
  }
}
```

#### 3.3 Restart Claude Desktop

1. **Quit Claude Desktop completely**
2. **Restart Claude Desktop**
3. **Start a new conversation**

## Daily Usage

### Authentication Workflow

#### 1. **Check Available Tools**

In Claude Desktop, ask:
```
What tools do you have available?
```

You should see 7 tools:
- **login** - Start authentication
- **whoami** - Check authentication status
- **logout** - Sign out
- **getGraphQLToken** - Get token for external use
- **execute** - Run GraphQL queries
- **introspect** - Explore GraphQL schema
- **search** - Search GraphQL schema

#### 2. **Start Authentication**

```
Please use the login tool to authenticate me
```

**Expected Response:**
```json
{
  "status": "login_initiated",
  "message": "🔐 Authentication Required\n\nPlease visit:\nhttps://your-tenant.auth0.com/activate\n\nAnd enter code: ABCD-EFGH\n\nOr click this direct link:\nhttps://your-tenant.auth0.com/activate?user_code=ABCD-EFGH\n\nThis code expires in 600 seconds...",
  "verification_uri": "https://your-tenant.auth0.com/activate",
  "user_code": "ABCD-EFGH",
  "expires_in": 600,
  "instructions": [
    "1. Visit https://your-tenant.auth0.com/activate",
    "2. Enter code: ABCD-EFGH",
    "3. Complete the authentication in your browser",
    "4. Return here - authentication will complete automatically"
  ]
}
```

#### 3. **Complete Authentication in Browser**

1. **Click the direct link** or visit the verification URI
2. **Enter the user code** when prompted (e.g., `ABCD-EFGH`)
3. **Sign in** with your Auth0 credentials
4. **Grant permissions** to the application when prompted
5. **Return to Claude Desktop** - authentication will complete automatically

#### 4. **Verify Authentication**

```
Please use the whoami tool to check my authentication status
```

**When Authenticated:**
```json
{
  "authenticated": true,
  "session_id": "default-session",
  "user": {
    "sub": "auth0|123456789",
    "email": "user@example.com",
    "name": "John Doe",
    "groups": ["admin", "users"],
    "permissions": ["read:data", "write:data"]
  },
  "status": "✅ Authenticated"
}
```

**When Not Authenticated:**
```json
{
  "authenticated": false,
  "session_id": "default-session",
  "message": "Not authenticated. Use the 'login' tool to authenticate.",
  "status": "❌ Not Authenticated"
}
```

### Using GraphQL Operations

#### 1. **Basic Query Execution**

```
Please execute this GraphQL query: { __typename }
```

```
Please execute this GraphQL query:
{
  users {
    id
    name
    email
  }
}
```

#### 2. **Query with Variables**

```
Please execute this GraphQL query with variables:
query: 
{
  user(id: $userId) {
    id
    name
    email
  }
}
variables:
{
  "userId": "123"
}
```

#### 3. **Schema Exploration**

```
Please use the introspect tool to show me the Query type
```

```
Please use the search tool to find all types related to "user"
```

### Advanced Usage

#### 1. **Get Token for External Tools**

```
Please use the getGraphQLToken tool to get my authentication token
```

**Response:**
```json
{
  "token": "Bearer eyJhbGciOiJSUzI1NiIs...",
  "session_id": "default-session",
  "usage": {
    "apollo_explorer": "Add this as 'Authorization' header in Apollo Explorer",
    "curl": "curl -H 'Bearer eyJhbGciOiJSUzI1NiIs...' https://your-api.example.com/graphql"
  },
  "warning": "⚠️ This token will expire soon - use it quickly!",
  "expires_note": "Token automatically refreshes for MCP operations"
}
```

#### 2. **Use Token in Apollo Explorer**

1. **Open Apollo Studio Explorer**
2. **Go to Connection Settings** (headers section)
3. **Add header:**
   - **Name:** `Authorization`
   - **Value:** `Bearer eyJhbGciOiJSUzI1NiIs...` (from getGraphQLToken)
4. **Test a query** - should work with your authenticated token

#### 3. **Use Token with curl**

```bash
curl -X POST https://your-graphql-api.example.com/graphql \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer eyJhbGciOiJSUzI1NiIs..." \
  -d '{"query": "{ __typename }"}'
```

#### 4. **Sign Out**

```
Please use the logout tool to sign me out
```

**Response:**
```json
{
  "status": "logged_out",
  "message": "✅ Successfully logged out",
  "session_id": "default-session"
}
```

## Session Management

### Session Persistence

- **Memory Storage**: Sessions lost when container restarts
- **File Storage**: Sessions persist across container restarts
- **Automatic Cleanup**: Expired sessions are automatically removed

### Token Lifecycle

1. **Initial Authentication**: Device flow creates access and refresh tokens
2. **Automatic Refresh**: Tokens refresh 5 minutes before expiration
3. **Session Expiry**: Sessions expire based on Auth0 settings
4. **Re-authentication**: Users must log in again after session expiry

### Multiple Sessions

Each Claude Desktop instance gets its own session:
- **Desktop A**: User Alice's authentication
- **Desktop B**: User Bob's authentication  
- **Same Desktop, New Chat**: Maintains same authentication

## Troubleshooting

### Common Issues

#### 1. **Tools Not Appearing**

**Symptoms:** Claude Desktop doesn't show auth tools

**Solutions:**
```bash
# Check server logs
docker logs apollo-mcp

# Verify MCP connection
curl -X POST http://localhost:5000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

# Restart Claude Desktop completely
```

#### 2. **Authentication Fails**

**Symptoms:** Login tool returns errors

**Solutions:**
- **Check Auth0 configuration** in dashboard
- **Verify device flow client ID** is correct
- **Ensure "Device Code" grant** is enabled
- **Check network connectivity** to Auth0

#### 3. **GraphQL Queries Fail**

**Symptoms:** Execute tool returns authentication errors

**Solutions:**
- **Check token validity** with whoami tool
- **Verify API audience** matches GraphQL API
- **Re-authenticate** if session expired
- **Check GraphQL API** accepts Auth0 tokens

#### 4. **Container Won't Start**

**Symptoms:** Docker container exits immediately

**Solutions:**
```bash
# Check configuration syntax
docker run -it --rm -v $(pwd)/config:/config:ro apollo-mcp-server:latest /config/config.yaml --validate

# Check file permissions
ls -la config/

# Check container logs
docker logs apollo-mcp
```

### Debug Information

#### Health Check Endpoints

```bash
# Server health
curl http://localhost:5000/health

# OAuth metadata
curl http://localhost:5000/.well-known/oauth-protected-resource
```

#### Useful Log Commands

```bash
# Follow server logs
docker logs -f apollo-mcp

# Check last 50 lines
docker logs --tail 50 apollo-mcp

# Search for specific errors
docker logs apollo-mcp 2>&1 | grep -i "error\|auth"
```

## Best Practices

### Security

1. **Use HTTPS** for production deployments
2. **Rotate Auth0 secrets** regularly
3. **Monitor authentication logs** in Auth0 dashboard
4. **Use file storage** for session persistence
5. **Implement proper access controls** in your GraphQL API

### Performance

1. **Use memory storage** for development/testing
2. **Use file storage** for production
3. **Monitor session storage size** for cleanup
4. **Set appropriate token refresh intervals**

### Monitoring

1. **Monitor health endpoints** regularly
2. **Set up alerts** for authentication failures
3. **Track session creation/expiry** patterns
4. **Monitor GraphQL API** authentication metrics

## Support and Resources

### Getting Help

1. **Check server logs** first for error details
2. **Verify Auth0 configuration** in dashboard
3. **Test individual components** (health check, MCP protocol)
4. **Check Claude Desktop logs** for client-side issues

### Useful Resources

- **Auth0 Dashboard**: Manage applications and view logs
- **Apollo Studio**: Test GraphQL queries with tokens
- **Claude Desktop Logs**: Debug MCP communication issues
- **Container Logs**: Server-side debugging information

### Configuration Examples

See the repository for complete configuration examples:
- `config/server-config.yaml` - Production configuration template
- `config/dev-config.yaml` - Development configuration
- `docker-compose.yml` - Container orchestration example

This user guide provides everything needed to successfully deploy and use Apollo MCP Server with Phase 2 Auth0 authentication. For technical implementation details, see the Developer Guide.