use std::ops::Deref as _;
use std::sync::Arc;

use apollo_compiler::{Schema, validation::Valid};
use headers::HeaderMapExt as _;
use itops_ai_auth::Auth0TokenProvider;
use reqwest::header::HeaderMap;
use rmcp::model::Implementation;
use rmcp::{
    Peer, RoleServer, ServerHandler, ServiceError,
    model::{
        CallToolRequestParam, CallToolResult, Content, ErrorCode, InitializeRequestParam, InitializeResult,
        ListToolsResult, PaginatedRequestParam, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};
use url::Url;

use crate::{
    auth::{ValidToken, SessionManager, DeviceFlowManager, LoginTool, WhoAmITool, LogoutTool, GetGraphQLTokenTool},
    custom_scalar_map::CustomScalarMap,
    errors::{McpError, ServerError},
    explorer::{EXPLORER_TOOL_NAME, Explorer},
    graphql::{self, Executable as _},
    health::HealthCheck,
    introspection::tools::{
        execute::{EXECUTE_TOOL_NAME, Execute},
        introspect::{INTROSPECT_TOOL_NAME, Introspect},
        search::{SEARCH_TOOL_NAME, Search},
        validate::{VALIDATE_TOOL_NAME, Validate},
    },
    operations::{MutationMode, Operation, RawOperation},
    test_manager::{
        SNAPSHOT_CLEAR_TOOL, SNAPSHOT_LOAD_TOOL, SNAPSHOT_SAVE_TOOL, SNAPSHOT_LIST_TOOL,
        TEST_GET_TOOL, TEST_SAVE_TOOL, TEST_UPDATE_TOOL, TEST_SAVE_RESULT_TOOL,
        MCP_DESCRIPTION_GET_TOOL, MCP_DESCRIPTION_SET_TOOL,
        SaveSnapshotParams, SaveTestParams, UpdateTestParams, SaveResultParams,
    },
};

// Phase 2 Auth0 tool names
const LOGIN_TOOL_NAME: &str = "login";
const WHOAMI_TOOL_NAME: &str = "whoami";
const LOGOUT_TOOL_NAME: &str = "logout";
const GET_GRAPHQL_TOKEN_TOOL_NAME: &str = "getGraphQLToken";

#[derive(Clone)]
pub(super) struct Running {
    pub(super) schema: Arc<Mutex<Valid<Schema>>>,
    pub(super) operations: Arc<Mutex<Vec<Operation>>>,
    pub(super) headers: HeaderMap,
    pub(super) endpoint: Url,
    // Phase 1 Auth0 (backward compatibility)
    pub(super) auth0_token_provider: Option<Arc<Mutex<Auth0TokenProvider>>>,
    // Phase 2 Auth0 (per-session authentication)
    pub(super) session_manager: Option<Arc<SessionManager>>,
    pub(super) device_flow_manager: Option<Arc<DeviceFlowManager>>,
    // Role-based routing
    pub(super) schema_cache: Option<Arc<crate::schema_loader::SchemaCache>>,
    pub(super) role_config: Option<crate::server::RoleConfig>,
    // Test manager integration
    pub(super) test_manager: Option<Arc<crate::test_manager::TestManagerTools>>,
    // Authentication tools for Phase 2
    pub(super) login_tool: Option<LoginTool>,
    pub(super) whoami_tool: Option<WhoAmITool>,
    pub(super) logout_tool: Option<LogoutTool>,
    pub(super) get_graphql_token_tool: Option<GetGraphQLTokenTool>,
    pub(super) execute_tool: Option<Execute>,
    pub(super) introspect_tool: Option<Introspect>,
    pub(super) search_tool: Option<Search>,
    pub(super) explorer_tool: Option<Explorer>,
    pub(super) validate_tool: Option<Validate>,
    pub(super) custom_scalar_map: Option<CustomScalarMap>,
    pub(super) peers: Arc<RwLock<Vec<Peer<RoleServer>>>>,
    pub(super) cancellation_token: CancellationToken,
    pub(super) mutation_mode: MutationMode,
    pub(super) disable_type_description: bool,
    pub(super) disable_schema_description: bool,
    pub(super) health_check: Option<HealthCheck>,
}

impl Running {
    /// Get the GraphQL endpoint for a specific role, or default endpoint
    fn get_endpoint_for_role(&self, role: Option<&str>) -> Url {
        if let (Some(config), Some(role)) = (&self.role_config, role) {
            crate::role_router::build_endpoint_for_role(&config.graphql_base_url, role)
        } else {
            self.endpoint.clone()
        }
    }

    /// Get the schema for a specific role, or default schema
    async fn get_schema_for_role(&self, role: Option<&str>) -> Arc<Mutex<Valid<Schema>>> {
        if let (Some(cache), Some(role)) = (&self.schema_cache, role) {
            if let Some(schema) = cache.get_schema(role) {
                return Arc::new(Mutex::new(schema.clone()));
            }
        }
        self.schema.clone()
    }

    /// Extract role from request context (from HTTP path)
    fn extract_role(&self, context: &RequestContext<RoleServer>) -> Option<String> {
        if self.role_config.is_none() {
            return None;
        }

        // Extract path from HTTP request context
        context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| {
                let path = parts.uri.path();
                let default_role = self.role_config.as_ref().map(|c| c.default_role.as_str())?;
                Some(crate::role_router::get_role(path, default_role))
            })
    }

    /// Update a running server with a new schema.
    pub(super) async fn update_schema(self, schema: Valid<Schema>) -> Result<Running, ServerError> {
        debug!("Schema updated:\n{}", schema);

        // Update the operations based on the new schema. This is necessary because the MCP tool
        // input schemas and description are derived from the schema.
        let operations: Vec<Operation> = self
            .operations
            .lock()
            .await
            .iter()
            .cloned()
            .map(|operation| operation.into_inner())
            .filter_map(|operation| {
                operation
                    .into_operation(
                        &schema,
                        self.custom_scalar_map.as_ref(),
                        self.mutation_mode,
                        self.disable_type_description,
                        self.disable_schema_description,
                    )
                    .unwrap_or_else(|error| {
                        error!("Invalid operation: {}", error);
                        None
                    })
            })
            .collect();

        debug!(
            "Updated {} operations:\n{}",
            operations.len(),
            serde_json::to_string_pretty(&operations)?
        );
        *self.operations.lock().await = operations;

        // Update the schema itself
        *self.schema.lock().await = schema;

        // Notify MCP clients that tools have changed
        Self::notify_tool_list_changed(self.peers.clone()).await;
        Ok(self)
    }

    pub(super) async fn update_operations(
        self,
        operations: Vec<RawOperation>,
    ) -> Result<Running, ServerError> {
        debug!("Operations updated:\n{:?}", operations);

        // Update the operations based on the current schema
        {
            let schema = &*self.schema.lock().await;
            let updated_operations: Vec<Operation> = operations
                .into_iter()
                .filter_map(|operation| {
                    operation
                        .into_operation(
                            schema,
                            self.custom_scalar_map.as_ref(),
                            self.mutation_mode,
                            self.disable_type_description,
                            self.disable_schema_description,
                        )
                        .unwrap_or_else(|error| {
                            error!("Invalid operation: {}", error);
                            None
                        })
                })
                .collect();

            debug!(
                "Loaded {} operations:\n{}",
                updated_operations.len(),
                serde_json::to_string_pretty(&updated_operations)?
            );
            *self.operations.lock().await = updated_operations;
        }

        // Notify MCP clients that tools have changed
        Self::notify_tool_list_changed(self.peers.clone()).await;
        Ok(self)
    }

    /// Notify any peers that tools have changed. Drops unreachable peers from the list.
    async fn notify_tool_list_changed(peers: Arc<RwLock<Vec<Peer<RoleServer>>>>) {
        let mut peers = peers.write().await;
        if !peers.is_empty() {
            debug!(
                "Operations changed, notifying {} peers of tool change",
                peers.len()
            );
        }
        let mut retained_peers = Vec::new();
        for peer in peers.iter() {
            if !peer.is_transport_closed() {
                match peer.notify_tool_list_changed().await {
                    Ok(_) => retained_peers.push(peer.clone()),
                    Err(ServiceError::TransportSend(_) | ServiceError::TransportClosed) => {
                        error!("Failed to notify peer of tool list change - dropping peer",);
                    }
                    Err(e) => {
                        error!("Failed to notify peer of tool list change {:?}", e);
                        retained_peers.push(peer.clone());
                    }
                }
            }
        }
        *peers = retained_peers;
    }
}

