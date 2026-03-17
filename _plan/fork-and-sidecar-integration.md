# Apollo MCP Server — Fork & Sidecar Integration Plan

**Created:** 2026-03-17
**Status:** Phase 2 complete, Phase 3-5 pending

## Background

We use Apollo MCP Server to expose GraphQL operations as MCP tools for AI agents. Previously we cloned the official `apollographql/apollo-mcp-server` repo and committed custom changes (Auth0 integration, test manager, role-based routing, session management) directly onto `main`. This caused:

- Version divergence (local `0.8.0-itops-testmgr` vs upstream `v1.9.0`)
- No ability to pull upstream releases cleanly
- Custom code mixed with upstream code across 16+ files
- Origin remote still pointed to Apollo's repo (not ours)

We also need to integrate the MCP server with `itops-core` (our Go microservices monorepo) which has a mature AI chat service (`internal/chat/`) with its own tool system.

## Decision: Option A — Sidecar Deployment

Deploy apollo-mcp-server as a **sidecar** alongside itops-core. The chat service in itops-core connects to the MCP server over HTTP/SSE to access GraphQL tools. This keeps the repos separate and the dependency loose.

**Why not embed?** Keeps the MCP server closer to upstream Apollo, reduces maintenance burden, and avoids tight coupling between Rust and Go codebases.

## Decision: Minimal Customisation Path

Rather than porting all custom code (Auth0 sessions, device flow, role routing, test manager, schema loader) onto upstream v1.9.0, we chose to keep only the **Auth0 token injection** in the MCP server. Everything else (session management, role-based routing, test orchestration) will be handled in itops-core's chat service instead.

**Why?** Minimises diff against upstream (195 lines added vs 2,448 removed), makes future upstream merges trivial, and concentrates custom logic in the system we fully own.

---

## Phases

### Phase 1: Fork & Fix Remotes — COMPLETE

**What:** Created a proper GitHub fork and aligned remotes to match our other repos.

**Remotes:**
| Remote | URL | Purpose |
|--------|-----|---------|
| `origin` | `https://github.com/bwkiwi/apollo-mcp-server.git` | Our fork |
| `upstream` | `https://github.com/apollographql/apollo-mcp-server.git` | Apollo official |
| `it13` | `ssh://it13:2222/home/sue/workspace/apollo-mcp-server` | Internal mirror (needs bare repo created) |

**Actions taken:**
1. Created fork at `bwkiwi/apollo-mcp-server` via `gh repo fork`
2. Renamed `origin` → `upstream`, added new `origin` (fork) and `it13`
3. Force-pushed local main to fork
4. Cleaned up 116 WSL `Zone.Identifier` broken refs from `.git/refs/`
5. Created `upstream-main` branch tracking `upstream/main` (v1.9.0)
6. Pushed both branches to fork

**Outstanding:** Create bare repo on it13 server, then `git push it13 main`.

### Phase 2: Port to Upstream v1.9.0 — COMPLETE

**What:** Rebased our customisations onto clean upstream v1.9.0, keeping only the minimal Auth0 token injection.

**Branches:**
| Branch | Base | Purpose |
|--------|------|---------|
| `main` | old v0.8.0 | Original customised code (to be replaced) |
| `itops/pre-rebase` | old v0.8.0 | Permanent reference to all original custom work |
| `upstream-main` | upstream v1.9.0 | Clean upstream tracking, never modified |
| `itops/port-to-upstream` | upstream v1.9.0 | **Active branch** — minimal itops changes on latest upstream |

**Approach:** Instead of rebasing 3 commits onto 1040 upstream commits (18 conflicting files), we:
1. Created a new branch from `upstream/main`
2. Copied over new standalone files (docs, configs, Docker, itops-ai-auth crate)
3. Made minimal, feature-gated changes to upstream files
4. Removed all non-essential custom code (session mgmt, role routing, test manager, device flow)

