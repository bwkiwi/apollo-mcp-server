use std::path::PathBuf;
use std::sync::Arc;
use std::fs;

use apollo_mcp_registry::platform_api::operation_collections::collection_poller::CollectionSource;
use apollo_mcp_registry::uplink::persisted_queries::ManifestSource;
use apollo_mcp_registry::uplink::schema::SchemaSource;
use apollo_mcp_server::auth::{
    SessionManager, MemorySessionStorage, DeviceFlowManager, SessionStorage
};
use apollo_mcp_server::custom_scalar_map::CustomScalarMap;
use apollo_mcp_server::errors::ServerError;
use apollo_mcp_server::operations::OperationSource;
use apollo_mcp_server::auth::SessionStorageType;
use apollo_mcp_server::schema_loader::SchemaCache;
use apollo_mcp_server::server::Server;
use apollo_mcp_server::test_manager::TestManagerTools;
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};
use itops_ai_auth::Auth0TokenProvider;
use runtime::IdOrDefault;
use runtime::logging::Logging;
use tokio::sync::Mutex;
use tracing::{info, warn};

mod runtime;

/// Clap styling
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

/// Arguments to the MCP server
#[derive(Debug, Parser)]
#[command(
    version,
    styles = STYLES,
    about = "Apollo MCP Server - invoke GraphQL operations from an AI agent",
)]
struct Args {
    /// Path to the config file
    config: Option<PathBuf>,
}