impl ServerHandler for Running {
    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        // TODO: how to remove these?
        let mut peers = self.peers.write().await;
        peers.push(context.peer);
        Ok(self.get_info())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // Extract session ID for Phase 2 authentication tools
        let session_id = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.headers.get("mcp-session-id"))
            .and_then(|header| header.to_str().ok())
            .unwrap_or("default-session")
            .to_string();

        let result = match request.name.as_ref() {
            // Phase 2 Auth0 authentication tools
            LOGIN_TOOL_NAME => {
                self.login_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(session_id)
                    .await
            }
            WHOAMI_TOOL_NAME => {
                self.whoami_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(session_id)
                    .await
            }
            LOGOUT_TOOL_NAME => {
                self.logout_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(session_id)
                    .await
            }
            GET_GRAPHQL_TOKEN_TOOL_NAME => {
                self.get_graphql_token_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(session_id)
                    .await
            }
            // Test Manager tools
            SNAPSHOT_CLEAR_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let args: serde_json::Map<String, Value> = convert_arguments(request)?;
                let confirm = args.get("confirm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                match test_manager.snapshot_clear(confirm).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Snapshot cleared successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            SNAPSHOT_LOAD_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let args: serde_json::Map<String, Value> = convert_arguments(request)?;
                let snapshot_name = args.get("snapshotName")
                    .and_then(|v| v.as_str())
                    .ok_or(McpError::new(ErrorCode::INVALID_PARAMS, "snapshotName required", None::<Value>))?
                    .to_string();
                let clear_first = args.get("clearFirst")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                match test_manager.snapshot_load(snapshot_name, clear_first).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Snapshot loaded successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            SNAPSHOT_SAVE_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let params: SaveSnapshotParams = convert_arguments(request)?;

                match test_manager.snapshot_save(params).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Snapshot saved successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            SNAPSHOT_LIST_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;

                match test_manager.snapshot_list().await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("No snapshots found"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            TEST_GET_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let args: serde_json::Map<String, Value> = convert_arguments(request)?;
                let test_id = args.get("testId")
                    .and_then(|v| v.as_str())
                    .ok_or(McpError::new(ErrorCode::INVALID_PARAMS, "testId required", None::<Value>))?
                    .to_string();

                match test_manager.test_get(test_id).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Test retrieved"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            TEST_SAVE_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let params: SaveTestParams = convert_arguments(request)?;

                match test_manager.test_save(params).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Test saved successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            TEST_UPDATE_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let args: serde_json::Map<String, Value> = convert_arguments(request)?;
                let test_id = args.get("testId")
                    .and_then(|v| v.as_str())
                    .ok_or(McpError::new(ErrorCode::INVALID_PARAMS, "testId required", None::<Value>))?
                    .to_string();
                let params: UpdateTestParams = serde_json::from_value(Value::Object(args.clone()))
                    .map_err(|_| McpError::new(ErrorCode::INVALID_PARAMS, "Invalid parameters", None::<Value>))?;

                match test_manager.test_update(test_id, params).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Test updated successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            TEST_SAVE_RESULT_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let params: SaveResultParams = convert_arguments(request)?;

                match test_manager.test_save_result(params).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("Test result saved successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            MCP_DESCRIPTION_GET_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;

                let description = test_manager.get_mcp_description().await;
                let response = serde_json::json!({
                    "description": description
                });
                Ok(CallToolResult {
                    content: vec![Content::json(&response)
                        .unwrap_or_else(|_| Content::text(&description))],
                    is_error: Some(false),
                })
            }
            MCP_DESCRIPTION_SET_TOOL => {
                let test_manager = self.test_manager.as_ref()
                    .ok_or(tool_not_found(&request.name))?;
                let args: serde_json::Map<String, Value> = convert_arguments(request)?;
                let text = args.get("additionalText")
                    .and_then(|v| v.as_str())
                    .ok_or(McpError::new(ErrorCode::INVALID_PARAMS, "additionalText required", None::<Value>))?
                    .to_string();

                match test_manager.mcp_description_set(text).await {
                    Ok(result) => Ok(CallToolResult {
                        content: vec![Content::json(&result)
                            .unwrap_or_else(|_| Content::text("MCP description updated successfully"))],
                        is_error: Some(false),
                    }),
                    Err(e) => Err(McpError::new(ErrorCode::INTERNAL_ERROR, e, None::<Value>)),
                }
            }
            INTROSPECT_TOOL_NAME => {
                self.introspect_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(convert_arguments(request)?)
                    .await
            }
            SEARCH_TOOL_NAME => {
                self.search_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(convert_arguments(request)?)
                    .await
            }
            EXPLORER_TOOL_NAME => {
                self.explorer_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(convert_arguments(request)?)
                    .await
            }
            EXECUTE_TOOL_NAME => {
                let mut headers = self.headers.clone();
                
                // Add authentication based on Phase 1 or Phase 2
                if let Some(session_manager) = &self.session_manager {
                    // Phase 2: per-session authentication
                    match session_manager.get_valid_token(&session_id).await {
                        Ok(bearer_token) => {
                            headers.insert("Authorization", bearer_token.parse().unwrap());
                        }
                        Err(e) => {
                            // Session not authenticated, return authentication required error
                            warn!("Session {} not authenticated for GraphQL request: {:?}", session_id, e);
                            
                            // Return 401 Unauthorized to trigger OAuth flow in client
                            return Err(McpError::new(
                                ErrorCode::INTERNAL_ERROR,
                                "Authentication required. Please authenticate to access this resource.",
                                None::<serde_json::Value>
                            ));
                        }
                    }
                } else if let Some(axum_parts) = context.extensions.get::<axum::http::request::Parts>() {
                    // Phase 1: extract validated token from SSE auth middleware
                    if let Some(token) = axum_parts.extensions.get::<ValidToken>() {
                        headers.typed_insert(token.deref().clone());
                    }
                }
                
                if let Some(axum_parts) = context.extensions.get::<axum::http::request::Parts>() {
                    // Forward the mcp-session-id header if present
                    if let Some(session_id_header) = axum_parts.headers.get("mcp-session-id") {
                        headers.insert("mcp-session-id", session_id_header.clone());
                    }
                }

                // Extract role from request path for multi-schema routing
                let role = self.extract_role(&context);
                let endpoint = self.get_endpoint_for_role(role.as_deref());

                if let Some(ref r) = role {
                    debug!("Routing execute tool to role-specific endpoint: role={}, endpoint={}", r, endpoint);
                }

                self.execute_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(graphql::Request {
                        input: Value::from(request.arguments.clone()),
                        endpoint: &endpoint,
                        headers,
                        auth0_token_provider: self.auth0_token_provider.as_ref(),
                    })
                    .await
            }
            VALIDATE_TOOL_NAME => {
                self.validate_tool
                    .as_ref()
                    .ok_or(tool_not_found(&request.name))?
                    .execute(convert_arguments(request)?)
                    .await
            }
            _ => {
                let mut headers = self.headers.clone();
                
                // Add authentication based on Phase 1 or Phase 2
                if let Some(session_manager) = &self.session_manager {
                    // Phase 2: per-session authentication
                    match session_manager.get_valid_token(&session_id).await {
                        Ok(bearer_token) => {
                            headers.insert("Authorization", bearer_token.parse().unwrap());
                        }
                        Err(e) => {
                            // Session not authenticated, return authentication required error
                            warn!("Session {} not authenticated for operation {}: {:?}", session_id, request.name, e);
                            
                            // Return 401 Unauthorized to trigger OAuth flow in client
                            return Err(McpError::new(
                                ErrorCode::INTERNAL_ERROR,
                                "Authentication required. Please authenticate to access this resource.",
                                None::<serde_json::Value>
                            ));
                        }
                    }
                } else if let Some(axum_parts) = context.extensions.get::<axum::http::request::Parts>() {
                    // Phase 1: extract validated token from SSE auth middleware
                    if let Some(token) = axum_parts.extensions.get::<ValidToken>() {
                        headers.typed_insert(token.deref().clone());
                    }
                }
                
                if let Some(axum_parts) = context.extensions.get::<axum::http::request::Parts>() {
                    // Forward the mcp-session-id header if present
                    if let Some(session_id_header) = axum_parts.headers.get("mcp-session-id") {
                        headers.insert("mcp-session-id", session_id_header.clone());
                    }
                }

                // Extract role from request path for multi-schema routing
                let role = self.extract_role(&context);
                let endpoint = self.get_endpoint_for_role(role.as_deref());

                if let Some(ref r) = role {
                    debug!("Routing request to role-specific endpoint: role={}, endpoint={}", r, endpoint);
                }

                let graphql_request = graphql::Request {
                    input: Value::from(request.arguments.clone()),
                    endpoint: &endpoint,
                    headers,
                    auth0_token_provider: self.auth0_token_provider.as_ref(),
                };
                self.operations
                    .lock()
                    .await
                    .iter()
                    .find(|op| op.as_ref().name == request.name)
                    .ok_or(tool_not_found(&request.name))?
                    .execute(graphql_request)
                    .await
            }
        };

        // Track errors for health check
        if let (Err(_), Some(health_check)) = (&result, &self.health_check) {
            health_check.record_rejection();
        }

        result
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools: Vec<_> = self
            .operations
            .lock()
            .await
            .iter()
            .map(|op| op.as_ref().clone())
            .chain(self.execute_tool.as_ref().iter().map(|e| e.tool.clone()))
            .chain(self.introspect_tool.as_ref().iter().map(|e| e.tool.clone()))
            .chain(self.search_tool.as_ref().iter().map(|e| e.tool.clone()))
            .chain(self.explorer_tool.as_ref().iter().map(|e| e.tool.clone()))
            .chain(self.validate_tool.as_ref().iter().map(|e| e.tool.clone()))
            // Phase 2 Auth0 authentication tools
            .chain(self.login_tool.as_ref().iter().map(|t| t.tool.clone()))
            .chain(self.whoami_tool.as_ref().iter().map(|t| t.tool.clone()))
            .chain(self.logout_tool.as_ref().iter().map(|t| t.tool.clone()))
            .chain(self.get_graphql_token_tool.as_ref().iter().map(|t| t.tool.clone()))
            .collect();

        // Add test manager tools if enabled
        if self.test_manager.is_some() {
            tools.extend(crate::test_manager::create_test_manager_tools());
        }

        Ok(ListToolsResult {
            next_cursor: None,
            tools,
        })
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "Apollo MCP Server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
            ..Default::default()
        }
    }
}

