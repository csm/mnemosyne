use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Mutex;
use tracing::warn;

use mnemosyne_code_editor::{edit_description, Edit, Editor};
use mnemosyne_code_search::{CodeIndex, IndexedFunction, SearchQuery, SearchResult as FtResult};
use mnemosyne_code_storage::CodeRepository;
use mnemosyne_execution_engine::ClojureRuntime;
use mnemosyne_semantic_search::{SemanticIndex, SemanticResult};

use crate::{
    llm::{LlmBackend, LlmRequest, Message, Role},
    tool::{ToolCall, ToolResult},
    Result,
};

pub struct InferenceEngine {
    llm: Arc<dyn LlmBackend>,
    runtime: Arc<Mutex<ClojureRuntime>>,
    index: Arc<CodeIndex>,
    semantic: Option<Arc<Mutex<SemanticIndex>>>,
    /// Named repositories available for `edit_function`. Key is the repo name
    /// the LLM uses in tool calls; value is the working-directory path.
    repos: HashMap<String, PathBuf>,
    pub default_model: String,
    pub system_prompt: String,
}

impl InferenceEngine {
    pub fn new(llm: Arc<dyn LlmBackend>, runtime: ClojureRuntime, index: CodeIndex) -> Self {
        Self {
            llm,
            runtime: Arc::new(Mutex::new(runtime)),
            index: Arc::new(index),
            semantic: None,
            repos: HashMap::new(),
            default_model: "claude-opus-4-7".into(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
        }
    }

    /// Attach a semantic index. After this, `search_code` merges full-text and
    /// semantic results; without it the tool falls back to full-text only.
    pub fn with_semantic(mut self, index: SemanticIndex) -> Self {
        self.semantic = Some(Arc::new(Mutex::new(index)));
        self
    }

    /// Register a named repository for use by `edit_function`.
    /// `name` is the identifier the LLM passes in the `repo` field.
    pub fn with_repo(mut self, name: impl Into<String>, workdir: impl Into<PathBuf>) -> Self {
        self.repos.insert(name.into(), workdir.into());
        self
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

            if !content.trim_start().starts_with('{') {
                return Ok(content);
            }

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
            results.push(self.dispatch_single(call).await);
        }
        results
    }

    async fn dispatch_single(&self, call: &ToolCall) -> ToolResult {
        match call.name.as_str() {
            "search_code" => {
                let query = call.input["query"].as_str().unwrap_or("").to_owned();
                let limit = call.input["limit"].as_u64().unwrap_or(5) as usize;

                let ft = match self
                    .index
                    .search(&SearchQuery::new(&query).with_limit(limit * 2))
                {
                    Ok(r) => r,
                    Err(e) => return ToolResult::err(&call.id, e.to_string()),
                };

                let sem = match &self.semantic {
                    None => None,
                    Some(idx) => match idx.lock().await.search(&query, limit * 2) {
                        Ok(r) => Some(r),
                        Err(e) => {
                            warn!("semantic search failed: {e}");
                            None
                        }
                    },
                };

                match sem {
                    Some(sem) => ToolResult::ok(&call.id, merge_results(ft, sem, limit)),
                    None => ToolResult::ok(&call.id, ft),
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
                let repo_name = call.input["repo"].as_str().unwrap_or("").to_owned();
                let file_path = call.input["file"].as_str().unwrap_or("").to_owned();

                let edit: Edit = match serde_json::from_value(call.input["edit"].clone()) {
                    Ok(e) => e,
                    Err(e) => return ToolResult::err(&call.id, format!("invalid edit: {e}")),
                };

                let workdir = match self.repos.get(&repo_name) {
                    Some(p) => p.clone(),
                    None => {
                        return ToolResult::err(
                            &call.id,
                            format!("unknown repo '{repo_name}'; register it with with_repo()"),
                        )
                    }
                };

                let repo = match CodeRepository::open(&workdir) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::err(&call.id, e.to_string()),
                };

                let abs = workdir.join(&file_path);
                let source = match std::fs::read_to_string(&abs) {
                    Ok(s) => s,
                    Err(e) => {
                        return ToolResult::err(&call.id, format!("cannot read {file_path}: {e}"))
                    }
                };

                let message = format!("{}: {}", edit_description(&edit), file_path);
                let edits = [edit];
                let result = match Editor::new(source).apply(&edits) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::err(&call.id, e.to_string()),
                };

                match repo.write_and_commit(&file_path, result.source.as_bytes(), &message, None) {
                    Ok(oid) => ToolResult::ok(
                        &call.id,
                        serde_json::json!({ "commit": oid.to_string(), "file": file_path }),
                    ),
                    Err(e) => ToolResult::err(&call.id, e.to_string()),
                }
            }

            other => ToolResult::err(&call.id, format!("unknown tool: {other}")),
        }
    }
}

// ── Result merging ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct MergedResult {
    score: f32,
    /// Where the hit came from: "fulltext", "semantic", or "both".
    source: &'static str,
    function: IndexedFunction,
}

/// Merge full-text and semantic results into a single ranked list.
///
/// Both score sets are normalised to [0, 1] before combining so BM25 scores
/// (unbounded) and cosine similarities (already in [0, 1]) are comparable.
/// Results appearing in both indexes get `(ft_norm + sem_norm) / 2`; results
/// appearing in only one get that score directly.
fn merge_results(ft: Vec<FtResult>, sem: Vec<SemanticResult>, limit: usize) -> Vec<MergedResult> {
    let ft_max = ft.iter().map(|r| r.score).fold(0f32, f32::max);
    let sem_max = sem.iter().map(|r| r.score).fold(0f32, f32::max);

    let mut map: HashMap<String, (f32, f32, IndexedFunction)> = HashMap::new();

    for r in ft {
        let key = result_key(&r.function);
        let score = if ft_max > 0.0 { r.score / ft_max } else { 0.0 };
        map.insert(key, (score, 0.0, r.function));
    }

    for r in sem {
        let key = result_key(&r.function);
        let score = if sem_max > 0.0 {
            r.score / sem_max
        } else {
            0.0
        };
        map.entry(key)
            .and_modify(|(_, s, _)| *s = score)
            .or_insert((0.0, score, r.function));
    }

    let mut results: Vec<MergedResult> = map
        .into_values()
        .map(|(ft, sem, function)| {
            let (score, source) = match (ft > 0.0, sem > 0.0) {
                (true, true) => ((ft + sem) / 2.0, "both"),
                (true, false) => (ft, "fulltext"),
                (false, true) => (sem, "semantic"),
                _ => (0.0, "none"),
            };
            MergedResult {
                score,
                source,
                function,
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    results
}

fn result_key(f: &IndexedFunction) -> String {
    format!("{}:{}", f.repo, f.name)
}

// ── Constants ─────────────────────────────────────────────────────────────────

const DEFAULT_SYSTEM_PROMPT: &str = "\
You are Mnemosyne, an AI programming assistant with access to tools for \
searching code repositories, evaluating Clojure expressions, and editing \
Clojure functions. Use these tools to answer questions and complete tasks. \
Prefer precise, minimal edits over large rewrites.";
