//! Schema loading and caching for role-based GraphQL endpoints

use std::collections::HashMap;

use apollo_compiler::{Schema, validation::Valid};
use reqwest::Client;
use serde_json::json;
use thiserror::Error;
use tracing::{debug, info, warn};
use url::Url;

#[derive(Debug, Error)]
pub enum SchemaLoadError {
    #[error("Failed to fetch schema from GraphQL endpoint: {0}")]
    FetchError(#[from] reqwest::Error),

    #[error("Failed to parse GraphQL schema: {0}")]
    ParseError(String),

    #[error("Failed to validate GraphQL schema: {0}")]
    ValidationError(String),

    #[error("GraphQL endpoint returned error: {0}")]
    GraphQLError(String),

    #[error("No __schema field in introspection response")]
    MissingSchemaField,
}

/// Cache for role-specific GraphQL schemas
#[derive(Debug, Clone)]
pub struct SchemaCache {
    schemas: HashMap<String, Valid<Schema>>,
}

impl SchemaCache {
    /// Create a new empty schema cache
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Load schemas from GraphQL backend for all specified roles
    pub async fn load_from_backend(
        base_url: &Url,
        roles: &[String],
    ) -> Result<Self, SchemaLoadError> {
        info!("Loading schemas from GraphQL backend at {}", base_url);
        let mut schemas = HashMap::new();

        for role in roles {
            let endpoint = format!("{}/graphql/{}", base_url, role);
            info!("Fetching schema for role '{}' from {}", role, endpoint);

            match fetch_schema_from_graphql(&endpoint).await {
                Ok(schema) => {
                    info!("Successfully loaded schema for role '{}' ({} types)",
                          role, schema.types.len());
                    schemas.insert(role.clone(), schema);
                }
                Err(e) => {
                    warn!("Failed to load schema for role '{}': {}", role, e);
                    return Err(e);
                }
            }
        }

        Ok(Self { schemas })
    }

    /// Get a schema for a specific role
    pub fn get_schema(&self, role: &str) -> Option<&Valid<Schema>> {
        self.schemas.get(role)
    }

    /// Get all available roles
    pub fn available_roles(&self) -> Vec<String> {
        self.schemas.keys().cloned().collect()
    }

    /// Check if a role exists in the cache
    pub fn has_role(&self, role: &str) -> bool {
        self.schemas.contains_key(role)
    }
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Fetch a GraphQL schema from an endpoint using introspection
pub async fn fetch_schema_from_graphql(endpoint: &str) -> Result<Valid<Schema>, SchemaLoadError> {
    info!("🔍 Executing introspection query against {}", endpoint);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))  // 10 second timeout
        .build()
        .map_err(|e| SchemaLoadError::FetchError(e))?;

