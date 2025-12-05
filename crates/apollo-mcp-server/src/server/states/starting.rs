use std::{net::SocketAddr, sync::Arc};

use apollo_compiler::{Name, Schema, ast::OperationType, validation::Valid};
use axum::{Router, extract::Query, http::StatusCode, response::Json, routing::get};
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::{
    ServiceExt as _,
    transport::{SseServer, sse_server::SseServerConfig, stdio},
};
use serde_json::json;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument as _, debug, error, info, trace, warn};

use crate::{
    auth::{LoginTool, WhoAmITool, LogoutTool, GetGraphQLTokenTool},
    errors::ServerError,
    explorer::Explorer,
    health::HealthCheck,
    introspection::tools::{
        execute::Execute, introspect::Introspect, search::Search, validate::Validate,
    },
    operations::{MutationMode, RawOperation},
    server::Transport,
};

use super::{Config, Running, shutdown_signal};

pub(super) struct Starting {
    pub(super) config: Config,
    pub(super) schema: Valid<Schema>,
    pub(super) operations: Vec<RawOperation>,
}

impl Starting {
    pub(super) async fn start(self) -> Result<Running, ServerError> {
        let peers = Arc::new(RwLock::new(Vec::new()));

        let operations: Vec<_> = self
            .operations
            .into_iter()
            .filter_map(|operation| {
                operation
                    .into_operation(
                        &self.schema,
                        self.config.custom_scalar_map.as_ref(),
                        self.config.mutation_mode,
                        self.config.disable_type_description,
                        self.config.disable_schema_description,
                    )
                    .unwrap_or_else(|error| {
                        error!("Invalid operation: {}", error);
                        None
                    })
            })
            .collect();

        debug!(
            "Loaded {} operations:\n{}",
            operations.len(),
            serde_json::to_string_pretty(&operations)?
        );

        let execute_tool = self
            .config
            .execute_introspection
            .then(|| Execute::new(self.config.mutation_mode));

        let root_query_type = self
            .config
            .introspect_introspection
            .then(|| {
                self.schema
                    .root_operation(OperationType::Query)
                    .map(Name::as_str)
                    .map(|s| s.to_string())
            })
            .flatten();
        let root_mutation_type = self
            .config
            .introspect_introspection
            .then(|| {
                matches!(self.config.mutation_mode, MutationMode::All)
                    .then(|| {
                        self.schema
                            .root_operation(OperationType::Mutation)
                            .map(Name::as_str)
                            .map(|s| s.to_string())
                    })
                    .flatten()
            })
            .flatten();
        let schema = Arc::new(Mutex::new(self.schema));
        let introspect_tool = self.config.introspect_introspection.then(|| {
            Introspect::new(
                schema.clone(),
                root_query_type,
                root_mutation_type,
                self.config.introspect_minify,
            )
        });
        let validate_tool = self
            .config
            .validate_introspection
            .then(|| Validate::new(schema.clone()));
        let search_tool = if self.config.search_introspection {
            Some(Search::new(
                schema.clone(),
                matches!(self.config.mutation_mode, MutationMode::All),
                self.config.search_leaf_depth,
                self.config.index_memory_bytes,
                self.config.search_minify,
            )?)
        } else {
            None
        };

        let explorer_tool = self.config.explorer_graph_ref.map(Explorer::new);

        // Create Phase 2 Auth0 authentication tools if enabled
        let (login_tool, whoami_tool, logout_tool, get_graphql_token_tool) = if let (Some(session_manager), Some(device_flow_manager)) = 
            (&self.config.session_manager, &self.config.device_flow_manager) {
            (
                Some(LoginTool::new(device_flow_manager.clone(), session_manager.clone())),
                Some(WhoAmITool::new(session_manager.clone())),
                Some(LogoutTool::new(session_manager.clone())),
                Some(GetGraphQLTokenTool::new(session_manager.clone())),
            )
        } else {
            (None, None, None, None)
        };

        let cancellation_token = CancellationToken::new();

        // Create health check if enabled (only for StreamableHttp transport)
        let health_check = match (&self.config.transport, self.config.health_check.enabled) {
            (
                Transport::StreamableHttp {
                    auth: _,
                    address: _,
                    port: _,
                },
                true,
            ) => {
                let mut health_config = self.config.health_check.clone();
                // Set GraphQL endpoint for backend connectivity check
                health_config.graphql_endpoint = Some(self.config.endpoint.to_string());
                Some(HealthCheck::new(health_config))
            },
            _ => None, // No health check for SSE, Stdio, or when disabled
        };

        let running = Running {
            schema,
            operations: Arc::new(Mutex::new(operations)),
            headers: self.config.headers,
            endpoint: self.config.endpoint,
            auth0_token_provider: self.config.auth0_token_provider,
            session_manager: self.config.session_manager,
            device_flow_manager: self.config.device_flow_manager,
            schema_cache: self.config.schema_cache,
            role_config: self.config.role_config,
            test_manager: self.config.test_manager,
            login_tool,
            whoami_tool,
            logout_tool,
            get_graphql_token_tool,
            execute_tool,
            introspect_tool,
            search_tool,
            explorer_tool,
            validate_tool,
            custom_scalar_map: self.config.custom_scalar_map,
            peers,
            cancellation_token: cancellation_token.clone(),
            mutation_mode: self.config.mutation_mode,
            disable_type_description: self.config.disable_type_description,
            disable_schema_description: self.config.disable_schema_description,
            health_check: health_check.clone(),
        };

        // Helper to enable auth
        macro_rules! with_auth {
            ($router:expr, $auth:ident) => {{
                let mut router = $router;
                if let Some(auth) = $auth {
                    router = auth.enable_middleware(router);
                }

                router
            }};
        }
        match self.config.transport {
            Transport::StreamableHttp {
                auth,
                address,
                port,
            } => {
                info!(port = ?port, address = ?address, "Starting MCP server in Streamable HTTP mode");
                let running_for_service = running.clone();
                let running_for_debug = running.clone();
                let listen_address = SocketAddr::new(address, port);
                let service = StreamableHttpService::new(
                    move || Ok(running_for_service.clone()),
                    LocalSessionManager::default().into(),
                    Default::default(),
                );
                let mut router =
                    with_auth!(axum::Router::new().nest_service("/mcp", service), auth);

                // Add debug endpoint to show internal state
                let debug_router = Router::new()
                    .route("/debug", get(debug_endpoint))
                    .with_state(running_for_debug);
                router = router.merge(debug_router);

                // Add health check endpoint if configured
                if let Some(health_check) = health_check.filter(|h| h.config().enabled) {
                    let health_router = Router::new()
                        .route(&health_check.config().path, get(health_endpoint))
                        .with_state(health_check.clone());
                    router = router.merge(health_router);
                }

                info!("🔌 Attempting to bind TCP listener on {}:{}", address, port);
                let tcp_listener = tokio::net::TcpListener::bind(listen_address).await?;
                info!("✅ Successfully bound TCP listener on {}:{}", address, port);

                tokio::spawn(async move {
                    info!("🚀 Starting axum HTTP server...");
                    // Health check is already active from creation
                    if let Err(e) = axum::serve(tcp_listener, router)
                        .with_graceful_shutdown(shutdown_signal())
                        .await
                    {
                        // This can never really happen
                        error!("Failed to start MCP server: {e:?}");
                    }
                    info!("🛑 Axum server has shutdown");
                });
                info!("✅ HTTP server task spawned successfully, server should be running in background");
            }
            Transport::SSE {
                auth,
                address,
                port,
            } => {
                info!(port = ?port, address = ?address, "Starting MCP server in SSE mode");
                let running = running.clone();
                let listen_address = SocketAddr::new(address, port);

                let (server, router) = SseServer::new(SseServerConfig {
                    bind: listen_address,
                    sse_path: "/sse".to_string(),
                    post_path: "/message".to_string(),
                    ct: cancellation_token,
                    sse_keep_alive: None,
                });

                // Optionally wrap the router with auth, if enabled
                let router = with_auth!(router, auth);

                // Start up the SSE server
                // Note: Until RMCP consolidates SSE with the same tower system as StreamableHTTP,
                // we need to basically copy the implementation of `SseServer::serve_with_config` here.
                let listener = tokio::net::TcpListener::bind(server.config.bind).await?;
                let ct = server.config.ct.child_token();
                let axum_server =
                    axum::serve(listener, router).with_graceful_shutdown(async move {
                        ct.cancelled().await;
                        tracing::info!("mcp server cancelled");
                    });

                tokio::spawn(
                    async move {
                        if let Err(e) = axum_server.await {
                            tracing::error!(error = %e, "mcp shutdown with error");
                        }
                    }
                    .instrument(
                        tracing::info_span!("mcp-server", bind_address = %server.config.bind),
                    ),
                );

                server.with_service(move || running.clone());
            }
            Transport::Stdio => {
                info!("Starting MCP server in stdio mode");
                let service = running.clone().serve(stdio()).await.inspect_err(|e| {
                    error!("serving error: {:?}", e);
                })?;
                service.waiting().await.map_err(ServerError::StartupError)?;
            }
        }

        Ok(running)
    }
}

