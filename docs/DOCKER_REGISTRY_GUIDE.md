# Docker Registry and Build Guide

This guide covers building and deploying the Apollo MCP Server Docker image to the GitHub Container Registry (ghcr.io).

## Quick Start

**Linux/WSL (Bash):**
```bash
./scripts/build-docker.sh           # Build only
./scripts/build-docker.sh --push    # Build and push
```

**Windows (PowerShell):**
```powershell
.\scripts\build-docker.ps1          # Build only
.\scripts\build-docker.ps1 -Push    # Build and push
```

## Prerequisites

- Docker installed and running
- GitHub Personal Access Token (PAT) with `write:packages` scope
- Access to the target container registry

## Environment Setup

### 1. Configure Credentials

Copy the example environment file and add your credentials:

```bash
cp .env.example .env
```

Edit `.env` with your GitHub credentials:

```bash
# .env
GITHUB_USERNAME=bwkiwi
GITHUB_PAT=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
CONTAINER_REGISTRY=ghcr.io
CONTAINER_REPO=ghcr.io/bwkiwi/apollo-mcp-server
```

**IMPORTANT**: Never commit `.env` to git. It's already in `.gitignore`.

---

## Build Versioning

The build script automatically generates version tags using git information:

| Tag Format | Example | Purpose |
|------------|---------|---------|
| `latest` | `latest` | Most recent build |
| `{cargo-version}` | `0.8.0-itops-testmgr` | Cargo.toml version |
| `{date}-{hash}` | `20251221-a1b2c3d` | Date + git commit |
| `{version}-{date}-{hash}` | `0.8.0-itops-testmgr-20251221-a1b2c3d` | Full version |

Example output:
```
==============================================
Apollo MCP Server - Docker Build
==============================================
Cargo Version : 0.8.0-itops-testmgr
Git Commit    : a1b2c3d
Git Branch    : main
Build Date    : 20251221
Build Tags    :
  - latest
  - 0.8.0-itops-testmgr
  - 20251221-a1b2c3d
  - 0.8.0-itops-testmgr-20251221-a1b2c3d
==============================================
```

---

## Building the Docker Image

### Using the Build Script (Recommended)

```bash
# Build with all version tags
./scripts/build-docker.sh

# Build and push to registry
./scripts/build-docker.sh --push
```

### From WSL or Linux (Manual)

```bash
cd /mnt/c/workspace/apollo-mcp-server
# or
cd ~/workspace/apollo-mcp-server

# Load environment variables
source .env

# Build with a specific tag
docker build -t apollo-mcp-server:auth-bypass .

# Or build with multiple tags
docker build \
  -t apollo-mcp-server:latest \
  -t apollo-mcp-server:v0.8.0-itops \
  .
```

### Available Image Tags

| Tag | Purpose |
|-----|---------|
| `latest` | Most recent stable build |
| `auth-bypass` | Development build with auth bypass enabled |
| `introspection-schema` | Build with introspection schema support |
| `v0.8.0-itops` | Versioned release |

---

## Pushing to GitHub Container Registry

### 1. Login to Registry

```bash
# Load credentials from .env
source .env

# Login to ghcr.io
echo "$GITHUB_PAT" | docker login ghcr.io -u $GITHUB_USERNAME --password-stdin
```

### 2. Tag the Image

```bash
# Tag for the registry
docker tag apollo-mcp-server:auth-bypass ghcr.io/bwkiwi/apollo-mcp-server:auth-bypass

# Or tag multiple versions
docker tag apollo-mcp-server:latest ghcr.io/bwkiwi/apollo-mcp-server:latest
docker tag apollo-mcp-server:latest ghcr.io/bwkiwi/apollo-mcp-server:v0.8.0
```

### 3. Push to Registry

```bash
# Push specific tag
docker push ghcr.io/bwkiwi/apollo-mcp-server:auth-bypass

# Push all tags
docker push ghcr.io/bwkiwi/apollo-mcp-server:latest
docker push ghcr.io/bwkiwi/apollo-mcp-server:v0.8.0
```

---

## Deploying to Demo Server

### 1. SSH to Server

```bash
ssh user@demo.it-ops.ai
```

