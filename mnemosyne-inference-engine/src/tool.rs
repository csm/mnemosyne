use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Descriptor for a tool the LLM may invoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A tool invocation requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// The result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: String,
    pub output: Value,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(call_id: impl Into<String>, output: impl Serialize) -> Self {
        Self {
            call_id: call_id.into(),
            output: serde_json::to_value(output).unwrap_or(Value::Null),
            is_error: false,
        }
    }

    pub fn err(call_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            output: Value::String(message.into()),
            is_error: true,
        }
    }
}

/// Tool definitions that the inference engine exposes to the LLM.
pub fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "search_code".into(),
            description: "Search indexed repositories for functions matching a query. \
                Combines full-text (keyword) and semantic (intent-based) search when \
                the semantic index is loaded; falls back to full-text only otherwise. \
                Results include a source field: \"fulltext\", \"semantic\", or \"both\".".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Functionality description" },
                    "limit": { "type": "integer", "default": 5 }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "eval_clojure".into(),
            description: "Evaluate a Clojure expression and return its printed result.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Clojure source to evaluate" }
                },
                "required": ["source"]
            }),
        },
        Tool {
            name: "edit_function".into(),
            description: "Apply a structural edit to a Clojure function in a repository file.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "repo": { "type": "string" },
                    "file": { "type": "string" },
                    "edit": { "type": "object", "description": "Serialised Edit variant" }
                },
                "required": ["repo", "file", "edit"]
            }),
        },
    ]
}
