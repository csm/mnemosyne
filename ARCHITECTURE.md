# Mnemosyne Architecture

## Mental Model

Mnemosyne is a **Clojure-native agent**: it perceives the world through Clojure
expressions, stores memory as Clojure/EDN, reasons by searching its function
space, and acts by composing or generating new functions. The LLM is the
planner; Clojure is the execution substrate.

## Layer Stack

```
┌─────────────────────────────────────────────────────┐
│              Interface Adapters                      │
│   Chat (HTTP/WS) │ CLI │ Slack │ custom protocol     │
└────────────────────────┬────────────────────────────┘
                         │  Intent (natural language / structured)
┌────────────────────────▼────────────────────────────┐
│              Agent Loop  (inference-engine)          │
│   plan → dispatch → observe → reflect → commit       │
└────┬───────────────────┬────────────────────────────┘
     │                   │
┌────▼────────┐   ┌───────▼──────────────────────────┐
│  Semantic   │   │         Execution Engine          │
│  Search     │   │   eval sandbox │ spec checking    │
│  (intent →  │   │   result capture │ side-effect    │
│   fns)      │   │   isolation                       │
└────┬────────┘   └───────┬──────────────────────────┘
     │                    │
┌────▼────────────────────▼──────────────────────────┐
│                  Code Store                         │
│  git repos │ function registry │ EDN memory blobs   │
│  built-in lib │ self-generated │ contributed        │
└────────────────────────┬───────────────────────────┘
                         │
┌────────────────────────▼───────────────────────────┐
│           Experimentation Layer                     │
│  candidate gen │ spec/test eval │ proof hooks       │
└────────────────────────────────────────────────────┘
```

## Crates

| Crate | Purpose |
|---|---|
| `mnemosyne-code-storage` | Git-backed read/write access to function repositories |
| `mnemosyne-code-search` | Full-text indexing and search via Tantivy |
| `mnemosyne-semantic-search` | Embedding model + HNSW vector index for intent-to-function retrieval |
| `mnemosyne-code-editor` | Structural (zipper-based) Clojure AST editing |
| `mnemosyne-execution-engine` | Clojure interpreter sandbox via Clojurust |
| `mnemosyne-core-functions` | Built-in standard library and template building blocks |
| `mnemosyne-memory` | EDN episodic log, working-memory snapshot, session state |
| `mnemosyne-experiment` | Candidate generation pipeline: propose → validate → score → promote |
| `mnemosyne-inference-engine` | Agent loop, LLM orchestration, tool dispatch |
| `mnemosyne-interface` | Pluggable adapter trait + HTTP/SSE and CLI implementations |
| `mnemosyne-mcp-core` | Self-contained MCP protocol layer: JSON-RPC framing, lifecycle, tool trait, stdio transport |
| `mnemosyne-mcp` | MCP tool implementations: Clojure eval, function lookup, save, annotate |
| `mnemosyne-mcp-server` | Self-contained MCP server binary (stdio) |

The first four crates in the table (`code-storage` through `core-functions`),
`inference-engine`, and the three MCP crates have initial implementations. The
remaining crates are planned.

## Key Design Decisions

### Function as the Unit of Identity

Every named `defn` in the system has a stable identity:

```
{namespace}/{name}@{content-hash}
```

The registry maps this to a git object ID. When the agent modifies a function
structurally it creates a new version; the old one is reachable via git history.
This gives undo and diff semantics for free without a separate versioning layer.

### Dual Search: Full-Text + Semantic

Tantivy handles exact and syntactic lookup (find functions named `retry`).
Semantic search handles intent (find something that retries on transient errors).
These compose: semantic search narrows candidates, Tantivy re-ranks within them.

The embedding unit is `(fn-name + docstring + body-summary)` as a single string.
Vectors are stored locally via an HNSW index (no external service dependency) and
rebuilt incrementally as functions are promoted or modified.

### Memory Tiers

- **Working memory** — active REPL bindings in the running `ClojureRuntime`
- **Episodic memory** — append-only EDN log per session: what was asked, what
  ran, what was returned
- **Semantic memory** — named functions in the code store; the agent's
  accumulated capability literally lives here
- **Retrieval** — semantic search over the code store, not a separate vector DB
  of opaque embeddings

### Structural Editing via Zipper

