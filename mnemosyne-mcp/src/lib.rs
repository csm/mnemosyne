//! MCP surface for Mnemosyne.
//!
//! This crate wires Mnemosyne's existing capability crates into four MCP
//! tools served over the protocol layer in `mnemosyne-mcp-core`:
//!
//! | Tool | Backed by |
//! |---|---|
//! | `clojure_eval` | `mnemosyne-execution-engine` (persistent runtime thread) |
//! | `function_lookup` | `mnemosyne-semantic-search` + `mnemosyne-code-search` + `mnemosyne-symbol-registry` |
//! | `save_function` | `mnemosyne-code-storage` (internal git repository) |
//! | `annotate_function` | EDN sidecars in the same repository |
//!
//! The self-contained server binary lives in `mnemosyne-mcp-server`; this
//! crate stays a library so the tools can also be embedded in other hosts
//! (e.g. mounted alongside the HTTP interface adapter).

pub mod annotations;
pub mod clj;
pub mod indexer;
pub mod state;
pub mod tools;

use std::sync::Arc;

use mnemosyne_mcp_core::McpServer;

pub use mnemosyne_execution_engine::IoPolicy;
pub use state::{McpConfig, McpState};

pub const SERVER_NAME: &str = "mnemosyne";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTRUCTIONS: &str = "\
Mnemosyne is a Clojure-native agent substrate: a persistent Clojure runtime plus a \
git-backed store of reusable functions.

Recommended workflow:
1. `function_lookup` before writing anything — the store may already have what you need. \
Use a natural-language query for intent search, or `namespace/name` for exact source.
2. `clojure_eval` to develop and test candidate code; definitions persist across calls \
within this session.
3. `save_function` to promote working definitions into the git-backed store. Each save \
returns a `namespace/name@commit` ref that pins the exact revision.
4. `annotate_function` to record what a saved function is for — annotations make future \
lookups dramatically better.";

/// Initialise state under `config` and assemble the MCP server with all four
/// Mnemosyne tools registered.
pub async fn build_server(config: McpConfig) -> anyhow::Result<McpServer> {
    let state = McpState::init(config).await?;
    Ok(McpServer::new(SERVER_NAME, SERVER_VERSION)
        .instructions(INSTRUCTIONS)
        .tool(Arc::new(tools::EvalTool::new(state.clone())))
        .tool(Arc::new(tools::LookupTool::new(state.clone())))
        .tool(Arc::new(tools::SaveFunctionTool::new(state.clone())))
        .tool(Arc::new(tools::AnnotateTool::new(state))))
}