**What the minimal integration adds:**
- `crates/itops-ai-auth/` — Auth0 token provider crate (token refresh, bearer injection)
- `auth0:` config section in YAML (domain, client_id, audience, refresh_token)
- `Auth0TokenProvider` threaded through `Server → states::Config → Starting → Running`
- Bearer token injected into outbound GraphQL request headers at both execution points in `running.rs`
- All changes wrapped in `#[cfg(feature = "itops-auth0")]` (enabled by default)
- Supporting files: Docker configs, docs, build scripts, .env.example

**Files modified (upstream):**
- `Cargo.toml` — added itops-ai-auth to workspace members
- `crates/apollo-mcp-server/Cargo.toml` — added optional itops-ai-auth dep + feature flag
- `crates/apollo-mcp-server/src/main.rs` — create token provider from config
- `crates/apollo-mcp-server/src/server.rs` — add field to Server struct + builder
- `crates/apollo-mcp-server/src/server/states.rs` — thread through Config
- `crates/apollo-mcp-server/src/server/states/starting.rs` — pass to Running
- `crates/apollo-mcp-server/src/server/states/running.rs` — inject token into headers
- `crates/apollo-mcp-server/src/runtime.rs` — add auth0 module
- `crates/apollo-mcp-server/src/runtime/config.rs` — add auth0 config field
- `crates/apollo-mcp-server/src/runtime/auth0.rs` — Auth0Config struct (new file, in runtime)

**Outstanding:** Install Rust toolchain and verify build compiles (`cargo check`).

### Phase 3: Merge to Main — PENDING

Once the build is verified:
```bash
git checkout main
git reset --hard itops/port-to-upstream
git push origin main --force
```

This replaces the old diverged main with the clean upstream-based version.

### Phase 4: Sidecar Integration with itops-core — PENDING

1. **Add to docker-compose** (`itops-core/deploy/docker/docker-compose.yml`):
   ```yaml
   mcp-server:
     image: apollo-mcp-server:latest
     ports:
       - "5000:5000"
     volumes:
       - ./config/mcp-config.yaml:/config/server-config.yaml
     depends_on:
       postgres:
         condition: service_healthy
   ```

2. **Add MCP tool provider** in `itops-core/internal/chat/tools/`:
   - Register MCP tools into the chat agent's tool registry
   - Connect over HTTP/SSE (port 5000)
   - Map MCP tool calls to the chat agent's tool execution interface

3. **Config file** for MCP server pointing at GraphQL endpoint(s) with Auth0 credentials matching itops-core's auth setup.

### Phase 5: CI/CD & Ongoing Maintenance — PENDING

1. **Update fork CI workflows:**
   - Disable Apple notarisation and ghcr.io publishing (no secrets)
   - Keep build + test
   - Add Docker build step for our own registry

2. **Upstream sync process:**
   ```bash
   git fetch upstream
   git checkout upstream-main && git merge upstream/main
   git checkout main && git merge upstream-main
   # Resolve any conflicts in auth0 feature-gated code
   git push origin main
   ```

3. **Versioning:** Tag as `v1.9.0-itops.1`, etc. Track upstream versions.

---

## Config Example

```yaml
endpoint: https://api.example.com/graphql

auth0:
  domain: your-tenant.eu.auth0.com
  client_id: your-client-id
  audience: https://api.example.com/graphql
  refresh_token: ${env.AUTH0_REFRESH_TOKEN}

transport:
  type: streamable_http
  port: 5000

operations:
  source: local
  paths:
    - ./operations/*.graphql
```

## Reference

- Fork: https://github.com/bwkiwi/apollo-mcp-server
- Upstream: https://github.com/apollographql/apollo-mcp-server
- Original custom code: branch `itops/pre-rebase`
- itops-core chat service: `~/workspace/itops-core/internal/chat/`
- itops-core tool registry: `~/workspace/itops-core/internal/chat/tools/`