/// Health check endpoint handler
async fn health_endpoint(
    axum::extract::State(health_check): axum::extract::State<HealthCheck>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let query = params.keys().next().map(|k| k.as_str());
    let (health, status_code) = health_check.get_health_state(query).await;

    trace!(?health, query = ?query, "health check");

    Ok((status_code, Json(json!(health))))
}

/// Debug endpoint handler - shows internal server state
async fn debug_endpoint(
    axum::extract::State(running): axum::extract::State<Running>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    debug!("Debug endpoint called");

    // Gather session information
    let sessions_info = if let Some(session_manager) = &running.session_manager {
        match session_manager.list_sessions().await {
            Ok(session_ids) => {
                let mut sessions_data = Vec::new();
                for session_id in session_ids.iter() {
                    if let Ok(Some(user_info)) = session_manager.get_session_info(session_id).await {
                        sessions_data.push(json!({
                            "session_id": session_id,
                            "user_info": {
                                "sub": user_info.sub,
                                "email": user_info.email,
                                "name": user_info.name,
                                "groups": user_info.groups,
                                "permissions": user_info.permissions,
                            },
                            "is_authenticated": session_manager.is_authenticated(session_id).await.unwrap_or(false),
                        }));
                    }
                }
                Some(json!({
                    "active_count": session_ids.len(),
                    "sessions": sessions_data
                }))
            },
            Err(e) => {
                warn!("Failed to list sessions: {}", e);
                Some(json!({
                    "error": format!("Failed to list sessions: {}", e)
                }))
            }
        }
    } else {
        None
    };

    // Gather Auth0 token info (Phase 1)
    let auth0_phase1_info = if let Some(provider) = &running.auth0_token_provider {
        let mut token_data = provider.lock().await;
        Some(json!({
            "enabled": true,
            "has_token": token_data.get_bearer().await.is_ok(),
        }))
    } else {
        None
    };

    // Gather device flow info (Phase 2)
    let device_flow_info = if running.device_flow_manager.is_some() {
        Some(json!({
            "enabled": true,
        }))
    } else {
        None
    };

    // Gather peer/connection information
    let peers = running.peers.read().await;
    let connections_info = json!({
        "active_peers": peers.len(),
        "peers": peers.iter().enumerate().map(|(i, _peer)| {
            json!({
                "index": i,
                "type": "mcp_peer"
            })
        }).collect::<Vec<_>>()
    });

    // Gather operations info
    let operations = running.operations.lock().await;
    let operations_info = json!({
        "count": operations.len(),
        "names": operations.iter().map(|op| &op.as_ref().name).collect::<Vec<_>>()
    });

    // Gather role-based routing info
    let role_info = if let Some(role_config) = &running.role_config {
        Some(json!({
            "enabled": true,
            "base_url": role_config.graphql_base_url,
            "available_roles": role_config.available_roles,
        }))
    } else {
        None
    };

    // Gather schema cache info
    let schema_cache_info = if let Some(cache) = &running.schema_cache {
        let cached_roles = cache.available_roles();
        Some(json!({
            "enabled": true,
            "cached_roles": cached_roles,
        }))
    } else {
        None
    };

    // Build debug response
    let debug_info = json!({
        "server": {
            "version": env!("CARGO_PKG_VERSION"),
            "endpoint": running.endpoint.to_string(),
            "mutation_mode": format!("{:?}", running.mutation_mode),
        },
        "authentication": {
            "phase1_auth0": auth0_phase1_info,
            "phase2_auth0": {
                "sessions": sessions_info,
                "device_flow": device_flow_info,
            },
        },
        "connections": connections_info,
        "operations": operations_info,
        "role_routing": role_info,
        "schema_cache": schema_cache_info,
        "features": {
            "health_check": running.health_check.is_some(),
            "test_manager": running.test_manager.is_some(),
            "execute_tool": running.execute_tool.is_some(),
            "introspect_tool": running.introspect_tool.is_some(),
            "search_tool": running.search_tool.is_some(),
            "explorer_tool": running.explorer_tool.is_some(),
            "validate_tool": running.validate_tool.is_some(),
        },
    });

    Ok(Json(debug_info))
}