    // Use the standard introspection query
    let introspection_query = r#"
        query IntrospectionQuery {
            __schema {
                queryType { name }
                mutationType { name }
                subscriptionType { name }
                types {
                    ...FullType
                }
                directives {
                    name
                    description
                    locations
                    args {
                        ...InputValue
                    }
                }
            }
        }
        fragment FullType on __Type {
            kind
            name
            description
            fields(includeDeprecated: true) {
                name
                description
                args {
                    ...InputValue
                }
                type {
                    ...TypeRef
                }
                isDeprecated
                deprecationReason
            }
            inputFields {
                ...InputValue
            }
            interfaces {
                ...TypeRef
            }
            enumValues(includeDeprecated: true) {
                name
                description
                isDeprecated
                deprecationReason
            }
            possibleTypes {
                ...TypeRef
            }
        }
        fragment InputValue on __InputValue {
            name
            description
            type {
                ...TypeRef
            }
            defaultValue
        }
        fragment TypeRef on __Type {
            kind
            name
            ofType {
                kind
                name
                ofType {
                    kind
                    name
                    ofType {
                        kind
                        name
                        ofType {
                            kind
                            name
                            ofType {
                                kind
                                name
                                ofType {
                                    kind
                                    name
                                    ofType {
                                        kind
                                        name
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    "#;

    let request_body = json!({
        "query": introspection_query,
    });

    info!("📡 Sending introspection request to {}", endpoint);
    let response = client
        .post(endpoint)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            warn!("❌ Failed to send request to {}: {}", endpoint, e);
            SchemaLoadError::FetchError(e)
        })?;

    info!("📥 Received response from {} with status {}", endpoint, response.status());

    if !response.status().is_success() {
        return Err(SchemaLoadError::GraphQLError(format!(
            "HTTP {} from GraphQL endpoint",
            response.status()
        )));
    }

    let response_json: serde_json::Value = response.json().await?;

    // Check for GraphQL errors
    if let Some(errors) = response_json.get("errors") {
        return Err(SchemaLoadError::GraphQLError(format!(
            "GraphQL errors: {}",
            errors
        )));
    }

    // Extract the schema from the introspection result
    let schema_data = response_json
        .get("data")
        .and_then(|d| d.get("__schema"))
        .ok_or(SchemaLoadError::MissingSchemaField)?;

    // Convert introspection JSON to SDL (Schema Definition Language)
    let sdl = introspection_to_sdl(schema_data)?;

    debug!("Converting SDL to apollo_compiler Schema");

    // Parse the SDL into a Schema
    let schema = Schema::parse(&sdl, "schema.graphql")
        .map_err(|e| SchemaLoadError::ParseError(format!("{:?}", e)))?;

    // Validate the schema
    let validated_schema = schema
        .validate()
        .map_err(|e| SchemaLoadError::ValidationError(format!("{:?}", e)))?;

    Ok(validated_schema)
}

/// Convert introspection JSON to GraphQL SDL
fn introspection_to_sdl(schema_data: &serde_json::Value) -> Result<String, SchemaLoadError> {
    // This is a simplified conversion - in production you might want to use
    // a more robust library or the apollo_compiler's introspection support

    // For now, we'll create a basic SDL representation
    let mut sdl = String::new();

    // Add schema definition
    if let Some(query_type) = schema_data.get("queryType").and_then(|q| q.get("name")) {
        sdl.push_str("schema {\n");
        sdl.push_str(&format!("  query: {}\n", query_type.as_str().unwrap_or("Query")));

        if let Some(mutation_type) = schema_data.get("mutationType").and_then(|m| m.get("name")) {
            sdl.push_str(&format!("  mutation: {}\n", mutation_type.as_str().unwrap_or("Mutation")));
        }

        if let Some(subscription_type) = schema_data.get("subscriptionType").and_then(|s| s.get("name")) {
            sdl.push_str(&format!("  subscription: {}\n", subscription_type.as_str().unwrap_or("Subscription")));
        }

        sdl.push_str("}\n\n");
    }

    // Add types
    if let Some(types) = schema_data.get("types").and_then(|t| t.as_array()) {
        for type_obj in types {
            if let Some(type_name) = type_obj.get("name").and_then(|n| n.as_str()) {
                // Skip introspection types
                if type_name.starts_with("__") {
                    continue;
                }

                if let Some(kind) = type_obj.get("kind").and_then(|k| k.as_str()) {
                    match kind {
                        "OBJECT" => {
                            sdl.push_str(&format!("type {} {{\n", type_name));
                            if let Some(fields) = type_obj.get("fields").and_then(|f| f.as_array()) {
                                for field in fields {
                                    if let Some(field_name) = field.get("name").and_then(|n| n.as_str()) {
                                        if let Some(field_type) = field.get("type") {
                                            let type_str = format_type_ref(field_type);
                                            sdl.push_str(&format!("  {}: {}\n", field_name, type_str));
                                        }
                                    }
                                }
                            }
                            sdl.push_str("}\n\n");
                        }
                        "SCALAR" => {
                            // Skip built-in scalars
                            if !matches!(type_name, "String" | "Int" | "Float" | "Boolean" | "ID") {
                                sdl.push_str(&format!("scalar {}\n\n", type_name));
                            }
                        }
                        "ENUM" => {
                            sdl.push_str(&format!("enum {} {{\n", type_name));
                            if let Some(enum_values) = type_obj.get("enumValues").and_then(|e| e.as_array()) {
                                for value in enum_values {
                                    if let Some(value_name) = value.get("name").and_then(|n| n.as_str()) {
                                        sdl.push_str(&format!("  {}\n", value_name));
                                    }
                                }
                            }
                            sdl.push_str("}\n\n");
                        }
                        "INTERFACE" => {
                            sdl.push_str(&format!("interface {} {{\n", type_name));
                            if let Some(fields) = type_obj.get("fields").and_then(|f| f.as_array()) {
                                for field in fields {
                                    if let Some(field_name) = field.get("name").and_then(|n| n.as_str()) {
                                        if let Some(field_type) = field.get("type") {
                                            let type_str = format_type_ref(field_type);
                                            sdl.push_str(&format!("  {}: {}\n", field_name, type_str));
                                        }
                                    }
                                }
                            }
                            sdl.push_str("}\n\n");
                        }
                        "UNION" => {
                            sdl.push_str(&format!("union {} = ", type_name));
                            if let Some(possible_types) = type_obj.get("possibleTypes").and_then(|p| p.as_array()) {
                                let type_names: Vec<String> = possible_types
                                    .iter()
                                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
                                    .collect();
                                sdl.push_str(&type_names.join(" | "));
                            }
                            sdl.push_str("\n\n");
                        }
                        "INPUT_OBJECT" => {
                            sdl.push_str(&format!("input {} {{\n", type_name));
                            if let Some(input_fields) = type_obj.get("inputFields").and_then(|f| f.as_array()) {
                                for field in input_fields {
                                    if let Some(field_name) = field.get("name").and_then(|n| n.as_str()) {
                                        if let Some(field_type) = field.get("type") {
                                            let type_str = format_type_ref(field_type);
                                            sdl.push_str(&format!("  {}: {}\n", field_name, type_str));
                                        }
                                    }
                                }
                            }
                            sdl.push_str("}\n\n");
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(sdl)
}

/// Format a type reference from introspection JSON
fn format_type_ref(type_obj: &serde_json::Value) -> String {
    if let Some(kind) = type_obj.get("kind").and_then(|k| k.as_str()) {
        match kind {
            "NON_NULL" => {
                if let Some(of_type) = type_obj.get("ofType") {
                    return format!("{}!", format_type_ref(of_type));
                }
            }
            "LIST" => {
                if let Some(of_type) = type_obj.get("ofType") {
                    return format!("[{}]", format_type_ref(of_type));
                }
            }
            _ => {
                if let Some(name) = type_obj.get("name").and_then(|n| n.as_str()) {
                    return name.to_string();
                }
            }
        }
    }
    "Unknown".to_string()
}