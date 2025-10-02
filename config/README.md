# Apollo MCP Server Configuration

## Quick Start

1. **Update the configuration in `server-config.yaml`:**
   - Replace `your-tenant.auth0.com` with your Auth0 domain
   - Replace `your-auth0-client-id` with your Auth0 application client ID
   - Replace `your-device-flow-client-id` with your Auth0 Native application client ID
   - Replace `your-graphql-api.example.com` with your GraphQL API endpoint
   - Replace `your-apollo-studio-api-key` with your Apollo Studio API key (if using)
   - Replace `your-graph@current` with your graph reference (if using)

2. **Build and start the container:**
   ```bash
   docker-compose up -d
   ```

3. **Configure Claude Desktop:**
   ```json
   {
     "mcpServers": {
       "apollo-graphql": {
         "command": "npx",
         "args": [
           "@modelcontextprotocol/server-http-proxy", 
           "http://your-server.com:5000/mcp"
         ]
       }
     }
   }
   ```

## Available Endpoints

- **MCP Protocol:** `http://your-server:5000/mcp`
- **Health Check:** `http://your-server:5000/health`
- **OAuth Metadata:** `http://your-server:5000/.well-known/oauth-protected-resource`

## Configuration Options

### Schema Sources
- `uplink: {}` - Use Apollo Studio schema
- `local: { path: "/config/schema.graphql" }` - Use local schema file

### Operation Sources  
- `introspect: {}` - Generate from schema introspection
- `collection: { id: "default" }` - Use Apollo Studio collection
- `local: { paths: ["/config/operations"] }` - Use local operation files

### Session Storage
- `memory` - In-memory (lost on restart)
- `file: { path: "/data/sessions.json" }` - Persistent file storage

## Volumes

- `/config` - Configuration files (read-only)
- `/data` - Session storage and runtime data (persistent)

## Environment Variables

- `RUST_LOG` - Log level (error, warn, info, debug)
- `NO_COLOR` - Disable colored output (required for MCP)
- `APOLLO_MCP_TRANSPORT__*` - Override transport settings