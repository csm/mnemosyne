use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use mnemosyne_inference_engine::{InferenceError, LlmBackend, LlmRequest, LlmResponse, Role};

/// Backend for any OpenAI-compatible `/v1/chat/completions` endpoint.
///
/// Works with Ollama (`http://localhost:11434/v1`), llama.cpp server
/// (`http://localhost:8080/v1`), LM Studio (`http://localhost:1234/v1`), and
/// any other server that follows the OpenAI chat completions schema.
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
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
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
    content: String,
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
        let messages: Vec<ChatMessage> = request
            .messages
            .into_iter()
            .map(|m| ChatMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: m.content,
            })
            .collect();

        let body = ChatRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
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
        let content = data
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
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
