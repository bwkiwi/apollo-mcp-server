# Troubleshooting Guide - Apollo MCP Server

## Server Not Responding to HTTP Requests

### Symptoms
- Server shows port is open (e.g., listening on port 3100)
- HTTP requests to `/mcp`, `/health`, or any endpoint timeout
- No errors in logs
- No response at all

### Root Cause
The server may be **hanging during startup** when trying to connect to external services.

### Common Issues

#### 1. Role-Based Schema Loading Timeout

**Problem**: If `roles` configuration is enabled, the server tries to fetch schemas from the GraphQL backend during startup. If those endpoints don't exist or are slow, the server hangs.

**Check your config**:
```yaml
roles:
  graphql_base_url: "https://api.it-ops.ai"  # ← Does this respond?
  available_roles:
    - reader
    - creator
    - approver
    - admin
```

**What happens**:
1. Server starts
2. Tries to fetch schema from `https://api.it-ops.ai/graphql/reader`
3. Tries to fetch schema from `https://api.it-ops.ai/graphql/creator`
4. ... and so on for each role
5. If any endpoint doesn't respond, server hangs (with timeout, it will fail after 10s per role)

**Solutions**:

**Option A: Disable role-based routing**
```yaml
# Comment out or remove the roles section
# roles:
#   graphql_base_url: "https://api.it-ops.ai"
#   available_roles: [...]
```

**Option B: Use only available endpoints**
```yaml
roles:
  graphql_base_url: "https://api.it-ops.ai"
  available_roles:
    - reader  # Only include roles that actually exist
```

**Option C: Ensure backend endpoints exist**
Make sure these endpoints respond:
- `https://api.it-ops.ai/graphql/reader` (POST with introspection query)
- `https://api.it-ops.ai/graphql/creator`
- `https://api.it-ops.ai/graphql/approver`
- `https://api.it-ops.ai/graphql/admin`

#### 2. Test Manager Backend Timeout

**Problem**: If `test_manager.enabled: true`, the server tries to connect to the Test Manager backend during startup.

**Check your config**:
```yaml
test_manager:
  enabled: true
  backend_url: "http://localhost:5000"  # ← Is this running?
```

**What happens**:
1. Server starts
2. Tries to check `http://localhost:5000/api/test-mgr/enabled`
3. Tries to fetch MCP description from `http://localhost:5000/api/test-mgr/mcp-description`
4. If backend doesn't respond, uses fallback or disables feature

**Solutions**:

**Option A: Disable test manager**
```yaml
test_manager:
  enabled: false
```

**Option B: Add fallback description**
```yaml
test_manager:
  enabled: true
  backend_url: "http://localhost:5000"
  fallback_description: |
    Test Manager unavailable
```

**Option C: Start the IT-Ops backend**
```bash
cd /path/to/it-ops/server
npm run dev  # Starts on localhost:5000
```

#### 3. Auth0 Device Flow Issues

**Problem**: If Auth0 per-session auth is enabled, device flow initialization might hang.

**Check your config**:
```yaml
auth0:
  domain: "your-tenant.auth0.com"
  per_session_auth:
    enabled: true
    device_flow_client_id: "..."
```

**Solutions**:

**Option A: Disable per-session auth**
```yaml
auth0:
  domain: "your-tenant.auth0.com"
  # Remove or comment out per_session_auth
```

**Option B: Use Phase 1 auth instead**
```yaml
auth0:
  domain: "your-tenant.auth0.com"
  client_id: "..."
  refresh_token: "..."  # Phase 1 token-based auth
```

## Debugging Steps

### 1. Check Server Logs

Look for these log messages to see where startup is hanging:

```
✅ Good signs (server is progressing):
- "Apollo MCP Server v0.8.0-itops-testmgr"
- "✅ Test Manager backend detected"
- "✅ Successfully loaded schemas for N roles"
- "Server listening on..."

⚠️ Warning signs (might be slow):
- "⏳ Fetching schemas for N roles (this may take a moment)..."
- "🔍 Detecting Test Manager backend at..."
- "📡 Sending introspection request to..."

❌ Bad signs (something failed):
- "❌ Failed to send request to..."
- "❌ Test Manager backend not available"
- "⚠️ Failed to load role-based schemas"
```

