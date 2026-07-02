//! `annotate_function` — attach a description and use cases to a saved
//! function. Annotations are EDN sidecars committed to the internal repo and
//! merged into the search indexes.

use std::sync::Arc;

use mnemosyne_mcp_core::{CallToolResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::annotations::{annotation_rel_path, Annotation};
use crate::clj;
#[cfg(feature = "semantic")]
use crate::indexer;
use crate::state::McpState;
use crate::tools::parse_args;

pub struct AnnotateTool {
    state: Arc<McpState>,
}

impl AnnotateTool {
    pub fn new(state: Arc<McpState>) -> Self {
        Self { state }
    }
}

#[derive(Deserialize)]
struct AnnotateArgs {
    function: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    use_cases: Vec<String>,
}

#[async_trait::async_trait]
impl McpTool for AnnotateTool {
    fn name(&self) -> &str {
        "annotate_function"
    }

    fn description(&self) -> &str {
        "Attach or update a natural-language description and use cases for a function \
         already saved in the code store. Annotations are stored as EDN files in git \
         alongside the code and are folded into both search indexes, so a well-annotated \
         function is far easier to rediscover via function_lookup. A new description \
         replaces the old one; use cases accumulate."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "function": {
                    "type": "string",
                    "description": "Qualified name of a saved function, e.g. `mnemosyne.util/frob`"
                },
                "description": {
                    "type": "string",
                    "description": "What the function does (replaces any existing description)"
                },
                "use_cases": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Concrete situations where this function applies (appended, deduplicated)"
                }
            },
            "required": ["function"]
        })
    }

    async fn call(&self, arguments: Value) -> CallToolResult {
        let args: AnnotateArgs = match parse_args(arguments) {
            Ok(a) => a,
            Err(e) => return e,
        };

        let Some((ns, name)) = args.function.trim().split_once('/') else {
            return CallToolResult::error("function must be a qualified name: namespace/name");
        };
        if ns.is_empty() || name.is_empty() || name.contains('/') {
            return CallToolResult::error("function must be a qualified name: namespace/name");
        }
        if args.description.is_none() && args.use_cases.is_empty() {
            return CallToolResult::error(
                "nothing to annotate: provide a description and/or use_cases",
            );
        }

        let src_path = format!("src/{}", clj::namespace_to_path(ns));
        let ann_path = annotation_rel_path(ns, name);

        let (commit, annotation, parsed_docstring) = {
            let repo = self.state.repo.lock().expect("repo lock poisoned");

            // Only annotate functions that actually exist in the store.
            let source =
                std::fs::read_to_string(self.state.code_dir.join(&src_path)).unwrap_or_default();
            let Some(def) = clj::top_level_defs(&source)
                .into_iter()
                .find(|d| d.name == name)
            else {
                return CallToolResult::error(format!(
                    "unknown function {ns}/{name} — save it with save_function first"
                ));
            };

            let mut ann = std::fs::read_to_string(self.state.code_dir.join(&ann_path))
                .ok()
                .and_then(|s| Annotation::from_edn(&s).ok())
                .unwrap_or_default();
            ann.merge(args.description.clone(), args.use_cases.clone());

            let oid = match repo.write_and_commit(
                &ann_path,
                ann.to_edn(),
                &format!("Annotate {ns}/{name}"),
                None,
            ) {
                Ok(oid) => oid.to_string(),
                Err(e) => return CallToolResult::error(format!("commit failed: {e}")),
            };
            (oid, ann, def.docstring)
        };

        if let Err(e) = self.state.refresh_fulltext() {
            return CallToolResult::error(format!(
                "annotation committed ({commit}) but re-indexing failed: {e}"
            ));
        }

        #[cfg(feature = "semantic")]
        {
            let body = {
                let source = std::fs::read_to_string(self.state.code_dir.join(&src_path))
                    .unwrap_or_default();
                clj::top_level_defs(&source)
                    .into_iter()
                    .find(|d| d.name == name)
                    .map(|d| d.source)
            };
            if let Some(body) = body {
                self.state
                    .semantic_add(mnemosyne_code_search::IndexedFunction {
                        repo: indexer::INTERNAL_REPO_NAME.to_owned(),
                        file_path: src_path.clone(),
                        name: format!("{ns}/{name}"),
                        docstring: indexer::merged_docstring(
                            parsed_docstring.as_deref(),
                            Some(&annotation),
                        ),
                        body,
                    })
                    .await;
            }
        }

        #[cfg(not(feature = "semantic"))]
        let _ = parsed_docstring;

        CallToolResult::text(format!(
            "Annotated {ns}/{name} (commit {commit})\n\n{}\nstored at: {ann_path}",
            annotation.to_edn()
        ))
    }
}
