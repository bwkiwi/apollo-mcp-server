//! Test Manager integration for IT-Ops backend testing support
//!
//! Provides MCP tools for snapshot management, test definitions, and MCP description control.
//! All business logic resides in the IT-Ops Node.js backend; this module is a thin HTTP client wrapper.

use std::time::Duration;

use reqwest::Client;
use rmcp::model::Tool;
use rmcp::schemars::JsonSchema;
use rmcp::{schemars, serde_json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

use crate::schema_from_type;

/// Test Manager tools for snapshot and test management
#[derive(Clone)]
pub struct TestManagerTools {
    client: Client,
    base_url: String,
    fallback_description: Option<String>,
}

// Input schemas for MCP tools
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotClearInput {
    /// Safety confirmation - must be true to execute
    pub confirm: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotLoadInput {
    #[serde(rename = "snapshotName")]
    pub snapshot_name: String,
    #[serde(rename = "clearFirst")]
    pub clear_first: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TestGetInput {
    #[serde(rename = "testId")]
    pub test_id: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpDescriptionSetInput {
    #[serde(rename = "additionalText")]
    pub additional_text: String,
}

#[derive(Debug, JsonSchema, Deserialize)]
pub struct EmptyInput {}

// Request parameter types for backend API calls
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SaveSnapshotParams {
    #[serde(rename = "snapshotName")]
    pub snapshot_name: String,
    pub description: String,
    #[serde(rename = "clientInstructions")]
    pub client_instructions: Option<String>,
    #[serde(rename = "expectedResults")]
    pub expected_results: Option<String>,
    #[serde(rename = "includeCalculatedData")]
    pub include_calculated_data: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SaveTestParams {
    pub name: String,
    #[serde(rename = "snapshotName")]
    pub snapshot_name: String,
    #[serde(rename = "clientActions")]
    pub client_actions: Vec<String>,
    #[serde(rename = "expectedOutcome")]
    pub expected_outcome: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct UpdateTestParams {
    pub name: Option<String>,
    #[serde(rename = "snapshotName")]
    pub snapshot_name: Option<String>,
    #[serde(rename = "clientActions")]
    pub client_actions: Option<Vec<String>>,
    #[serde(rename = "expectedOutcome")]
    pub expected_outcome: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SaveResultParams {
    #[serde(rename = "testId")]
    pub test_id: String,
    pub passed: bool,
    #[serde(rename = "actualOutcome")]
    pub actual_outcome: String,
    #[serde(rename = "executionTime")]
    pub execution_time: Option<String>,
    pub bug: bool,
    #[serde(rename = "bugDescription")]
    pub bug_description: Option<String>,
    #[serde(rename = "additionalMcpDescription")]
    pub additional_mcp_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FeatureCheckResponse {
    #[serde(rename = "testingEnabled")]
    testing_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct McpDescriptionResponse {
    description: McpDescription,
}

#[derive(Debug, Deserialize)]
struct McpDescription {
    #[serde(rename = "additionalText")]
    additional_text: String,
}

impl TestManagerTools {
    /// Create a new TestManagerTools instance
    pub fn new(base_url: String, timeout_ms: u64, fallback_description: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .expect("Failed to build HTTP client for test manager");

        Self {
            client,
            base_url,
            fallback_description,
        }
    }

    /// Detect if test manager features are enabled on the backend
    pub async fn detect_features(&self) -> bool {
        let url = format!("{}/api/test-mgr/enabled", self.base_url);

        match self.client.get(&url).send().await {
            Ok(response) => {
                match response.json::<FeatureCheckResponse>().await {
                    Ok(data) => {
                        debug!("Test manager features detected: enabled={}", data.testing_enabled);
                        data.testing_enabled
                    }
                    Err(e) => {
                        debug!("Failed to parse test manager feature check response: {}", e);
                        false
                    }
                }
            }
            Err(e) => {
                debug!("Test manager backend not available: {}", e);
                false
            }
        }
    }

    /// Get additional MCP description from backend, with fallback to config
    pub async fn get_mcp_description(&self) -> String {
        let url = format!("{}/api/test-mgr/mcp-description", self.base_url);

        match self.client.get(&url).send().await {
            Ok(response) => {
                match response.json::<McpDescriptionResponse>().await {
                    Ok(data) => {
                        debug!("Retrieved MCP description from backend");
                        data.description.additional_text
                    }
                    Err(e) => {
                        warn!("Failed to parse MCP description from backend: {}", e);
                        self.use_fallback_description()
                    }
                }
            }
            Err(e) => {
                debug!("MCP description backend not available, using fallback: {}", e);
                self.use_fallback_description()
            }
        }
    }

    fn use_fallback_description(&self) -> String {
        self.fallback_description.clone().unwrap_or_default()
    }

    /// Clear all Layer 2 data (clean slate for testing)
    pub async fn snapshot_clear(&self, confirm: bool) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/snapshot/clear", self.base_url);

        let body = serde_json::json!({ "confirm": confirm });

        self.client.post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to clear snapshot: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse clear response: {}", e))
    }

    /// Load a snapshot with optional clear first
    pub async fn snapshot_load(&self, snapshot_name: String, clear_first: bool) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/snapshot/load", self.base_url);

        let body = serde_json::json!({
            "snapshotName": snapshot_name,
            "clearFirst": clear_first
        });

        self.client.post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to load snapshot: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse load response: {}", e))
    }

    /// Save current state as a snapshot
    pub async fn snapshot_save(&self, params: SaveSnapshotParams) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/snapshot/save", self.base_url);

        self.client.post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| format!("Failed to save snapshot: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse save response: {}", e))
    }

    /// List available snapshots
    pub async fn snapshot_list(&self) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/snapshot/list", self.base_url);

        self.client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to list snapshots: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse list response: {}", e))
    }

    /// Get test definition
    pub async fn test_get(&self, test_id: String) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/test/{}", self.base_url, test_id);

        self.client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to get test: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse test response: {}", e))
    }

    /// Save new test definition
    pub async fn test_save(&self, params: SaveTestParams) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/test", self.base_url);

        self.client.post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| format!("Failed to save test: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse save test response: {}", e))
    }

    /// Update existing test definition
    pub async fn test_update(&self, test_id: String, params: UpdateTestParams) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/test/{}", self.base_url, test_id);

        self.client.put(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| format!("Failed to update test: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse update test response: {}", e))
    }

    /// Save test execution result
    pub async fn test_save_result(&self, params: SaveResultParams) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/test/{}/result", self.base_url, params.test_id);

        self.client.post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| format!("Failed to save test result: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse save result response: {}", e))
    }

    /// Set MCP description on backend
    pub async fn mcp_description_set(&self, text: String) -> Result<Value, String> {
        let url = format!("{}/api/test-mgr/mcp-description", self.base_url);

        let body = serde_json::json!({
            "additionalText": text
        });

        self.client.post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to set MCP description: {}", e))?
            .json::<Value>()
            .await
            .map_err(|e| format!("Failed to parse set description response: {}", e))
    }
}

