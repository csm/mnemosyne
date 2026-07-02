//! `function_lookup` — semantic / full-text search and exact symbol lookup
//! over the code store, returning Clojure source.

use std::sync::Arc;

use mnemosyne_code_search::{IndexedFunction, SearchQuery};
use mnemosyne_mcp_core::{CallToolResult, McpTool};
use mnemosyne_symbol_registry::VersionedRef;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::annotations::{annotation_rel_path, Annotation};
use crate::clj;
use crate::state::McpState;
use crate::tools::parse_args;

/// Cap on the source text included per search hit, to keep tool output sane.
const MAX_BODY_CHARS: usize = 4000;

pub struct LookupTool {
    state: Arc<McpState>,
}

impl LookupTool {
    pub fn new(state: Arc<McpState>) -> Self {
        Self { state }
    }
}

#[derive(Deserialize)]
struct LookupArgs {
    query: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait::async_trait]
impl McpTool for LookupTool {
    fn name(&self) -> &str {
        "function_lookup"
    }

    fn description(&self) -> &str {
        "Find Clojure functions in the code store and return their source. Two ways in: \
         (1) exact symbol lookup — query is `namespace/name` or `namespace/name@commit` \
         (also a bare `namespace` for the whole file); returns the pinned source with its \
         versioned ref, trust status, and any annotations. (2) semantic search — query is \
         a natural-language description of what you need (falls back to full-text search \
         when the embedding model is unavailable). Mode is auto-detected from the query \
         shape; pass `mode` to force it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "`ns/name[@commit]` for exact lookup, or a description of the desired functionality for search"
                },
                "mode": {
                    "type": "string",
                    "enum": ["auto", "exact", "semantic", "fulltext"],
                    "description": "Force a lookup mode (default: auto)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum search results (default 5; ignored for exact lookup)"
                }
            },
            "required": ["query"]
        })
    }

    async fn call(&self, arguments: Value) -> CallToolResult {
        let args: LookupArgs = match parse_args(arguments) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let query = args.query.trim().to_owned();
        if query.is_empty() {
            return CallToolResult::error("query must not be empty");
        }
        let limit = args.limit.unwrap_or(5).clamp(1, 25);

        let mode = match args.mode.as_deref() {
            None | Some("auto") => {
                let symbol_shaped = !query.contains(char::is_whitespace)
                    && (query.contains('/') || query.contains('@'));
                if symbol_shaped {
                    "exact"
                } else {
                    "semantic"
                }
            }
            Some(m @ ("exact" | "semantic" | "fulltext")) => m,
            Some(other) => {
                return CallToolResult::error(format!(
                    "unknown mode '{other}' (expected auto, exact, semantic, or fulltext)"
                ))
            }
        };

        match mode {
            "exact" => self.exact(&query),
            "semantic" => self.semantic(&query, limit).await,
            _ => self.fulltext(&query, limit, None),
        }
    }
}

impl LookupTool {
    /// Resolve `ns[/name][@commit]` through the symbol registry.
    fn exact(&self, query: &str) -> CallToolResult {
        // Split the optional commit off the end.
        let (sym_part, commit) = match query.rsplit_once('@') {
            Some((s, c)) if !c.is_empty() => (s, Some(c.to_owned())),
            Some(_) => return CallToolResult::error("empty commit after '@'"),
            None => (query, None),
        };
        let (namespace, symbol) = match sym_part.split_once('/') {
            Some((ns, name)) if !ns.is_empty() && !name.is_empty() => {
                (ns.to_owned(), Some(name.to_owned()))
            }
            Some(_) => return CallToolResult::error("malformed symbol (expected ns/name)"),
            None => (sym_part.to_owned(), None),
        };

        // Pin to a concrete SHA so registry caching is sound (HEAD moves).
        let commit = {
            let repo = self.state.repo.lock().expect("repo lock poisoned");
            let rev = commit.as_deref().unwrap_or("HEAD");
            match repo.resolve_commit_hash(rev) {
                Ok(sha) => sha,
                Err(_) if commit.is_none() => {
                    return CallToolResult::error("the code store is empty — save a function first")
                }
                Err(e) => return CallToolResult::error(format!("could not resolve '{rev}': {e}")),
            }
        };

        // Resolve the whole namespace so trust/signature checks apply, then
        // extract the requested definition ourselves (this also handles
        // defn-, def, and defmacro, which the registry's extractor does not).
        let vref = VersionedRef {
            repo_url: None,
            namespace: namespace.clone(),
            symbol: None,
            commit: commit.clone(),
        };
        let resolved = {
            let mut registry = self.state.registry.lock().expect("registry lock poisoned");
            match registry.resolve_ref(&vref) {
                Ok(r) => r,
                Err(e) => return CallToolResult::error(format!("lookup failed: {e}")),
            }
        };

        let (source, shown_ref) = match &symbol {
            None => (resolved.source.clone(), format!("{namespace}@{commit}")),
            Some(name) => {
                let Some(def) = clj::top_level_defs(&resolved.source)
                    .into_iter()
                    .find(|d| d.name == *name)
                else {
                    return CallToolResult::error(format!(
                        "no definition named '{name}' in {namespace}@{commit}"
                    ));
                };
                (def.source, format!("{namespace}/{name}@{commit}"))
            }
        };

        let mut header = vec![
            format!(";; {shown_ref}"),
            format!(
                ";; trust: {:?}  signature: {}",
                resolved.trust,
                match &resolved.signature_fingerprint {
                    Some(fp) if resolved.signature_valid => format!("{fp} (valid)"),
                    Some(fp) => format!("{fp} (INVALID)"),
                    None => "none".to_owned(),
                }
            ),
        ];

        if let Some(name) = &symbol {
            if let Some(ann) = self.read_annotation(&namespace, name) {
                if let Some(d) = &ann.description {
                    header.push(format!(";; description: {}", d.replace('\n', "\n;;   ")));
                }
                for uc in &ann.use_cases {
                    header.push(format!(";; use case: {uc}"));
                }
            }
        }

        CallToolResult::text(format!("{}\n{}", header.join("\n"), source))
    }

