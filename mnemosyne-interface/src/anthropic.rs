use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use mnemosyne_inference_engine::{
    ContentBlock, InferenceError, LlmBackend, LlmRequest, LlmResponse, Role, ToolCall,
};

/// Anthropic Messages API backend.
pub struct AnthropicBackend {
    client: Client,
    api_key: String,
}

impl AnthropicBackend {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }
}

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ApiTool>,
}

#[derive(Serialize)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Serialize)]
struct ApiMessage {
    role: &'static str,
    content: Vec<ApiContentBlock>,
}

/// Outgoing content blocks, in the shape the Messages API expects.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ApiContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<RespBlock>,
    model: String,
    usage: Usage,
}

/// Incoming content blocks. Unknown block types (e.g. `thinking`) are ignored.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RespBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
}

// ── LlmBackend impl ───────────────────────────────────────────────────────────

#[async_trait]
impl LlmBackend for AnthropicBackend {
    async fn complete(
        &self,
        request: LlmRequest,
    ) -> mnemosyne_inference_engine::Result<LlmResponse> {
        // Anthropic carries the system prompt as a top-level field, not as a
        // message, so it is split out here.
        let mut system: Option<String> = None;
        let mut messages: Vec<ApiMessage> = Vec::new();

        for msg in &request.messages {
            if msg.role == Role::System {
                system = Some(concat_text(&msg.content));
                continue;
            }
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => unreachable!(),
            };
            messages.push(ApiMessage {
                role,
                content: msg.content.iter().map(to_api_block).collect(),
            });
        }

        let tools = request
            .tools
            .iter()
            .map(|t| ApiTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let body = ApiRequest {
            model: request.model,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            system,
            messages,
            tools,
        };

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let message = resp.text().await.unwrap_or_default();
            return Err(InferenceError::Llm { status, message });
        }

        let data: ApiResponse = resp.json().await?;

        let mut text = String::new();
        let mut tool_calls = Vec::new();
        for block in data.content {
            match block {
                RespBlock::Text { text: t } => text.push_str(&t),
                RespBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall { id, name, input })
                }
                RespBlock::Other => {}
            }
        }

        Ok(LlmResponse {
            text,
            tool_calls,
            model: data.model,
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
        })
    }
}

/// Concatenate the text blocks of a message (used for the system prompt).
fn concat_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn to_api_block(block: &ContentBlock) -> ApiContentBlock {
    match block {
        ContentBlock::Text { text } => ApiContentBlock::Text { text: text.clone() },
        ContentBlock::ToolUse { id, name, input } => ApiContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ApiContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        },
    }
}
