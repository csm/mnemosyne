use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Mutex;
use tracing::warn;

use mnemosyne_code_editor::{edit_description, Edit, Editor};
use mnemosyne_code_search::{CodeIndex, IndexedFunction, SearchQuery, SearchResult as FtResult};
use mnemosyne_code_storage::CodeRepository;
use mnemosyne_execution_engine::RuntimeHandle;
use mnemosyne_memory::{EpisodeKind, MemoryStore};
use mnemosyne_semantic_search::{SemanticIndex, SemanticResult};
use mnemosyne_symbol_registry::{SymbolRegistry, TrustPolicy};

use crate::{
    llm::{LlmBackend, LlmRequest, Message},
    tool::{builtin_tools, ToolCall, ToolResult},
    Result,
};

pub struct InferenceEngine {
    llm: Arc<dyn LlmBackend>,
    runtime: RuntimeHandle,
    index: Arc<CodeIndex>,
    semantic: Option<Arc<Mutex<SemanticIndex>>>,
    /// Named repositories available for `edit_function`. Key is the repo name
    /// the LLM uses in tool calls; value is the working-directory path.
    repos: HashMap<String, PathBuf>,
    memory: Option<Arc<Mutex<MemoryStore>>>,
    /// Symbol registry used by `load_versioned_symbol`. When `None` the tool
    /// returns an error explaining how to configure one.
    registry: Option<Arc<Mutex<SymbolRegistry>>>,
    pub default_model: String,
    pub system_prompt: String,
}

