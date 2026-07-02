//! The trait an MCP tool implements.

use serde_json::Value;

use crate::types::CallToolResult;

/// A single MCP tool: static metadata plus an async handler.
///
/// Implementations should report execution failures via
/// [`CallToolResult::error`] rather than panicking — errors returned this way
/// flow back to the model as tool output it can react to.
#[async_trait::async_trait]
pub trait McpTool: Send + Sync {
    /// Unique tool name (what the client passes to `tools/call`).
    fn name(&self) -> &str;

    /// Human/model-readable description shown in `tools/list`.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's `arguments` object.
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given `arguments`.
    async fn call(&self, arguments: Value) -> CallToolResult;
}
