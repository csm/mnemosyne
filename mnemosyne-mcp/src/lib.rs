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
pub mod seed;
pub mod state;
pub mod tools;

use std::sync::Arc;

use mnemosyne_mcp_core::McpServer;

pub use mnemosyne_execution_engine::IoPolicy;
pub use state::{McpConfig, McpState};

pub const SERVER_NAME: &str = "mnemosyne";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compose the `initialize` instructions for this server configuration.
///
/// This is the first (often only) orientation an agent gets, so it answers
/// the two questions a fresh session has: what is this system for (write or
/// reuse Clojure functions instead of reaching for host shell tools), and
/// what is already in it (the pre-seeded built-in library). Availability
/// notes track the actual `IoPolicy` / runtime configuration so the agent is
/// never promised a namespace this session cannot use.
fn instructions(config: &McpConfig) -> String {
    let builtins_loaded = !config.minimal_runtime;
    let shell_loaded = builtins_loaded && config.io_policy.file_io;

    let mut text = String::from(
        "\
Mnemosyne is a Clojure-native execution substrate: a persistent Clojure runtime paired \
with a git-backed store of reusable, searchable functions. Prefer it over one-off host \
shell commands — explore and modify files and data by evaluating Clojure, and promote \
anything that works into a named function so the next task (or the next session) reuses \
it instead of rewriting it. The store accumulates capability over time: every function \
you save or annotate makes future lookups better.

Workflow:
1. `function_lookup` BEFORE writing anything — the store may already have what you need. \
A natural-language query searches by intent; `namespace/name` returns exact pinned source.
2. `clojure_eval` to develop and test code. Definitions persist across calls for the \
lifetime of this session.
3. `save_function` to promote working definitions into the store. Each save is a git \
commit and returns a `namespace/name@commit` ref that pins that exact revision.
4. `annotate_function` to record what a saved function is for — annotations are folded \
into the search indexes, so they directly improve future discovery.

The store is pre-seeded with the built-in library",
    );
    text.push_str(if builtins_loaded {
        ", already loaded in the eval runtime:\n"
    } else {
        " (NOT preloaded in the eval runtime — this server runs a minimal runtime; \
evaluate what you need first):\n"
    });
    text.push_str(
        "\
- `mnemosyne.core` — string, collection, and IO helpers
- `mnemosyne.templates` — skeleton functions with slots, for specialising into new functions
- `mnemosyne.shell` — shell-style file exploration as async channels (`cat`, `ls`, \
`find`, `grep`, `head`, `sed`, …) that pipeline with `->>`",
    );
    text.push_str(if shell_loaded {
        "\n"
    } else {
        " — in the store but NOT loaded in this session because the server denies file IO \
(start it with `--allow-file-io` to enable)\n"
    });
    text.push_str(
        "\
To see what a built-in does, `function_lookup` its `namespace/name` for the full source \
and docstring. (Runtime introspection such as `doc` and `ns-publics` is not available \
yet — discover functions through `function_lookup`, not eval.)",
    );
    text
}

/// Initialise state under `config` and assemble the MCP server with all four
/// Mnemosyne tools registered.
pub async fn build_server(config: McpConfig) -> anyhow::Result<McpServer> {
    let instructions = instructions(&config);
    let state = McpState::init(config).await?;
    Ok(McpServer::new(SERVER_NAME, SERVER_VERSION)
        .instructions(instructions)
        .tool(Arc::new(tools::EvalTool::new(state.clone())))
        .tool(Arc::new(tools::LookupTool::new(state.clone())))
        .tool(Arc::new(tools::SaveFunctionTool::new(state.clone())))
        .tool(Arc::new(tools::AnnotateTool::new(state))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_track_io_policy_and_runtime() {
        // Full runtime, no file IO: builtins loaded, shell gated.
        let default = instructions(&McpConfig::default());
        assert!(default.contains("already loaded in the eval runtime"));
        assert!(default.contains("--allow-file-io"));

        // File IO granted: no gating note.
        let with_io = instructions(&McpConfig {
            io_policy: IoPolicy::allow_all(),
            ..McpConfig::default()
        });
        assert!(!with_io.contains("--allow-file-io"));
        assert!(with_io.contains("already loaded in the eval runtime"));

        // Minimal runtime: nothing preloaded.
        let minimal = instructions(&McpConfig {
            minimal_runtime: true,
            ..McpConfig::default()
        });
        assert!(minimal.contains("NOT preloaded"));

        // Every variant names the seeded namespaces and steers discovery
        // through function_lookup (runtime doc/ns-publics don't exist yet).
        for text in [&default, &with_io, &minimal] {
            for needle in [
                "mnemosyne.core",
                "mnemosyne.shell",
                "mnemosyne.templates",
                "function_lookup",
                "not available",
            ] {
                assert!(text.contains(needle), "missing {needle:?} in: {text}");
            }
        }
    }
}
