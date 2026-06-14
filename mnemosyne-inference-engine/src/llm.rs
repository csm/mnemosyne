use crate::tool::{Tool, ToolCall, ToolResult};
use crate::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// A single block of message content. This is the provider-neutral subset of
/// the Anthropic / OpenAI content-block model; each backend translates it into
/// the wire shape its API expects. Modelling tool calls and tool results as
/// first-class content — rather than smuggling JSON through a text field — is
/// what gives the LLM a hard structural boundary between *harness tools* and
/// the *Clojure code* it reads and writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Natural-language text.
    Text { text: String },
    /// A tool invocation requested by the assistant.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// The result of a tool invocation, sent back to the assistant.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }

    /// Reconstruct an assistant turn from a model response: any leading text
    /// followed by the `tool_use` blocks the model emitted. Appending this to
    /// the running transcript keeps the follow-up `tool_result` blocks aligned
    /// to their call ids.
    pub fn assistant_turn(text: &str, tool_calls: &[ToolCall]) -> Self {
        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(ContentBlock::Text {
                text: text.to_owned(),
            });
        }
        for call in tool_calls {
            content.push(ContentBlock::ToolUse {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.input.clone(),
            });
        }
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// A user turn carrying the results of the tool calls just executed.
    pub fn tool_results(results: &[ToolResult]) -> Self {
        let content = results
            .iter()
            .map(|r| ContentBlock::ToolResult {
                tool_use_id: r.call_id.clone(),
                content: tool_result_text(&r.output),
                is_error: r.is_error,
            })
            .collect();
        Self {
            role: Role::User,
            content,
        }
    }
}

/// Render a tool-result payload as the string body providers expect: JSON
/// strings pass through unquoted, everything else is serialized.
fn tool_result_text(output: &Value) -> String {
    match output {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub messages: Vec<Message>,
    /// Tools advertised to the model via the provider's native tool-calling
    /// interface. Empty means "no tools" (plain completion).
    #[serde(default)]
    pub tools: Vec<Tool>,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// Assistant text for this turn; may be empty when the turn is a pure tool
    /// call.
    pub text: String,
    /// Tool calls the model requested this turn. An empty vec means the turn is
    /// final and `text` is the answer.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Pluggable LLM backend — implement this to add a new provider.
#[async_trait]
pub trait LlmBackend: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse>;
}