### 2. Test Backend Availability

Before starting the MCP server, test if backends are reachable:

**Test Role-Based Schema Endpoint**:
```bash
curl -X POST https://api.it-ops.ai/graphql/reader \
  -H "Content-Type: application/json" \
  -d '{"query": "{ __schema { queryType { name } } }"}'
```

**Test Manager Endpoint**:
```bash
curl http://localhost:5000/api/test-mgr/enabled
```

### 3. Minimal Configuration

Start with a minimal config to isolate the issue:

```yaml
# minimal-config.yaml
endpoint: "http://localhost:4000/graphql"

transport:
  type: streamable_http
  address: 127.0.0.1
  port: 3100

schema:
  type: local
  path: ./path/to/schema.graphql

operations:
  type: introspect

# Everything else disabled
introspection:
  execute:
    enabled: true
```

Then add features one by one:
1. Add `test_manager` → Test if server starts
2. Add `roles` → Test if server starts
3. Add `auth0` → Test if server starts

### 4. Enable Debug Logging

Set environment variable:
```bash
RUST_LOG=debug cargo run -- config.yaml
```

Or in config:
```yaml
logging:
  level: debug
```

### 5. Check for Port Conflicts

```bash
# Check if port 3100 is already in use
lsof -i :3100
# or
netstat -an | grep 3100
```

## Configuration Checklist

Before starting the server, verify:

- [ ] All `graphql_base_url` endpoints are accessible
- [ ] All `available_roles` have corresponding backend endpoints
- [ ] Test Manager `backend_url` is running (if enabled)
- [ ] Auth0 credentials are valid (if using auth)
- [ ] Schema file exists at specified `path` (if using local schema)
- [ ] Port specified in `transport` is not in use
- [ ] No firewall blocking outbound connections to external services

## Quick Fixes

### Disable All External Dependencies

```yaml
# Safe minimal config - no external dependencies
endpoint: "http://localhost:4000/graphql"

transport:
  type: streamable_http
  address: 127.0.0.1
  port: 3100

schema:
  type: local
  path: ./schema.graphql

operations:
  type: introspect

introspection:
  execute:
    enabled: true
  introspect:
    enabled: true

# Everything else commented out or removed
```

### Increase Timeouts

If backends are slow but working:

```yaml
test_manager:
  enabled: true
  timeout_ms: 30000  # Increase from default 5000ms to 30 seconds
```

Note: Schema loader now has a 10-second timeout per role (hardcoded).

## Recent Changes (v0.8.0-itops-testmgr)

### Bug Fixes
- ✅ Added 10-second timeout to schema loader HTTP client
- ✅ Added detailed emoji-based logging for visibility
- ✅ Added error logging for failed HTTP requests

### Known Limitations
- Schema loader timeout is hardcoded to 10 seconds
- No retry logic for failed schema fetches
- No parallel loading of schemas (sequential)

## Getting Help

1. Check logs with `RUST_LOG=debug`
2. Test backend endpoints manually with `curl`
3. Use minimal config to isolate issue
4. Check `DEV-README.md` for technical details
5. Review config examples in `config/` directory

## Environment-Specific Issues

### Docker
- Ensure `localhost` references work (use `host.docker.internal` on Mac/Windows)
- Check network connectivity to external services
- Verify mounted volumes for config files

### WSL/Linux
- Check firewall rules
- Verify DNS resolution for external domains
- Test connectivity with `curl` or `wget`

### macOS
- Check network permissions
- Verify firewall isn't blocking connections
- Test with `host.docker.internal` for Docker Desktop

## Performance Notes

**Startup Time Expectations**:
- Minimal config (no external deps): < 1 second
- With Test Manager (backend available): 2-3 seconds
- With role-based routing (4 roles): 5-10 seconds per role = 20-40 seconds
- If backend timeouts occur: Add 10 seconds per timeout

**Recommendation**: Disable features you don't need to speed up startup.
