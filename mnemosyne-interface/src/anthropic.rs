use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use mnemosyne_inference_engine::{InferenceError, LlmBackend, LlmRequest, LlmResponse, Role};

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
}

#[derive(Serialize)]
struct ApiMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    model: String,
    usage: Usage,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
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
        let mut system: Option<String> = None;
        let mut messages: Vec<ApiMessage> = Vec::new();

        for msg in request.messages {
            match msg.role {
                Role::System => {
                    system = Some(msg.content);
                }
                Role::User => messages.push(ApiMessage {
                    role: "user",
                    content: msg.content,
                }),
                Role::Assistant => messages.push(ApiMessage {
                    role: "assistant",
                    content: msg.content,
                }),
            }
        }

        let body = ApiRequest {
            model: request.model,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            system,
            messages,
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
        let content = data
            .content
            .iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse {
            content,
            model: data.model,
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
        })
    }
}
