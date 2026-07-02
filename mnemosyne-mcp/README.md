# mnemosyne-mcp

MCP surface for Mnemosyne: exposes the Clojure runtime, the function store,
and both search indexes to any MCP client as four tools.

## Crate layout

The MCP stack is deliberately three crates:

| Crate | Role | Depends on |
|---|---|---|
| `mnemosyne-mcp-core` | Protocol layer: JSON-RPC 2.0 framing, MCP lifecycle (`initialize`, `tools/list`, `tools/call`), the `McpTool` trait, stdio transport | serde/tokio only — **no Mnemosyne crates** |
| `mnemosyne-mcp` (this crate) | The tools, wired to the capability crates; a library so other hosts can embed them | mcp-core + execution-engine, code-storage, code-search, symbol-registry, semantic-search |
| `mnemosyne-mcp-server` | Self-contained binary: argument parsing, logging to stderr, stdio serve loop | mnemosyne-mcp |

Rationale: the protocol layer is generic and should be reusable by any future
Mnemosyne service (or extracted entirely); the tools are a library so they can
be mounted next to the HTTP interface adapter later; the binary stays a thin
shell so packaging/distribution concerns never leak into the tools.

## Running

```sh
cargo run -p mnemosyne-mcp-server -- --data-dir ~/.mnemosyne
```

Claude Code / MCP client configuration:

```json
{
  "mcpServers": {
    "mnemosyne": {
      "command": "mnemosyne-mcp-server",
      "args": ["--data-dir", "/path/to/state"]
    }
  }
}
```

Flags: `--allow-file-io`, `--allow-network`, `--allow-all` grant the eval
runtime host capabilities (everything is denied by default);
`--minimal-runtime` skips the full Clojure standard library for a fast boot.
Logging goes to stderr (`RUST_LOG`, default `info`); stdout carries the
protocol.

## Tools

### `clojure_eval`

Evaluate Clojure source in a **persistent** runtime session (a dedicated
interpreter thread that lives as long as the server). Definitions accumulate
across calls; the printed value of the last form is returned. Host IO is
governed by the server's `IoPolicy`.

```json
{ "code": "(defn twice [x] (* 2 x)) (twice 21)", "namespace": "scratch" }
```

### `function_lookup`

One tool, three paths, always returning Clojure source:

- **exact** — query is `namespace/name`, `namespace/name@commit`, or a bare
  `namespace`. Resolution goes through the symbol registry: the ref is pinned
  to a concrete commit SHA, the commit signature is checked, and the trust
  policy applied. Output includes the pinned ref, trust/signature status, and
  any annotations.
- **semantic** — query is a natural-language intent ("retry with backoff").
  Backed by the embedding index; the model is loaded lazily on the first
  semantic query.
- **fulltext** — Tantivy keyword search over names, docstrings (including
  annotation text), and bodies. Also the automatic fallback when the
  embedding model is unavailable or the `semantic` feature is compiled out.

Mode is auto-detected (symbol-shaped queries go exact, prose goes semantic)
and can be forced with `"mode"`.

### `save_function`

Persist a top-level definition (`defn`, `defn-`, `defmacro`, `def`) into the
internal git repository at `<data-dir>/code`:

- namespace `my.cool-ns` maps to `src/my/cool_ns.clj` (created with an `(ns …)`
  header on first save)
- an existing definition of the same name is **replaced in place**
  (string/comment-aware scanning, not regex), siblings untouched
- every save is a git commit; the returned `namespace/name@commit` ref is a
  stable pin that `function_lookup` and the symbol registry can resolve forever
- both search indexes are refreshed immediately

### `annotate_function`

Attach a description and use cases to a saved function. Annotations are EDN
sidecars committed to the same repository:

```
meta/my/cool_ns/frob.edn
{:description "Frobnicates the widget."
 :use-cases ["normalising legacy input"]}
```

Descriptions replace; use cases accumulate (deduplicated). Annotation text is
folded into the docstring field of both indexes, so annotating a function
directly improves its future discoverability.

## Storage layout

```
<data-dir>/
  code/             internal git repo: src/**.clj + meta/**.edn
  fulltext-index/   Tantivy index (rebuilt from the repo on boot and after writes)
  repo-cache/       clones of external repos referenced by versioned refs
```

The full-text index is treated as a disposable cache of the repository — it
is rebuilt from HEAD on startup and resynchronised after every save/annotate,
so the git repo remains the single source of truth.

## Features

- `semantic` (default): embedding-based intent search via
  `mnemosyne-semantic-search` (candle + a BERT-family model downloaded from
  HuggingFace on first use). Disable for a lightweight build; lookup then
  falls back to full-text search.

## Forward plan

- **HTTP/SSE transport** in `mnemosyne-mcp-core` next to stdio (streamable
  HTTP per the current MCP spec), so one server can serve remote clients.
- **MCP resources** exposing the episodic memory log (`mnemosyne-memory`) and
  repository files as readable resources.
- **`structuredContent` results** (2025-06-18 spec) alongside text, so
  clients get machine-readable search hits and eval values.
- **Structural edit tool** wrapping `mnemosyne-code-editor` (ReplaceBody,
  WrapBody, Rename, …) once the zipper editor mutates source.
- **Semantic index persistence/incremental updates** — today the embedding
  index is built per process and appended to on save; persisting vectors and
  deleting stale entries becomes worthwhile as the store grows.
- **Trust policy configuration** — the server currently runs a permissive
  policy for its own store; strict signature-based policies for external
  repos should be surfaced as server flags.
