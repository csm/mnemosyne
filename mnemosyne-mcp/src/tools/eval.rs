//! `clojure_eval` — evaluate Clojure source in the persistent runtime.

use std::sync::Arc;

use mnemosyne_mcp_core::{CallToolResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::McpState;
use crate::tools::parse_args;

pub struct EvalTool {
    state: Arc<McpState>,
}

impl EvalTool {
    pub fn new(state: Arc<McpState>) -> Self {
        Self { state }
    }
}

#[derive(Deserialize)]
struct EvalArgs {
    code: String,
    #[serde(default)]
    namespace: Option<String>,
}

#[async_trait::async_trait]
impl McpTool for EvalTool {
    fn name(&self) -> &str {
        "clojure_eval"
    }

    fn description(&self) -> &str {
        "Evaluate Clojure source in Mnemosyne's persistent runtime and return the printed \
         value of the last form. Prefer this over host shell tools for exploring and \
         transforming files and data. Definitions (def/defn) persist across calls for the \
         lifetime of the server, so you can build up scratch state incrementally; use \
         save_function to persist anything worth keeping. Built-in namespaces \
         (mnemosyne.core, mnemosyne.templates, and — when file IO is granted — \
         mnemosyne.shell with cat/ls/find/grep pipelines) are preloaded; discover them \
         with `(ns-publics 'mnemosyne.core)` and `(doc mnemosyne.core/deep-merge)`, or \
         read full source via function_lookup. Host capabilities (file IO, network) are \
         governed by the server's IO policy and are denied by default."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Clojure source; all forms are evaluated, the last value is returned"
                },
                "namespace": {
                    "type": "string",
                    "description": "Namespace to evaluate in (created if absent). Defaults to the runtime's current namespace."
                }
            },
            "required": ["code"]
        })
    }

    async fn call(&self, arguments: Value) -> CallToolResult {
        let args: EvalArgs = match parse_args(arguments) {
            Ok(a) => a,
            Err(e) => return e,
        };

        if let Some(ns) = &args.namespace {
            if let Err(e) = self.state.runtime.set_namespace(ns).await {
                return CallToolResult::error(format!("could not switch namespace: {e}"));
            }
        }

        match self.state.runtime.eval(args.code).await {
            Ok(value) => CallToolResult::text(value.to_string()),
            Err(e) => CallToolResult::error(format!("evaluation error: {e}")),
        }
    }
}
