//! Custom Tools Module
//!
//! Fetches tool definitions from an IT-Ops backend REST API at startup and
//! proxies tool execution back to the same API. This lets the MCP server
//! expose curated chat tools (metrics, cost-model, etc.) without needing
//! to know GraphQL details.
//!
//! ## Configuration
//!
//! ```yaml
//! custom_tools:
//!   enabled: true
//!   backend_url: http://localhost:4010
//!   packages: ["metrics", "scom-monitoring"]
//!   timeout_ms: 10000
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use reqwest::Client;
use reqwest::header::HeaderMap;
use rmcp::model::{CallToolResult, Content, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, error, info, warn};

use crate::errors::McpError;

/// Configuration for custom tools from the IT-Ops backend.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CustomToolsConfig {
    /// Whether custom tools are enabled.
    pub enabled: bool,

    /// Base URL of the IT-Ops backend (e.g., `http://localhost:4010`).
    pub backend_url: String,

    /// Which tool packages to load (e.g., `["metrics", "scom-monitoring"]`).
    pub packages: Vec<String>,

    /// HTTP timeout in milliseconds for discovery and execution requests.
    pub timeout_ms: u64,
}

impl CustomToolsConfig {
    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.timeout_ms > 0 {
            self.timeout_ms
        } else {
            10_000
        })
    }
}

/// A discovered custom tool with its MCP Tool definition.
#[derive(Debug, Clone)]
struct CustomTool {
    tool: Tool,
}

/// Runtime state for custom tools — holds discovered tools and execution config.
#[derive(Debug, Clone)]
pub struct CustomTools {
    client: Client,
    backend_url: String,
    tools: Vec<CustomTool>,
    tool_names: HashSet<String>,
    timeout: std::time::Duration,
}

impl CustomTools {
    /// Discover tools from the backend at startup.
    pub async fn discover(config: &CustomToolsConfig, auth_headers: &HeaderMap) -> Option<Self> {
        if !config.enabled || config.backend_url.is_empty() || config.packages.is_empty() {
            return None;
        }

        let client = Client::new();
        let timeout = config.timeout();
        let packages_param = config.packages.join(",");
        let url = format!("{}/api/mcp/tools?packages={}", config.backend_url, packages_param);

        info!(
            "Discovering custom tools from {} (packages: {})",
            config.backend_url, packages_param
        );

        let mut request = client.get(&url).timeout(timeout);
        // Forward auth headers (e.g., Bearer token)
        for (key, value) in auth_headers.iter() {
            request = request.header(key.clone(), value.clone());
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                error!("Failed to discover custom tools: {}", e);
                return None;
            }
        };

        if !response.status().is_success() {
            error!(
                "Custom tools discovery returned HTTP {}",
                response.status()
            );
            return None;
        }

        let body: Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to parse custom tools response: {}", e);
                return None;
            }
        };

        let raw_tools = match body.get("tools").and_then(|t| t.as_array()) {
            Some(arr) => arr,
            None => {
                warn!("Custom tools response has no 'tools' array");
                return None;
            }
        };

        let mut tools = Vec::new();
        let mut tool_names = HashSet::new();

        for raw in raw_tools {
            let name = raw
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let description = raw
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = raw
                .get("inputSchema")
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();

            if name.is_empty() {
                warn!("Skipping custom tool with empty name");
                continue;
            }

            debug!("Discovered custom tool: {}", name);
            tool_names.insert(name.clone());
            let tool = Tool::new(name, description, Arc::new(input_schema));
            tools.push(CustomTool { tool });
        }

        info!("Discovered {} custom tools", tools.len());

        Some(CustomTools {
            client,
            backend_url: config.backend_url.clone(),
            tools,
            tool_names,
            timeout,
        })
    }

    /// Check if this module handles a given tool name.
    pub fn handles(&self, tool_name: &str) -> bool {
        self.tool_names.contains(tool_name)
    }

    /// Return MCP Tool definitions for list_tools.
    pub fn tools(&self) -> impl Iterator<Item = Tool> + '_ {
        self.tools.iter().map(|ct| ct.tool.clone())
    }

    /// Execute a custom tool by proxying to the backend REST API.
    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: Option<&serde_json::Map<String, Value>>,
        headers: &HeaderMap,
    ) -> Result<CallToolResult, McpError> {
        let url = format!("{}/api/mcp/tools/{}", self.backend_url, tool_name);

        let input = arguments
            .map(|args| Value::Object(args.clone()))
            .unwrap_or(json!({}));

        let body = json!({ "input": input });

        let mut request = self
            .client
            .post(&url)
            .timeout(self.timeout)
            .json(&body);

        // Forward auth headers
        for (key, value) in headers.iter() {
            request = request.header(key.clone(), value.clone());
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Custom tool execution failed: {}",
                    e
                ))]));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Custom tool returned HTTP {}: {}",
                status, body
            ))]));
        }

        let body: Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to parse custom tool response: {}",
                    e
                ))]));
            }
        };

        // Extract the result from { success: true, result: ... }
        let result_value = body
            .get("result")
            .cloned()
            .unwrap_or(body.clone());

        let text = serde_json::to_string_pretty(&result_value).unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}
