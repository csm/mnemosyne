//! Shared server state: the persistent Clojure runtime, the internal code
//! repository, the symbol registry, and the search indexes.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mnemosyne_code_search::CodeIndex;
use mnemosyne_code_storage::CodeRepository;
use mnemosyne_execution_engine::{ClojureRuntime, IoPolicy, RuntimeHandle};
use mnemosyne_symbol_registry::{SymbolRegistry, TrustPolicy};

use crate::indexer;

#[cfg(feature = "semantic")]
use mnemosyne_semantic_search::{EmbedModel, Embedder, SemanticIndex, SemanticResult};

/// Configuration for a Mnemosyne MCP server instance.
#[derive(Debug, Clone)]
pub struct McpConfig {
    /// Root directory for all persistent state. Layout:
    /// - `code/` — the internal git repository of saved functions
    /// - `fulltext-index/` — Tantivy index
    /// - `repo-cache/` — clones of external repositories
    pub data_dir: PathBuf,
    /// Host capabilities granted to the `clojure_eval` runtime.
    pub io_policy: IoPolicy,
    /// Boot the runtime with only the minimal bootstrap environment. Much
    /// faster cold start; mainly for tests.
    pub minimal_runtime: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./.mnemosyne"),
            io_policy: IoPolicy::deny_all(),
            minimal_runtime: false,
        }
    }
}

/// Lazily initialised semantic index. The embedding model is only loaded
/// (and, on first ever use, downloaded) when a semantic query arrives.
#[cfg(feature = "semantic")]
pub enum SemanticSlot {
    Unloaded,
    Ready(Box<SemanticIndex>),
    Failed(String),
}

/// Everything the tools share. All interior mutability is per-component so a
/// long eval cannot block a lookup.
pub struct McpState {
    pub runtime: RuntimeHandle,
    /// The internal git repository of saved functions.
    pub repo: Mutex<CodeRepository>,
    /// Working directory of `repo` (kept separately so annotation reads don't
    /// need the repo lock).
    pub code_dir: PathBuf,
    pub registry: Mutex<SymbolRegistry>,
    pub index: Mutex<CodeIndex>,
    #[cfg(feature = "semantic")]
    pub semantic: tokio::sync::Mutex<SemanticSlot>,
}

impl McpState {
    /// Open (or create) all persistent state under `config.data_dir`, spawn
    /// the Clojure runtime thread, and resynchronise the full-text index with
    /// the repository.
    pub async fn init(config: McpConfig) -> anyhow::Result<Arc<Self>> {
        let data_dir = &config.data_dir;
        std::fs::create_dir_all(data_dir)?;

        let code_dir = data_dir.join("code");
        let repo = CodeRepository::open_or_init(&code_dir)?;

        let mut registry =
            SymbolRegistry::new(data_dir.join("repo-cache"), TrustPolicy::permissive());
        registry.register_repo("default", &code_dir);

        let index = CodeIndex::open_or_create(data_dir.join("fulltext-index"))?;
        let fns = indexer::collect_functions(&repo);
        index.replace_all(&fns)?;
        tracing::info!(functions = fns.len(), "full-text index synchronised");

        let minimal = config.minimal_runtime;
        let runtime = RuntimeHandle::spawn_with_policy(
            move || {
                let mut rt = if minimal {
                    ClojureRuntime::minimal()
                } else {
                    ClojureRuntime::new()
                };
                if !minimal {
                    if let Err(e) = mnemosyne_core_functions::load_core(&mut rt) {
                        tracing::warn!("could not load core function library: {e}");
                    }
                }
                rt
            },
            config.io_policy.clone(),
        );

        // The shell namespace is defined over the async IO substrate, so it
        // only loads once the runtime thread has installed it.
        if config.io_policy.file_io && !minimal {
            if let Err(e) = runtime
                .eval(mnemosyne_core_functions::embedded::SHELL_CLJ)
                .await
            {
                tracing::warn!("could not load mnemosyne.shell: {e}");
            }
        }

        Ok(Arc::new(Self {
            runtime,
            repo: Mutex::new(repo),
            code_dir,
            registry: Mutex::new(registry),
            index: Mutex::new(index),
            #[cfg(feature = "semantic")]
            semantic: tokio::sync::Mutex::new(SemanticSlot::Unloaded),
        }))
    }

    /// Rebuild the full-text index from the repository at HEAD. Called after
    /// every write so search always reflects the committed state.
    pub fn refresh_fulltext(&self) -> anyhow::Result<usize> {
        let fns = {
            let repo = self.repo.lock().expect("repo lock poisoned");
            indexer::collect_functions(&repo)
        };
        let index = self.index.lock().expect("index lock poisoned");
        index.replace_all(&fns)?;
        Ok(fns.len())
    }

    /// Run a semantic (embedding) search, loading the model and building the
    /// index on first use. `Err` carries a human-readable reason the caller
    /// can surface before falling back to full-text search.
    #[cfg(feature = "semantic")]
    pub async fn semantic_search(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<SemanticResult>, String> {
        let mut slot = self.semantic.lock().await;

        if matches!(*slot, SemanticSlot::Unloaded) {
            tracing::info!("loading embedding model (first semantic query)");
            let fns = {
                let repo = self.repo.lock().expect("repo lock poisoned");
                indexer::collect_functions(&repo)
            };
            let built = tokio::task::spawn_blocking(move || -> Result<SemanticIndex, String> {
                let embedder = Embedder::new(EmbedModel::default())
                    .map_err(|e| format!("could not load embedding model: {e}"))?;
                let mut idx = SemanticIndex::new(embedder);
                idx.add_functions(&fns)
                    .map_err(|e| format!("could not embed functions: {e}"))?;
                Ok(idx)
            })
            .await
            .map_err(|e| format!("embedder task panicked: {e}"))?;

            *slot = match built {
                Ok(idx) => SemanticSlot::Ready(Box::new(idx)),
                Err(reason) => {
                    tracing::warn!("semantic index unavailable: {reason}");
                    SemanticSlot::Failed(reason)
                }
            };
        }

        match &*slot {
            SemanticSlot::Ready(idx) => {
                if idx.is_empty() {
                    return Ok(Vec::new());
                }
                idx.search(query, top_k).map_err(|e| e.to_string())
            }
            SemanticSlot::Failed(reason) => Err(reason.clone()),
            SemanticSlot::Unloaded => unreachable!("slot was just initialised"),
        }
    }

    /// Add (or re-add) a function to the semantic index if it is loaded.
    /// Duplicate entries are tolerated; lookups deduplicate by function.
    #[cfg(feature = "semantic")]
    pub async fn semantic_add(&self, f: mnemosyne_code_search::IndexedFunction) {
        let mut slot = self.semantic.lock().await;
        if let SemanticSlot::Ready(idx) = &mut *slot {
            if let Err(e) = idx.add_functions(&[f]) {
                tracing::warn!("could not add function to semantic index: {e}");
            }
        }
    }
}
