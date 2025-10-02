# Objectives

* Enable the **Apollo MCP server** to act **as the currently connected Claude user**, not a shared account.
* Perform **per-session Auth0 login** (Device Flow) inside the MCP server and **bind identity to the MCP session**.
* Obtain/refresh **GraphQL-audience access tokens** from Auth0 and attach them to **every outbound GraphQL request**.
* Keep a **single Auth0 Application** (Native) for all users/sessions—no per-user/per-session apps.
* Provide a smooth UX for Claude users (login/whoami/logout) and support Apollo Explorer/API usage with the same tokens.

# Action items (recommended changes)

## A) Auth0 configuration (single app)

* [ ] Create/use **one Auth0 Native Application** (not M2M, not SPA).
* [ ] Enable **Grant Types**: *Device Authorization Grant*, *Refresh Token* (Allow Offline Access).
* [ ] Create/use a single **API (Resource Server)** in Auth0 with Identifier (audience), e.g. `https://api.it-ops.ai`.
* [ ] Scopes: `openid profile email offline_access` (+ any custom scopes you plan to enforce).

## B) MCP server: per-session user login

* [ ] Add a `login` tool:

  * Triggers **Device Flow** (`/oauth/device/code`), returns `verification_uri_complete` + `user_code` message to the user.
  * Polls `/oauth/token` until tokens received; store `{sub, refresh_token, access_token, expires_at}`.
* [ ] Add a `whoami` tool: returns the **current session’s** user claims (email/sub/groups).
* [ ] Add a `logout` tool: revoke/clear the **current session’s** stored tokens.

## C) Session-aware token management

* [ ] Maintain an in-memory map:
  `sessions: HashMap<McpSessionId, TokenState>` where `TokenState` includes `{sub, refresh_token, access_token, expires_at}`.
* [ ] Implement an `Auth0TokenProvider` that:

  * Uses **refresh\_token** to mint **GraphQL-audience** `access_token` via `/oauth/token` (form-encoded).
  * Auto-refreshes if `expires_at` is near/expired.
* [ ] Store per-session state keyed by the **MCP session id** (e.g., header `mcp-session-id`), not globally.

## D) Inject Authorization on outbound GraphQL requests

* [ ] In the GraphQL request path (e.g., `apollo-mcp-server/src/server.rs`):

  * Resolve current `McpSessionId` → lookup `TokenState`.
  * Call `get_bearer()` on the session’s `Auth0TokenProvider`.
  * Add `Authorization: Bearer <access_token>` header to **every** outbound GraphQL HTTP request.

## E) Apollo Server (GraphQL) verification & Shield

* [ ] Verify JWT via Auth0 **JWKS**; check:

  * `iss = https://<tenant>.auth0.com/`
  * `aud = https://api.it-ops.ai`
* [ ] Populate `context.user` from verified claims; apply **GraphQL Shield** rules using `user.sub`, `user.scope`, and/or `user.groups` (optionally enrich from your DB).

## F) Persistence & ops

* [ ] Add a pluggable storage interface (memory first, optional file/kv) to **persist refresh tokens** per session across restarts (if desired).
* [ ] Add logging/metrics for auth events (login, refresh, revoke) with session ids (no secrets in logs).
* [ ] Provide an admin/debug endpoint or tool to **invalidate a session**.

## G) Developer ergonomics & Explorer/API usage

* [ ] Expose a `getGraphQLToken` tool (optional): returns the **current session’s** short-lived access token so the user can paste it into Apollo Explorer (`Authorization: Bearer …`).
* [ ] Document the local “login → whoami → query” flow for Claude users.
* [ ] Provide a **minimal web helper** (optional) for non-Claude users to obtain a token for Explorer/API calls (same single Auth0 App & API).

## H) Security & policy

* [ ] Use **short-lived** access tokens (e.g., 5–15 minutes) and rely on refresh.
* [ ] Encrypt at-rest storage if you persist refresh tokens; scope the Native App to only required permissions.
* [ ] Implement **logout** to revoke refresh tokens (Auth0 Management API) or at least delete locally.

## I) Configuration & deployment

* [ ] Read from env/config: `AUTH0_DOMAIN`, `AUTH0_CLIENT_ID`, `AUTH0_AUDIENCE`.
* [ ] Optional: `TOKEN_STORE_PATH`, `REFRESH_SKEW_SECONDS`.
* [ ] Build and publish a **custom Docker image** of your modified Apollo MCP server; update Claude/remote-mcp to point at it.

---

**End state:**

* One **Auth0 Native Application** serves all users.
* Each MCP session runs its *own* Auth0 login (Device Flow), maintains its *own* refresh/access tokens, and calls GraphQL **as that user**.
* Apollo Server verifies tokens against your Auth0 API audience; Shield enforces your granular rights.