### 2. Login to Registry (on server)

```bash
sudo -i

# Set PAT (or source from a secure location)
export CR_PAT='ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'

# Login
echo "$CR_PAT" | docker login ghcr.io -u bwkiwi --password-stdin
```

### 3. Pull the Image

```bash
docker pull ghcr.io/bwkiwi/apollo-mcp-server:auth-bypass

# Or pull specific version
docker pull ghcr.io/bwkiwi/apollo-mcp-server:introspection-schema
```

### 4. Start the Service

```bash
# Using docker-compose
docker compose -f backend.compose.yml up -d apollo-mcp

# Or start individual services
docker compose -f backend.compose.yml up -d redis-dev1
docker compose -f backend.compose.yml up -d apollo-mcp-server
```

---

## Quick Reference Commands

### Build and Push (Complete Workflow)

```bash
# From WSL
cd /mnt/c/workspace/apollo-mcp-server
source .env

# Build
docker build -t apollo-mcp-server:auth-bypass .

# Login, tag, and push
echo "$GITHUB_PAT" | docker login ghcr.io -u $GITHUB_USERNAME --password-stdin
docker tag apollo-mcp-server:auth-bypass $CONTAINER_REPO:auth-bypass
docker push $CONTAINER_REPO:auth-bypass
```

### Deploy (On Demo Server)

```bash
sudo -i
export CR_PAT='your-pat-here'
echo "$CR_PAT" | docker login ghcr.io -u bwkiwi --password-stdin
docker pull ghcr.io/bwkiwi/apollo-mcp-server:auth-bypass
docker compose -f backend.compose.yml up -d
```

---

## Troubleshooting

### Login Failed

```
Error: unauthorized: authentication required
```

**Fix**: Ensure your PAT has `write:packages` and `read:packages` scopes.

### Push Denied

```
Error: denied: requested access to the resource is denied
```

**Fix**:
1. Check PAT permissions include `write:packages`
2. Ensure the repository exists or you have permission to create it
3. Verify the image tag matches the repository name

### Image Not Found on Pull

```
Error: manifest unknown
```

**Fix**:
1. Verify the exact tag was pushed
2. Check `docker images` locally to confirm the tag exists
3. Use `docker manifest inspect ghcr.io/bwkiwi/apollo-mcp-server:tag` to verify

### Check Available Tags

```bash
# List local images
docker images | grep apollo-mcp-server

# Check registry (requires login)
docker manifest inspect ghcr.io/bwkiwi/apollo-mcp-server:latest
```

---

## Security Notes

1. **Never commit `.env`** - It's in `.gitignore` but always verify
2. **Rotate PATs regularly** - Create new tokens and revoke old ones
3. **Use minimal scopes** - PAT only needs `write:packages` and `read:packages`
4. **On servers** - Consider using Docker credential helpers instead of environment variables

---

## Authentication Bypass Mode

For development/testing, you can bypass authentication entirely using a pre-configured token.

### Configuration

```yaml
# In your server config (e.g., config/server-config.yaml)
bypass:
  enabled: true
  graphql_token: "eyJhbGciOiJSUzI1NiIs..."  # Your JWT token
  client_secret: "optional-protection"       # Optional
```

### Behavior When Bypass is Enabled

1. **No OAuth middleware** - Server accepts requests without authentication
2. **No WWW-Authenticate headers** - Clients won't try OAuth discovery
3. **Bypass token used for GraphQL** - All backend requests use the configured token
4. **Auth tools return bypass info** - `login`, `whoami`, `logout`, `getGraphQLToken` indicate bypass mode

### Important Notes

- **DEVELOPMENT ONLY** - Never use bypass mode in production
- The bypass token is sent to the GraphQL backend for all requests
- MCP clients will connect without authentication prompts

---

## Related Files

- `scripts/build-docker.sh` - Bash build script (Linux/WSL)
- `scripts/build-docker.ps1` - PowerShell build script (Windows)
- `.env` - Your credentials (git-ignored)
- `.env.example` - Template for credentials
- `Dockerfile` - Multi-stage build definition
- `docker-compose.yml` - Local development compose file
- `backend.compose.yml` - Production compose file (on server)