fn tool_not_found(name: &str) -> McpError {
    McpError::new(
        ErrorCode::METHOD_NOT_FOUND,
        format!("Tool {name} not found"),
        None,
    )
}

fn convert_arguments<T: serde::de::DeserializeOwned>(
    arguments: CallToolRequestParam,
) -> Result<T, McpError> {
    serde_json::from_value(Value::from(arguments.arguments))
        .map_err(|_| McpError::new(ErrorCode::INVALID_PARAMS, "Invalid input".to_string(), None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_operations_should_not_crash_server() {
        let schema = Schema::parse("type Query { id: String }", "schema.graphql")
            .unwrap()
            .validate()
            .unwrap();

        let running = Running {
            schema: Arc::new(Mutex::new(schema)),
            operations: Arc::new(Mutex::new(vec![])),
            headers: HeaderMap::new(),
            endpoint: "http://localhost:4000".parse().unwrap(),
            auth0_token_provider: None,
            session_manager: None,
            device_flow_manager: None,
            login_tool: None,
            whoami_tool: None,
            logout_tool: None,
            get_graphql_token_tool: None,
            execute_tool: None,
            introspect_tool: None,
            search_tool: None,
            explorer_tool: None,
            validate_tool: None,
            custom_scalar_map: None,
            peers: Arc::new(RwLock::new(vec![])),
            cancellation_token: CancellationToken::new(),
            mutation_mode: MutationMode::None,
            disable_type_description: false,
            disable_schema_description: false,
            health_check: None,
        };

        let operations = vec![
            RawOperation::from((
                "query Valid { id }".to_string(),
                Some("valid.graphql".to_string()),
            )),
            RawOperation::from((
                "query Invalid {{ id }".to_string(),
                Some("invalid.graphql".to_string()),
            )),
            RawOperation::from((
                "query { id }".to_string(),
                Some("unnamed.graphql".to_string()),
            )),
        ];

        let updated_running = running.update_operations(operations).await.unwrap();
        let updated_operations = updated_running.operations.lock().await;

        assert_eq!(updated_operations.len(), 1);
        assert_eq!(updated_operations.first().unwrap().as_ref().name, "Valid");
    }
}
