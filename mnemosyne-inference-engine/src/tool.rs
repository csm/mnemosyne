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
                Results include a source field: \"fulltext\", \"semantic\", or \"both\"."
                .into(),
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
            description: "Apply a structural edit to an existing Clojure function in a \
                repository file and commit it. Use this to evolve a function into a new \
                version — the returned commit hash is the new pin. Variants: ReplaceBody, \
                PrependToBody, WrapBody, Rename, InsertAfter."
                .into(),
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
        Tool {
            name: "recall_memory".into(),
            description: "Recall recent episodes (user messages, tool calls, tool results, \
                assistant replies) from the episodic memory log to recover context from \
                earlier in the session."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "default": 10 }
                }
            }),
        },
        Tool {
            name: "load_versioned_symbol".into(),
            description: "Load a git-pinned Clojure symbol or namespace into the live \
                runtime so it can be called. The registry resolves the versioned ref, \
                verifies the commit signature, and applies the trust policy. Ref syntax: \
                `namespace/symbol@<commit>`, `namespace@<commit>`, or \
                `https://host/u/r::ns/sym@<commit>` for external repositories."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "vref": {
                        "type": "string",
                        "description": "Versioned ref, e.g. mnemosyne.core/deep-merge@a1b2c3d4"
                    }
                },
                "required": ["vref"]
            }),
        },
    ]
}