impl InferenceEngine {
    pub fn new(llm: Arc<dyn LlmBackend>, runtime: RuntimeHandle, index: CodeIndex) -> Self {
        Self {
            llm,
            runtime,
            index: Arc::new(index),
            semantic: None,
            repos: HashMap::new(),
            memory: None,
            registry: None,
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

    /// Attach an episodic memory store. When set, every user message, tool
    /// call, and assistant reply is appended to the store automatically.
    pub fn with_memory(mut self, store: MemoryStore) -> Self {
        self.memory = Some(Arc::new(Mutex::new(store)));
        self
    }

    /// Attach a symbol registry. Required for the `load_versioned_symbol` tool.
    ///
    /// Pass a registry pre-configured with the appropriate [`TrustPolicy`] and
    /// any local repo aliases. External repositories are cloned on demand into
    /// the registry's cache directory.
    pub fn with_registry(mut self, registry: SymbolRegistry) -> Self {
        self.registry = Some(Arc::new(Mutex::new(registry)));
        self
    }

    /// Convenience builder that creates a [`SymbolRegistry`] with `policy` and
    /// no pre-registered repos, using `cache_dir` for external repo clones.
    pub fn with_trust_policy(
        self,
        cache_dir: impl Into<std::path::PathBuf>,
        policy: TrustPolicy,
    ) -> Self {
        self.with_registry(SymbolRegistry::new(cache_dir, policy))
    }

    /// Run a single-turn prompt, dispatching any tool calls before returning
    /// the final assistant response.
    pub async fn run(&self, user_message: impl Into<String>) -> Result<String> {
        let user_message = user_message.into();

        if let Some(mem) = &self.memory {
            let _ = mem.lock().await.log(EpisodeKind::UserMessage {
                content: user_message.clone(),
            });
        }

        let mut messages = vec![
            Message::system(&self.system_prompt),
            Message::user(&user_message),
        ];

        loop {
            let req = LlmRequest {
                messages: messages.clone(),
                tools: builtin_tools(),
                model: self.default_model.clone(),
                max_tokens: 4096,
                temperature: 0.2,
            };

            let response = self.llm.complete(req).await?;

            // Record the assistant turn — any text plus the tool_use blocks it
            // emitted — so the follow-up tool_result blocks line up by id.
            messages.push(Message::assistant_turn(
                &response.text,
                &response.tool_calls,
            ));

            // No tool calls means the model is done: `text` is the answer.
            if response.tool_calls.is_empty() {
                if let Some(mem) = &self.memory {
                    let _ = mem.lock().await.log(EpisodeKind::AssistantReply {
                        content: response.text.clone(),
                    });
                }
                return Ok(response.text);
            }

            let results = self.dispatch_tools(&response.tool_calls).await;
            messages.push(Message::tool_results(&results));
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
        // Log the tool call before dispatching.
        if let Some(mem) = &self.memory {
            let _ = mem.lock().await.log(EpisodeKind::ToolCall {
                tool: call.name.clone(),
                input: call.input.clone(),
            });
        }

        let result = self.execute_tool(call).await;

        // Log the result.
        if let Some(mem) = &self.memory {
            let _ = mem.lock().await.log(EpisodeKind::ToolResult {
                tool: call.name.clone(),
                success: !result.is_error,
                output: result.output.to_string(),
            });
        }

        result
    }

    async fn execute_tool(&self, call: &ToolCall) -> ToolResult {
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
                match self.runtime.eval(source).await {
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

            "recall_memory" => {
                let limit = call.input["limit"].as_u64().unwrap_or(10) as usize;
                match &self.memory {
                    None => ToolResult::err(&call.id, "memory not configured"),
                    Some(mem) => {
                        let store = mem.lock().await;
                        let recent = store.recent(limit).to_vec();
                        match serde_json::to_value(&recent) {
                            Ok(v) => ToolResult::ok(&call.id, v),
                            Err(e) => ToolResult::err(&call.id, e.to_string()),
                        }
                    }
                }
            }

            "load_versioned_symbol" => {
                let vref_str = call.input["vref"].as_str().unwrap_or("").to_owned();
                match &self.registry {
                    None => ToolResult::err(
                        &call.id,
                        "no symbol registry configured; call with_registry() on InferenceEngine",
                    ),
                    Some(reg) => {
                        let resolved = {
                            let mut reg = reg.lock().await;
                            match reg.resolve(&vref_str) {
                                Ok(r) => r,
                                Err(e) => return ToolResult::err(&call.id, e.to_string()),
                            }
                        };
                        let trust = format!("{:?}", resolved.trust);
                        match self
                            .runtime
                            .load_versioned(resolved.source.clone(), vref_str.clone())
                            .await
                        {
                            Ok(()) => ToolResult::ok(
                                &call.id,
                                serde_json::json!({
                                    "vref": vref_str,
                                    "commit": resolved.commit_hash,
                                    "signed": resolved.signature_valid,
                                    "fingerprint": resolved.signature_fingerprint,
                                    "trust": trust,
                                }),
                            ),
                            Err(e) => ToolResult::err(&call.id, e.to_string()),
                        }
                    }
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
You are Mnemosyne, an agent that accomplishes high-level tasks in the user's \
computer environment by looking up or writing Clojure functions and running \
them in a live Clojurust runtime. Clojure is how you get things done; your job \
is to turn an intent into the right Clojure expression — reusing what already \
exists wherever you can.

## Tools vs. Clojure — keep them separate

There are exactly two kinds of action, and they are NOT the same thing:

- HARNESS TOOLS are the named tools provided to you through the tool interface \
  (search_code, eval_clojure, edit_function, recall_memory, \
  load_versioned_symbol). You invoke them as tool calls. They are NOT Clojure \
  functions: never write `(eval_clojure ...)` or `(search_code ...)` in \
  Clojure source, and never expect a tool name to resolve as a symbol.
- CLOJURE FUNCTIONS are the code you read, write, compose, and run. They live \
  in the runtime and the code store. You run them by passing Clojure source to \
  the `eval_clojure` tool — e.g. tool `eval_clojure` with source `(map inc \
  [1 2 3])`. Clojure's own built-ins (map, reduce, filter, …) are Clojure, not \
  tools.

In short: tools are the controls of this harness; Clojure is the material you \
work with through them.

## How to work a task

1. UNDERSTAND the intent and what a successful result looks like in the user's \
   environment.
2. SEARCH first with `search_code` — there is almost always existing code to \
   build on. Read what you find before writing anything.
3. REUSE, in this order of preference:
   a. Call an existing function as-is via `eval_clojure`.
   b. COMPOSE existing functions into a larger one (thread them, map/reduce \
      over them, wrap them) rather than reimplementing their logic.
   c. MUTATE an existing function into a new version with `edit_function` \
      (ReplaceBody, WrapBody, Rename, …). The returned commit hash is the new \
      version; the old one still exists. Specialise the template functions in \
      mnemosyne.templates (transform-coll, reduce-to-map, retry, pipeline) and \
      the helpers in mnemosyne.core when they fit.
   d. Only write a brand-new function when nothing above fits.
4. RUN it with `eval_clojure`, inspect the result, and iterate.
5. ANSWER the user once the task is actually done.

## Capturing facts

Express everything you learn in the same form as everything else — as code or \
data, never as prose buried in a reply:

- a zero-arg function that returns the fact, e.g. \
  `(defn fact:home-dir [] \"/home/user\")`, or
- a raw EDN structure, e.g. `{:os :linux :cores 8}`.

Persistence is tiered. Scratch work — exploratory defs and intermediate \
values — lives in the live runtime via `eval_clojure`. Once a fact or function \
is validated and worth keeping, PROMOTE it into the committed code store with \
`edit_function` so later sessions can `search_code` and reuse it.

## Acting on the environment

You reach the user's computer through Clojure functions backed by the async \
IO and networking substrate (clojure.core.async channels over cljrs-io / \
cljrs-net), always within the configured guardrails. Prefer the smallest \
capability that does the job, and expect IO/network calls to be asynchronous. \
You may NOT shell out or exec arbitrary programs — that path is disabled.

## Versioned symbols and trust

When you reference code by version, pin it to a commit with the versioned-ref \
syntax — `namespace/symbol@<commit>`, `namespace@<commit>`, or \
`https://host/u/r::ns/sym@<commit>` for an external repo — and load it with \
`load_versioned_symbol`. Pinning makes runs reproducible, and multiple \
versions of a function can coexist as distinct immutable values. External code \
(including code from other agents) runs only after its signature is verified \
and the trust policy is satisfied; `load_versioned_symbol` reports the \
fingerprint and trust level, so check them when the source is external.

Prefer precise, minimal edits over large rewrites.";