    /// Current annotation for `ns/name`, read from the repo working tree.
    fn read_annotation(&self, ns: &str, name: &str) -> Option<Annotation> {
        let path = self.state.code_dir.join(annotation_rel_path(ns, name));
        let content = std::fs::read_to_string(path).ok()?;
        Annotation::from_edn(&content).ok()
    }

    #[cfg(feature = "semantic")]
    async fn semantic(&self, query: &str, limit: usize) -> CallToolResult {
        // Over-fetch so post-dedup still fills the requested limit.
        match self.state.semantic_search(query, limit * 2).await {
            Ok(results) => {
                let mut seen = std::collections::HashSet::new();
                let hits: Vec<(f32, IndexedFunction)> = results
                    .into_iter()
                    .filter(|r| {
                        seen.insert((r.function.file_path.clone(), r.function.name.clone()))
                    })
                    .take(limit)
                    .map(|r| (r.score, r.function))
                    .collect();
                render_hits(query, "semantic", &hits, None)
            }
            Err(reason) => self.fulltext(
                query,
                limit,
                Some(format!(
                    "semantic search unavailable ({reason}); showing full-text results"
                )),
            ),
        }
    }

    #[cfg(not(feature = "semantic"))]
    async fn semantic(&self, query: &str, limit: usize) -> CallToolResult {
        self.fulltext(
            query,
            limit,
            Some("semantic search not compiled in; showing full-text results".to_owned()),
        )
    }

    fn fulltext(&self, query: &str, limit: usize, note: Option<String>) -> CallToolResult {
        let index = self.state.index.lock().expect("index lock poisoned");
        match index.search(&SearchQuery::new(query).with_limit(limit)) {
            Ok(results) => {
                let hits: Vec<(f32, IndexedFunction)> =
                    results.into_iter().map(|r| (r.score, r.function)).collect();
                render_hits(query, "full-text", &hits, note)
            }
            Err(e) => CallToolResult::error(format!("search failed: {e}")),
        }
    }
}

fn render_hits(
    query: &str,
    kind: &str,
    hits: &[(f32, IndexedFunction)],
    note: Option<String>,
) -> CallToolResult {
    let mut out = String::new();
    if let Some(n) = note {
        out.push_str(&format!(";; note: {n}\n\n"));
    }
    if hits.is_empty() {
        out.push_str(&format!("No functions matched {kind} query: {query}"));
        return CallToolResult::text(out);
    }

    out.push_str(&format!("{} {kind} result(s) for: {query}\n", hits.len()));
    for (i, (score, f)) in hits.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}  (score {score:.3}, {})\n",
            i + 1,
            f.name,
            f.file_path
        ));
        if let Some(doc) = &f.docstring {
            for line in doc.lines() {
                out.push_str(&format!(";; {line}\n"));
            }
        }
        if f.body.len() > MAX_BODY_CHARS {
            let cut = f
                .body
                .char_indices()
                .take_while(|(idx, _)| *idx < MAX_BODY_CHARS)
                .last()
                .map(|(idx, ch)| idx + ch.len_utf8())
                .unwrap_or(0);
            out.push_str(&f.body[..cut]);
            out.push_str("\n;; … truncated; use exact lookup for the full source\n");
        } else {
            out.push_str(&f.body);
            out.push('\n');
        }
    }
    CallToolResult::text(out)
}