The editor operates on a zipper over the Clojure parse tree: parse → zip →
navigate to target form → replace subtree → unparse. This preserves whitespace
and comments, which matters when output is stored as human-readable source.
Tree-sitter drives navigation; the edit algebra follows the rewrite-clj model.

Available edit operations:

| Edit | Description |
|---|---|
| `ReplaceBody` | Swap the body of a `defn` |
| `PrependToBody` | Insert forms at the top of a `defn` body |
| `WrapBody` | Wrap the body in a new form (e.g. `try`) |
| `Rename` | Rename the function and update its registry entry |
| `InsertAfter` | Insert a new top-level form after this one |

### Experimentation Layer

Candidate functions move through a state machine before entering the code store:

```
propose → spec-check → sandbox-eval → score → promote | discard
```

- **propose** — LLM generates a candidate function body given the intent and
  similar functions retrieved from the store
- **spec-check** — run `clojure.spec` generative tests if a spec exists for
  this function's domain; `malli` schemas are an alternative
- **sandbox-eval** — execute in a resource-constrained `ClojureRuntime` with
  side effects isolated
- **score** — pass rate, output correctness, wall-clock performance
- **promote** — merge into the code store with a git commit and update the
  search index; failed experiments are logged to the episodic EDN log for
  future learning
- **proof gate** (optional) — for pure functions being promoted to the core
  library, translate to Lean 4 via subprocess and require a proof to pass
  before promotion

### MCP Surface

Mnemosyne's capabilities are exposed to external agents (Claude Code, other
MCP clients) through the Model Context Protocol. The MCP stack is split into
three crates so each layer stays independently reusable:

```
mnemosyne-mcp-server   (binary: stdio transport + configuration)
        │
mnemosyne-mcp          (tools: eval / lookup / save / annotate over the crates above)
        │
mnemosyne-mcp-core     (protocol: JSON-RPC 2.0, MCP lifecycle, McpTool trait — no Mnemosyne deps)
```

The four tools mirror the agent's own loop, letting an external LLM act as
the planner over the same substrate:

| Tool | Purpose |
|---|---|
| `clojure_eval` | Evaluate Clojure in a persistent runtime session (capabilities gated by `IoPolicy`) |
| `function_lookup` | Semantic (intent) search, full-text fallback, and exact `ns/name[@commit]` resolution returning pinned source |
| `save_function` | Commit a definition into the internal git repo; returns the `ns/name@commit` pin |
| `annotate_function` | Attach descriptions/use cases as EDN sidecars in git, folded into the search indexes |

See `mnemosyne-mcp/README.md` for tool schemas, storage layout, and the
forward plan (HTTP transport, resources, structured output).

### Pluggable Interface

The agent loop communicates through an adapter trait:

```rust
trait InterfaceAdapter {
    async fn recv(&mut self) -> Message;
    async fn send(&mut self, msg: AgentResponse);
}
```

The agent sees only `Message` and `AgentResponse`. HTTP/WebSocket, stdin/stdout,
Slack, and other paradigms are adapter implementations. This is symmetric with
the existing `LlmBackend` trait in the inference-engine, which abstracts the
outbound LLM call.

## Agent Loop

The inference-engine loop is:

```
recv(message)
  → retrieve(message)           # semantic search → candidate functions
  → plan(message, candidates)   # LLM plans using known functions as building blocks
  → for each step in plan:
      if fn exists: eval(fn, args)
      else:         experiment(intent)  # propose → validate → promote if good
  → commit_memory(result)        # update episodic log; promote successful functions
  → send(response)
```

The LLM never operates on an empty context: retrieved function signatures and
docstrings are always included in the planning prompt, grounding generation in
the actual capability of the code store.

## Roadmap

In priority order, based on what is currently blocked:

1. **Semantic search crate** — without it, function retrieval is purely
   syntactic; the "maps intent to functions" goal is entirely blocked
2. **Write path in code-storage** — the agent needs to save generated and
   modified functions back to git; currently read-only
3. **Structural editor completion** — tree-sitter zipper edits need to actually
   mutate source rather than returning it unchanged
4. **Memory crate** — EDN episodic log and session snapshot
5. **Interface adapter** — minimal HTTP/SSE adapter to wire up the chatbot
   interface
6. **Experimentation layer** — builds on all of the above; last because it
   requires a working eval sandbox, write path, and search index to be useful
