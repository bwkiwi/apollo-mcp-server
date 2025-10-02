# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Building and Testing
- **Build from source**: `cargo build` (creates binary in `target/debug/`)
- **Run tests**: `cargo test`
- **Run Clippy linting**: `cargo clippy`
- **Check formatting**: `cargo fmt --check`

### Nix Development (Preferred)
- **Build**: `nix build .#`
- **Development shell**: `nix develop`
- **Run all checks**: `nix flake check`
- **Run tests in dev shell**: `nix develop --command bash -c "cargo test"`

### Running the Server
- **With config file**: `cargo run -- /path/to/config.yaml`
- **From environment**: `cargo run` (reads config from env vars)
- **Generate config schema**: `cargo run --bin config-schema`

## Architecture

### High-Level Structure
Apollo MCP Server is a Rust workspace with three main crates:

1. **apollo-mcp-server** - Main server implementation that exposes GraphQL operations as MCP tools
2. **apollo-mcp-registry** - Handles GraphOS platform API integration and operation collection management
3. **apollo-schema-index** - Provides GraphQL schema indexing and search capabilities

### Key Components

#### Server Architecture (crates/apollo-mcp-server/src/)
- **main.rs** - Entry point, configuration parsing, and server initialization
- **server/** - Core server state machine with distinct states (starting, configuring, schema_configured, operations_configured, running)
- **runtime/** - Configuration management, schema/operation sources, GraphOS integration
- **auth/** - Authentication and authorization (JWT validation, protected resources)
- **introspection/** - GraphQL introspection tools (execute, validate, search, introspect)

#### Data Sources
- **Schema Sources**: Local files (with hot reload), GraphOS Uplink registry
- **Operation Sources**: Local GraphQL files, operation collections, persisted query manifests, GraphOS Uplink
- **Fallback Strategy**: If no operations specified, falls back to introspection tools or default collection

#### Transport & Protocol
- Uses `rmcp` crate for MCP protocol implementation
- Supports multiple transports: stdio, HTTP with SSE, streamable HTTP
- Health check endpoint configurable

### Configuration
- Primary config via YAML files (structure defined in `runtime/config.rs`)
- Environment variable support via Figment
- JSON schema generation available via `config-schema` binary
- Custom scalar mapping support for GraphQL types

### Code Quality Standards
- Strict Clippy lints (see workspace Cargo.toml): no `unwrap`, `expect`, `panic`, `exit`, indexing/slicing in production code
- Test exceptions allowed via `clippy.toml`
- All dependencies use workspace versions for consistency

### Example Configurations
See `graphql/` directory for example setups with different APIs (weather, space launches) including operation definitions and configuration files.