/// Tool names for test manager MCP tools
pub const SNAPSHOT_CLEAR_TOOL: &str = "snapshot_clear";
pub const SNAPSHOT_LOAD_TOOL: &str = "snapshot_load";
pub const SNAPSHOT_SAVE_TOOL: &str = "snapshot_save";
pub const SNAPSHOT_LIST_TOOL: &str = "snapshot_list";
pub const TEST_GET_TOOL: &str = "test_get";
pub const TEST_SAVE_TOOL: &str = "test_save";
pub const TEST_UPDATE_TOOL: &str = "test_update";
pub const TEST_SAVE_RESULT_TOOL: &str = "test_save_result";
pub const MCP_DESCRIPTION_GET_TOOL: &str = "mcp_description_get";
pub const MCP_DESCRIPTION_SET_TOOL: &str = "mcp_description_set";

/// Create MCP tool definitions for test manager
pub fn create_test_manager_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            SNAPSHOT_CLEAR_TOOL,
            "Clear all Layer 2 data (applications, deployments, resources, entities, etc.) to get a clean slate for testing. Preserves regions, internal users, and sessions.",
            schema_from_type!(SnapshotClearInput),
        ),
        Tool::new(
            SNAPSHOT_LOAD_TOOL,
            "Load a saved snapshot to restore a known test state. Optionally clear data first.",
            schema_from_type!(SnapshotLoadInput),
        ),
        Tool::new(
            SNAPSHOT_SAVE_TOOL,
            "Save current data state as a snapshot for later use in tests.",
            schema_from_type!(SaveSnapshotParams),
        ),
        Tool::new(
            SNAPSHOT_LIST_TOOL,
            "List all available snapshots with their metadata.",
            schema_from_type!(EmptyInput),
        ),
        Tool::new(
            TEST_GET_TOOL,
            "Retrieve a test definition by ID.",
            schema_from_type!(TestGetInput),
        ),
        Tool::new(
            TEST_SAVE_TOOL,
            "Save a new test definition with client actions and expected outcomes.",
            schema_from_type!(SaveTestParams),
        ),
        Tool::new(
            TEST_UPDATE_TOOL,
            "Update an existing test definition.",
            schema_from_type!(UpdateTestParams),
        ),
        Tool::new(
            TEST_SAVE_RESULT_TOOL,
            "Save test execution results. If bug=true, captures current state as a bug snapshot.",
            schema_from_type!(SaveResultParams),
        ),
        Tool::new(
            MCP_DESCRIPTION_GET_TOOL,
            "Get the current additional MCP description text.",
            schema_from_type!(EmptyInput),
        ),
        Tool::new(
            MCP_DESCRIPTION_SET_TOOL,
            "Set additional MCP description text to control what information is shown to Claude.",
            schema_from_type!(McpDescriptionSetInput),
        ),
    ]
}
