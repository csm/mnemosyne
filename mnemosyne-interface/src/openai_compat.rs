use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use mnemosyne_inference_engine::{
    ContentBlock, InferenceError, LlmBackend, LlmRequest, LlmResponse, Role, ToolCall,
};

/// Backend for any OpenAI-compatible `/v1/chat/completions` endpoint.
///
/// Works with Ollama (`http://localhost:11434/v1`), llama.cpp server
/// (`http://localhost:8080/v1`), LM Studio (`http://localhost:1234/v1`), and
/// any other server that follows the OpenAI chat completions schema, including
/// its `tools` / `tool_calls` function-calling extension.
pub struct OpenAiCompatBackend {
    client: Client,
    /// Root of the API, e.g. `http://localhost:11434/v1`. Trailing slash is stripped.
    base_url: String,
    /// Bearer token. Most local servers ignore this; pass `"local"` or `""` if unused.
    api_key: String,
}

impl OpenAiCompatBackend {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let base_url = base_url.into();
        let base_url = base_url.trim_end_matches('/').to_owned();
        Self {
            client: Client::new(),
            base_url,
            api_key: api_key.into(),
        }
    }
}

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ChatTool>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    function: ChatFunctionCall,
}

#[derive(Serialize)]
struct ChatFunctionCall {
    name: String,
    /// JSON-encoded argument object, per the OpenAI schema.
    arguments: String,
}

#[derive(Serialize)]
struct ChatTool {
    #[serde(rename = "type")]
    kind: &'static str,
    function: ChatFunctionDef,
}

#[derive(Serialize)]
struct ChatFunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<RespToolCall>,
}

#[derive(Deserialize)]
struct RespToolCall {
    id: String,
    function: RespFunction,
}

#[derive(Deserialize)]
struct RespFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

// ── LlmBackend impl ───────────────────────────────────────────────────────────

#[async_trait]
impl LlmBackend for OpenAiCompatBackend {
    async fn complete(
        &self,
        request: LlmRequest,
    ) -> mnemosyne_inference_engine::Result<LlmResponse> {
        let messages = build_messages(&request);

        let tools = request
            .tools
            .iter()
            .map(|t| ChatTool {
                kind: "function",
                function: ChatFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        let body = ChatRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            tools,
        };

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let message = resp.text().await.unwrap_or_default();
            return Err(InferenceError::Llm { status, message });
        }

        let data: ChatResponse = resp.json().await?;
        let message = data.choices.into_iter().next().map(|c| c.message);

        let (text, tool_calls) = match message {
            Some(m) => {
                let calls = m
                    .tool_calls
                    .into_iter()
                    .map(|c| ToolCall {
                        id: c.id,
                        name: c.function.name,
                        input: serde_json::from_str(&c.function.arguments).unwrap_or(Value::Null),
                    })
                    .collect();
                (m.content.unwrap_or_default(), calls)
            }
            None => (String::new(), Vec::new()),
        };

        Ok(LlmResponse {
            text,
            tool_calls,
            model: data.model.unwrap_or(request.model),
            input_tokens: data.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            output_tokens: data
                .usage
                .as_ref()
                .map(|u| u.completion_tokens)
                .unwrap_or(0),
        })
    }
}

/// Translate the provider-neutral transcript into OpenAI chat messages.
///
/// Tool results expand into their own `role: "tool"` messages (keyed by
/// `tool_call_id`); text and tool calls collapse into a single message for the
/// originating role.
fn build_messages(request: &LlmRequest) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();

    for msg in &request.messages {
        let mut text_parts: Vec<&str> = Vec::new();
        let mut tool_calls: Vec<ChatToolCall> = Vec::new();

        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => text_parts.push(text),
                ContentBlock::ToolUse { id, name, input } => tool_calls.push(ChatToolCall {
                    id: id.clone(),
                    kind: "function",
                    function: ChatFunctionCall {
                        name: name.clone(),
                        arguments: input.to_string(),
                    },
                }),
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => out.push(ChatMessage {
                    role: "tool",
                    content: Some(content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id.clone()),
                }),
            }
        }

        if text_parts.is_empty() && tool_calls.is_empty() {
            continue; // message held only tool results, already emitted above
        }

        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        out.push(ChatMessage {
            role,
            content: (!text_parts.is_empty()).then(|| text_parts.concat()),
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            tool_call_id: None,
        });
    }

    out
}