/// Validate that the configuration file exists and provide helpful error messages
fn validate_config_file(config_path: &PathBuf) -> anyhow::Result<()> {
    // Check if the file exists
    if !config_path.exists() {
        // Check if the parent directory exists
        if let Some(parent_dir) = config_path.parent() {
            if !parent_dir.exists() {
                anyhow::bail!(
                    "Configuration directory does not exist: {}\n\n\
                    Help:\n\
                    • Create the directory: mkdir -p {}\n\
                    • Ensure you have the correct path to your configuration file",
                    parent_dir.display(),
                    parent_dir.display()
                );
            }
        }
        
        // Directory exists but file doesn't
        anyhow::bail!(
            "Configuration file does not exist: {}\n\n\
            Help:\n\
            • Check the file path is correct\n\
            • Create a configuration file at this location\n\
            • Use the example configuration from the documentation\n\
            • Available at: https://github.com/apollographql/apollo-mcp-server/tree/main/config",
            config_path.display()
        );
    }
    
    // Check if it's actually a file (not a directory)
    if config_path.is_dir() {
        anyhow::bail!(
            "Configuration path is a directory, not a file: {}\n\n\
            Help:\n\
            • Specify the actual configuration file (e.g., {}/config.yaml)\n\
            • Ensure you're pointing to a file, not a directory",
            config_path.display(),
            config_path.display()
        );
    }
    
    // Check if the file is readable
    match fs::File::open(config_path) {
        Ok(_) => Ok(()),
        Err(e) => {
            anyhow::bail!(
                "Cannot read configuration file: {}\n\
                Error: {}\n\n\
                Help:\n\
                • Check file permissions (should be readable)\n\
                • Ensure the file is not locked by another process\n\
                • Verify you have access to the file",
                config_path.display(),
                e
            );
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    
    let config: runtime::Config = match args.config {
        Some(config_path) => {
            // Validate the configuration file before attempting to read it
            validate_config_file(&config_path)?;
            info!("Loading config from file: {}", config_path.display());
            let loaded_config = runtime::read_config(config_path)?;
            
            // Debug: Log auth0 config status
            if let Some(ref auth0) = loaded_config.auth0 {
                info!("Auth0 config found - domain: {}", auth0.domain);
                info!("Auth0 client_id from config: {}", auth0.client_id);
                if auth0.is_per_session_enabled() {
                    info!("Auth0 per-session authentication is ENABLED");
                    if let Some(ref per_session) = auth0.per_session_auth {
                        info!("Device flow client_id from config: {}", per_session.device_flow_client_id);
                    }
                } else if auth0.refresh_token.is_some() {
                    info!("Auth0 Phase 1 (refresh token) authentication is ENABLED");
                } else {
                    warn!("Auth0 config present but no valid authentication method");
                }
            } else {
                info!("No Auth0 configuration found in config file");
            }
            
            loaded_config
        }
        None => runtime::read_config_from_env().unwrap_or_default(),
    };

    // WorkerGuard is not used but needed to be at least defined or else the guard
    // is cleaned up too early and file appender logging does not work
    let _guard = Logging::setup(&config)?;

    info!(
        "Apollo MCP Server v{} // (c) Apollo Graph, Inc. // Licensed under MIT",
        env!("CARGO_PKG_VERSION")
    );

    let schema_source = match config.schema {
        runtime::SchemaSource::Local { path } => SchemaSource::File { path, watch: true },
        runtime::SchemaSource::Uplink => SchemaSource::Registry(config.graphos.uplink_config()?),
    };

    let operation_source = match config.operations {
        // Default collection is special and requires other information
        runtime::OperationSource::Collection {
            id: IdOrDefault::Default,
        } => OperationSource::Collection(CollectionSource::Default(
            config.graphos.graph_ref()?,
            config.graphos.platform_api_config()?,
        )),

        runtime::OperationSource::Collection {
            id: IdOrDefault::Id(collection_id),
        } => OperationSource::Collection(CollectionSource::Id(
            collection_id,
            config.graphos.platform_api_config()?,
        )),
        runtime::OperationSource::Introspect => OperationSource::None,
        runtime::OperationSource::Local { paths } if !paths.is_empty() => {
            OperationSource::from(paths)
        }
        runtime::OperationSource::Manifest { path } => {
            OperationSource::from(ManifestSource::LocalHotReload(vec![path]))
        }
        runtime::OperationSource::Uplink => {
            OperationSource::from(ManifestSource::Uplink(config.graphos.uplink_config()?))
        }

        // TODO: Inference requires many different combinations and preferences
        // TODO: We should maybe make this more explicit.
        runtime::OperationSource::Local { .. } | runtime::OperationSource::Infer => {
            if config.introspection.any_enabled() {
                warn!("No operations specified, falling back to introspection");
                OperationSource::None
            } else if let Ok(graph_ref) = config.graphos.graph_ref() {
                warn!(
                    "No operations specified, falling back to the default collection in {}",
                    graph_ref
                );
                OperationSource::Collection(CollectionSource::Default(
                    graph_ref,
                    config.graphos.platform_api_config()?,
                ))
            } else {
                anyhow::bail!(ServerError::NoOperations)
            }
        }
    };

    let explorer_graph_ref = config
        .overrides
        .enable_explorer
        .then(|| config.graphos.graph_ref())
        .transpose()?;

    // Create Auth0 components based on configuration
    info!("Checking Auth0 configuration...");
    let (auth0_token_provider, session_manager, device_flow_manager) = match &config.auth0 {
        Some(auth0_config) if auth0_config.is_per_session_enabled() => {
            // Phase 2: Per-session authentication
            info!("Initializing Auth0 Phase 2 (per-session authentication)");
            info!("  Domain: {}", auth0_config.domain);
            info!("  Client ID: {}", auth0_config.client_id);
            info!("  Device Flow Client ID: {:?}", auth0_config.device_flow_client_id());
            
            // Create session storage
            let storage: Box<dyn SessionStorage> = match &auth0_config.per_session_auth
                .as_ref()
                .unwrap()
                .session_storage 
            {
                SessionStorageType::Memory => {
                    info!("Using in-memory session storage");
                    Box::new(MemorySessionStorage::new())
                }
                SessionStorageType::File { path } => {
                    info!("Using file-based session storage at: {}", path);
                    // TODO: Implement FileSessionStorage
                    warn!("File storage not yet implemented, falling back to memory storage");
                    Box::new(MemorySessionStorage::new())
                }
            };
            
            let session_manager = Arc::new(SessionManager::new(auth0_config.clone(), storage));
            let device_flow_manager = Arc::new(DeviceFlowManager::new(auth0_config.clone()));
            
            info!("Auth0 Phase 2 components created successfully");
            (None, Some(session_manager), Some(device_flow_manager))
        }
        Some(auth0_config) if auth0_config.refresh_token.is_some() => {
            // Phase 1: Shared authentication (backward compatibility)
            info!("Initializing Auth0 Phase 1 (shared authentication)");
            info!("  Domain: {}", auth0_config.domain);
            info!("  Client ID: {}", auth0_config.client_id);
            
            let provider = Auth0TokenProvider::new(
                auth0_config.domain.clone(),
                auth0_config.client_id.clone(),
                auth0_config.audience.clone(),
                auth0_config.refresh_token.as_ref().unwrap().clone(),
            );
            
            info!("Auth0 Phase 1 token provider created successfully");
            (Some(Arc::new(Mutex::new(provider))), None, None)
        }
        Some(auth0_config) => {
            warn!("Auth0 configured but no valid authentication method specified");
            warn!("  Domain: {}", auth0_config.domain);
            warn!("  Has refresh_token: {}", auth0_config.refresh_token.is_some());
            warn!("  Per-session enabled: {}", auth0_config.is_per_session_enabled());
            (None, None, None)
        }
        None => {
            info!("No Auth0 configuration found");
            // No Auth0 configuration
            (None, None, None)
        }
    };

    // Initialize test manager if enabled
    let test_manager = if config.test_manager.enabled {
        info!("Initializing Test Manager integration");

        // Derive backend URL from endpoint if not explicitly configured
        let backend_url = config.test_manager.backend_url
            .clone()
            .unwrap_or_else(|| {
                // Extract base URL from GraphQL endpoint (remove /graphql suffix)
                let endpoint_str = config.endpoint.as_str();
                if let Some(base) = endpoint_str.strip_suffix("/graphql") {
                    base.to_string()
                } else {
                    // Try to extract just scheme://host:port
                    config.endpoint
                        .origin()
                        .ascii_serialization()
                }
            });

        info!("  Backend URL: {}", backend_url);
        info!("  Timeout: {}ms", config.test_manager.timeout_ms);

        let tools = TestManagerTools::new(
            backend_url.clone(),
            config.test_manager.timeout_ms,
            config.test_manager.fallback_description.clone(),
        );

        // Attempt feature detection
        if tools.detect_features().await {
            info!("  Test Manager backend detected and ready");

            // Try to load MCP description
            let description = tools.get_mcp_description().await;
            if !description.is_empty() {
                info!("  Retrieved MCP description from backend ({} chars)", description.len());
            } else if config.test_manager.fallback_description.is_some() {
                info!("  Using fallback MCP description from config");
            }

            Some(Arc::new(tools))
        } else {
            warn!("  Test Manager backend not available at {}", backend_url);
            if config.test_manager.fallback_description.is_some() {
                warn!("  Continuing with fallback MCP description from config");
                Some(Arc::new(tools))
            } else {
                warn!("  Disabling Test Manager features");
                None
            }
        }
    } else {
        info!("Test Manager integration disabled");
        None
    };

    // Load role-based schemas if configured
    let (schema_cache, role_config) = match &config.roles {
        Some(roles_config) => {
            info!("Loading role-based schemas from GraphQL backend");
            info!("  Base URL: {}", roles_config.graphql_base_url);
            info!("  Available roles: {:?}", roles_config.available_roles);
            info!("  Default role: {}", roles_config.default_role);

            match SchemaCache::load_from_backend(
                &roles_config.graphql_base_url,
                &roles_config.available_roles,
            )
            .await
            {
                Ok(cache) => {
                    info!("Successfully loaded schemas for {} roles", roles_config.available_roles.len());
                    for role in &roles_config.available_roles {
                        if cache.has_role(role) {
                            info!("  ✓ Schema loaded for role: {}", role);
                        } else {
                            warn!("  ✗ Failed to load schema for role: {}", role);
                        }
                    }
                    (Some(Arc::new(cache)), Some(roles_config.clone()))
                }
                Err(e) => {
                    warn!("Failed to load role-based schemas: {}", e);
                    warn!("Continuing without role-based routing");
                    (None, None)
                }
            }
        }
        None => {
            info!("No role-based routing configured");
            (None, None)
        }
    };

    Ok(Server::builder()
        .transport(config.transport)
        .schema_source(schema_source)
        .operation_source(operation_source)
        .endpoint(config.endpoint.into_inner())
        .maybe_explorer_graph_ref(explorer_graph_ref)
        .headers(config.headers)
        .maybe_auth0_token_provider(auth0_token_provider)
        .maybe_session_manager(session_manager)
        .maybe_device_flow_manager(device_flow_manager)
        .maybe_schema_cache(schema_cache)
        .maybe_role_config(role_config)
        .maybe_test_manager(test_manager)
        .execute_introspection(config.introspection.execute.enabled)
        .validate_introspection(config.introspection.validate.enabled)
        .introspect_introspection(config.introspection.introspect.enabled)
        .introspect_minify(config.introspection.introspect.minify)
        .search_minify(config.introspection.search.minify)
        .search_introspection(config.introspection.search.enabled)
        .mutation_mode(config.overrides.mutation_mode)
        .disable_type_description(config.overrides.disable_type_description)
        .disable_schema_description(config.overrides.disable_schema_description)
        .custom_scalar_map(
            config
                .custom_scalars
                .map(|custom_scalars_config| CustomScalarMap::try_from(&custom_scalars_config))
                .transpose()?,
        )
        .search_leaf_depth(config.introspection.search.leaf_depth)
        .index_memory_bytes(config.introspection.search.index_memory_bytes)
        .health_check(config.health_check)
        .build()
        .start()
        .await?)
}
