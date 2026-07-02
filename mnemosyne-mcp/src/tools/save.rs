//! `save_function` — persist a Clojure definition into the internal git repo.

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

pub struct SaveFunctionTool {
    state: Arc<McpState>,
}

impl SaveFunctionTool {
    pub fn new(state: Arc<McpState>) -> Self {
        Self { state }
    }
}

#[derive(Deserialize)]
struct SaveArgs {
    namespace: String,
    name: String,
    source: String,
    #[serde(default)]
    docstring: Option<String>,
    #[serde(default)]
    commit_message: Option<String>,
}

#[async_trait::async_trait]
impl McpTool for SaveFunctionTool {
    fn name(&self) -> &str {
        "save_function"
    }

    fn description(&self) -> &str {
        "Save a Clojure definition (defn, defn-, defmacro, or def) into the internal git \
         repository and index it for lookup. The namespace file is created on first save; \
         an existing definition with the same name is replaced in place. Every save is a \
         git commit, so history and rollback come for free. Returns the versioned ref \
         (`namespace/name@commit`) that pins this exact revision. Use this to promote \
         validated scratch work from clojure_eval into the durable code store."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "namespace": {
                    "type": "string",
                    "description": "Clojure namespace to save into, e.g. `mnemosyne.util` (file src/mnemosyne/util.clj)"
                },
                "name": {
                    "type": "string",
                    "description": "Name of the definition; must match the name in `source`"
                },
                "source": {
                    "type": "string",
                    "description": "Complete top-level form, e.g. `(defn frob \"doc\" [x] …)`"
                },
                "docstring": {
                    "type": "string",
                    "description": "Description stored as an annotation when the source has no docstring"
                },
                "commit_message": {
                    "type": "string",
                    "description": "Git commit message (default: `Save <namespace>/<name>`)"
                }
            },
            "required": ["namespace", "name", "source"]
        })
    }

    async fn call(&self, arguments: Value) -> CallToolResult {
        let args: SaveArgs = match parse_args(arguments) {
            Ok(a) => a,
            Err(e) => return e,
        };

        let ns = args.namespace.trim();
        let name = args.name.trim();
        if ns.is_empty() || ns.contains(char::is_whitespace) || ns.contains('/') {
            return CallToolResult::error("invalid namespace: must be a dotted Clojure ns name");
        }
        if name.is_empty() || name.contains(char::is_whitespace) || name.contains('/') {
            return CallToolResult::error("invalid name: must be a bare symbol");
        }

        // The source must actually define `name` at the top level.
        let defs = clj::top_level_defs(&args.source);
        let Some(def) = defs.iter().find(|d| d.name == name) else {
            let found: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
            return CallToolResult::error(format!(
                "source does not contain a top-level definition named '{name}'{}",
                if found.is_empty() {
                    " (no definitions found — is the form balanced?)".to_owned()
                } else {
                    format!(" (found: {})", found.join(", "))
                }
            ));
        };
        let parsed_docstring = def.docstring.clone();

        let rel_path = format!("src/{}", clj::namespace_to_path(ns));
        let message = args
            .commit_message
            .clone()
            .unwrap_or_else(|| format!("Save {ns}/{name}"));

        // A docstring argument is persisted as an annotation so it survives
        // index rebuilds; it rides along in the same commit.
        let annotation = match (&args.docstring, &parsed_docstring) {
            (Some(d), None) => {
                let mut ann = self.read_annotation(ns, name).unwrap_or_default();
                ann.merge(Some(d.clone()), vec![]);
                Some(ann)
            }
            _ => None,
        };

        let commit = {
            let repo = self.state.repo.lock().expect("repo lock poisoned");
            let workdir = self.state.code_dir.join(&rel_path);
            let existing = std::fs::read_to_string(&workdir).unwrap_or_default();

            let content = if existing.is_empty() {
                format!("(ns {ns})\n\n{}\n", args.source.trim_end())
            } else {
                clj::upsert_def(&existing, name, &args.source)
            };

            if let Err(e) = repo.write_file(&rel_path, content) {
                return CallToolResult::error(format!("could not write {rel_path}: {e}"));
            }
            let mut paths = vec![rel_path.clone()];
            if let Some(ann) = &annotation {
                let ann_path = annotation_rel_path(ns, name);
                if let Err(e) = repo.write_file(&ann_path, ann.to_edn()) {
                    return CallToolResult::error(format!("could not write {ann_path}: {e}"));
                }
                paths.push(ann_path);
            }
            match repo.commit(&paths, &message, None) {
                Ok(oid) => oid.to_string(),
                Err(e) => return CallToolResult::error(format!("commit failed: {e}")),
            }
        };

        if let Err(e) = self.state.refresh_fulltext() {
            return CallToolResult::error(format!(
                "saved {ns}/{name}@{commit} but re-indexing failed: {e}"
            ));
        }

        #[cfg(feature = "semantic")]
        {
            let ann = self.read_annotation(ns, name);
            self.state
                .semantic_add(mnemosyne_code_search::IndexedFunction {
                    repo: indexer::INTERNAL_REPO_NAME.to_owned(),
                    file_path: rel_path.clone(),
                    name: format!("{ns}/{name}"),
                    docstring: indexer::merged_docstring(parsed_docstring.as_deref(), ann.as_ref()),
                    body: args.source.clone(),
                })
                .await;
        }

        CallToolResult::text(format!(
            "Saved {ns}/{name}@{commit}\nfile: {rel_path}\npinned ref: {ns}/{name}@{commit}"
        ))
    }
}

impl SaveFunctionTool {
    fn read_annotation(&self, ns: &str, name: &str) -> Option<Annotation> {
        let path = self.state.code_dir.join(annotation_rel_path(ns, name));
        let content = std::fs::read_to_string(path).ok()?;
        Annotation::from_edn(&content).ok()
    }
}
