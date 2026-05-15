use crate::{
    llm::{LlmBackend, LlmRequest, Message, Role},
    tool::{ToolCall, ToolResult},
    Result,
};
use mnemosyne_code_search::{CodeIndex, SearchQuery};
use mnemosyne_execution_engine::ClojureRuntime;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct InferenceEngine {
    llm: Arc<dyn LlmBackend>,
    runtime: Arc<Mutex<ClojureRuntime>>,
    index: Arc<CodeIndex>,
    pub default_model: String,
    pub system_prompt: String,
}

impl InferenceEngine {
    pub fn new(
        llm: Arc<dyn LlmBackend>,
        runtime: ClojureRuntime,
        index: CodeIndex,
    ) -> Self {
        Self {
            llm,
            runtime: Arc::new(Mutex::new(runtime)),
            index: Arc::new(index),
            default_model: "claude-opus-4-7".into(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
        }
    }

    /// Run a single-turn prompt, dispatching any tool calls before returning
    /// the final assistant response.
    pub async fn run(&self, user_message: impl Into<String>) -> Result<String> {
        let mut messages = vec![
            Message::system(&self.system_prompt),
            Message::user(user_message.into()),
        ];

        loop {
            let req = LlmRequest {
                messages: messages.clone(),
                model: self.default_model.clone(),
                max_tokens: 4096,
                temperature: 0.2,
            };

            let response = self.llm.complete(req).await?;
            let content = response.content.clone();

            // If the response is plain text (no tool calls), we're done.
            if !content.trim_start().starts_with('{') {
                return Ok(content);
            }

            // Attempt to parse as a tool call batch.
            match serde_json::from_str::<Vec<ToolCall>>(&content) {
                Ok(calls) => {
                    messages.push(Message::assistant(&content));
                    let results = self.dispatch_tools(&calls).await;
                    let result_json = serde_json::to_string(&results)?;
                    messages.push(Message {
                        role: Role::User,
                        content: result_json,
                    });
                }
                Err(_) => return Ok(content),
            }
        }
    }

    async fn dispatch_tools(&self, calls: &[ToolCall]) -> Vec<ToolResult> {
        let mut results = Vec::new();
        for call in calls {
            let result = self.dispatch_single(call).await;
            results.push(result);
        }
        results
    }

    async fn dispatch_single(&self, call: &ToolCall) -> ToolResult {
        match call.name.as_str() {
            "search_code" => {
                let query = call.input["query"].as_str().unwrap_or("").to_owned();
                let limit = call.input["limit"].as_u64().unwrap_or(5) as usize;
                match self.index.search(&SearchQuery::new(query).with_limit(limit)) {
                    Ok(results) => ToolResult::ok(&call.id, results),
                    Err(e) => ToolResult::err(&call.id, e.to_string()),
                }
            }
            "eval_clojure" => {
                let source = call.input["source"].as_str().unwrap_or("").to_owned();
                let mut rt = self.runtime.lock().await;
                match rt.eval(&source) {
                    Ok(val) => ToolResult::ok(&call.id, val.to_string()),
                    Err(e) => ToolResult::err(&call.id, e.to_string()),
                }
            }
            "edit_function" => {
                // Structural edits are applied against in-memory source for now;
                // persistence back to the repo is handled by code_storage.
                ToolResult::err(&call.id, "edit_function not yet fully wired")
            }
            other => ToolResult::err(&call.id, format!("unknown tool: {other}")),
        }
    }
}

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are Mnemosyne, an AI programming assistant with access to tools for \
searching code repositories, evaluating Clojure expressions, and editing \
Clojure functions. Use these tools to answer questions and complete tasks. \
Prefer precise, minimal edits over large rewrites.";
