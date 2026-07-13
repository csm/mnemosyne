# End-to-End Testing Plan for the Mnemosyne MCP Server

Plan for exercising `mnemosyne-mcp-server` in realistic agent scenarios: an
external LLM agent connects over MCP and uses `clojure_eval` /
`function_lookup` / `save_function` / `annotate_function` to solve real tasks
inside an isolated Docker environment, and an out-of-band grader decides
whether the task was actually solved.

This is distinct from unit/integration tests of the crates. The question here
is: *does the MCP surface make a real agent more capable on real work*, and
does the whole stack (stdio protocol, persistent runtime, seeded library,
save/lookup loop) hold up under an actual LLM driving it?

## Goals

1. **Protocol soundness under a real client** — initialize handshake,
   instructions text, tool schemas, long eval outputs, errors, all through a
   production MCP client rather than a scripted one.
2. **Task success** — can an agent with Mnemosyne solve representative
   coding / ops / data tasks end to end.
3. **Tool adoption** — does the agent actually use the lookup → eval → save →
   annotate loop, or does it ignore the server. Measured from transcripts.
4. **Memory value** — do functions saved in one session get found and reused
   in a later session (this is the core value proposition).
5. *(Future)* **Baseline deltas** — same tasks, same model, Mnemosyne on vs
   off; and a model matrix.

## Architecture

Three containers per task run, on an internal Docker network with no default
egress:

```
┌────────────────────────────────────────────────────────────┐
│ docker network: tasknet (internal)                         │
│                                                            │
│  ┌──────────────────┐        ┌───────────────┐             │
│  │  agent container  │──────▶│ litellm proxy │──▶ internet │
│  │                  │  http  │  (only egress) │   (LLM API)│
│  │  harness (CLI)   │        └───────────────┘             │
│  │    │ stdio                                              │
│  │  mnemosyne-mcp-server                                   │
│  │    │                                                    │
│  │  /task  (fixture: broken tree / sick services / logs)   │
│  │  /mnemosyne-data  (server --data-dir, a volume)         │
│  └──────────────────┘                                      │
└────────────────────────────────────────────────────────────┘

           grader container (created AFTER the run, no network,
           mounts the agent's /task workspace read-only + the
           ground truth from the host — never present in tasknet)
```

Key decisions:

- **Everything the agent touches lives in one container.** The harness, the
  MCP server, and the task fixture share a filesystem so `clojure_eval` with
  `--allow-file-io` (and `mnemosyne.shell`) can operate on the task directly.
  This matches how a user would actually run it.
- **The only network egress is the LiteLLM proxy.** The agent container sits
  on an `internal: true` network; the proxy container is dual-homed. The
  agent cannot browse for solutions, and the model endpoint is swappable
  without touching the agent image.
- **Grading happens outside, after the run.** The grader container is created
  only after the agent container is stopped, has no network, and is the only
  place ground truth / hidden tests ever exist. Nothing to find, nothing to
  exfiltrate to.
- **`/mnemosyne-data` is a named volume**, so the function store survives
  across sessions for the memory-carryover experiments (Phase 2) and can be
  inspected (it is a plain git repo) as part of grading tool adoption.

### Anti-cheating checklist

- Ground truth, hidden tests, and grader scripts are **never baked into the
  agent image** and never mounted during the agent's run.
- No Docker socket, no host mounts other than the task volume, non-root user
  in the agent container.
- Network egress limited to the proxy (`internal` network); the proxy
  allowlists only the model API host.
- For ops tasks, graders verify the fix **survives a restart** of the broken
  service (prevents "looks healthy right now" masking).
- Synthetic data (logs) is generated per-run from a seed; the ground-truth
  answers are computed by the generator on the host and compared at grade
  time, so answers can't be memorized or found in the image.
- Transcripts are recorded (`--output-format stream-json`), so a "pass" can
  be audited for how it was achieved.

## Harness

**Primary: Claude Code in headless mode.** It is a first-class MCP client,
scriptable, and emits machine-readable transcripts:

```sh
claude -p "$(cat /task/prompt.md)" \
  --mcp-config /etc/mcp.json --strict-mcp-config \
  --allowedTools "mcp__mnemosyne__*" \
  --output-format stream-json \
  --max-turns 80 \
  > /results/transcript.jsonl
```

with `/etc/mcp.json`:

```json
{
  "mcpServers": {
    "mnemosyne": {
      "command": "/usr/local/bin/mnemosyne-mcp-server",
      "args": ["--data-dir", "/mnemosyne-data", "--allow-all"]
    }
  }
}
```

**Pluggable model URL.** The agent container never holds real credentials or
a hardcoded provider. It points at the sidecar:

```sh
ANTHROPIC_BASE_URL=http://litellm:4000   # LiteLLM serves /v1/messages
ANTHROPIC_AUTH_TOKEN=dummy-key
```

LiteLLM's config maps a single logical model name (e.g. `task-model`) to
whatever is affordable that week — Anthropic, OpenRouter, an OpenAI
endpoint, or a local model via Ollama (`ollama/...` with the Ollama host
reachable from the proxy container). Switching models is a one-line YAML
change on the host; nothing in the task images changes. LiteLLM also gives
per-run token/cost accounting for free.

**Open-source alternative: Goose** (`block/goose`). Native MCP client
("extensions"), headless `goose run`, and providers are natively pluggable
(OpenAI-compatible base URL, Ollama, OpenRouter) without a proxy. Worth
keeping the task format harness-agnostic so a run can be re-driven with Goose
— both to avoid Claude-Code-specific behavior skewing conclusions and for
fully-local, zero-credit runs. OpenHands also speaks MCP if a heavier
harness is ever wanted. (mini-swe-agent is *not* suitable: bash-only by
design, no MCP.)

### Tool-access modes

Every task runs in one of two modes; this is the most informative axis short
of a full baseline:

- **`mnemosyne-only`** — the harness gets *only* the four Mnemosyne tools
  (`--allowedTools "mcp__mnemosyne__*"`, built-in Bash/Edit/Write denied).
  All perception and action must flow through `clojure_eval` (with
  `mnemosyne.shell` available under `--allow-all`). This is the hard test of
  whether the substrate is sufficient.
- **`mixed`** — Mnemosyne tools *and* the harness's native tools. Measures
  voluntary adoption: given a shell, does the model still find
  `function_lookup`/`save_function` worth using? (The `initialize`
  instructions explicitly push the lookup-first workflow; this mode tests
  whether that onboarding text works.)

## Scenarios

Four task families, each playing to a different part of the stack. Each
task is a directory with a `task.yaml`, a fixture, and a grader (see layout
below).

### 1. `bugfix-clj` — fix a bug in a source tree

**Fixture:** a small self-contained Clojure library (~5 namespaces, e.g. an
ISO-8601 date-parsing + interval library) with 2 seeded bugs (one shallow —
an off-by-one; one requiring actual understanding — a timezone-handling
error) and a *visible* test file that exercises the happy path only. Baked
into the image at `/task/project`.

**Prompt:** "Tests are failing / users report X. Find and fix the bugs in
`/task/project`. Do not modify the test files."

**Why Clojure:** `clojure_eval`'s persistent runtime is genuinely the best
tool available — the agent can load the namespaces, poke at the functions
REPL-style, and verify fixes interactively. This is the scenario where
Mnemosyne should shine rather than merely keep up.

**Grading:** grader container runs a *hidden* test suite (superset of the
visible one, including regression tests for both seeded bugs) against the
agent's `/task/project`, plus a check that visible test files are
byte-identical to the originals. Score: hidden-suite pass rate.

**Expected Mnemosyne usage:** heavy `clojure_eval`; `function_lookup` against
the seeded built-ins; ideally `save_function` for helper predicates it builds
while diagnosing.

### 2. `bugfix-py` — fix a bug in a mainstream-language source tree

**Fixture:** a small Python project (~8 modules; e.g. a Flask JSON API with a
service layer and a data-munging module) with 2 seeded bugs mirroring the
Clojure task's difficulty split: one shallow (a mutable default argument /
off-by-one in pagination) and one requiring real understanding (a timezone or
encoding error that only manifests on certain inputs). Visible happy-path
tests via `pytest`; `python3` and `pytest` preinstalled in the image. Baked
into the image at `/task/project`.

**Prompt:** same shape as `bugfix-clj`: "Users report X; some tests fail.
Find and fix the bugs in `/task/project`. Do not modify the test files."

**Why a non-Clojure task matters:** this is the *common case* for a real
user — the code being worked on is not Clojure, so `clojure_eval`'s REPL
cannot host the task's code directly. Mnemosyne's contribution has to come
from `mnemosyne.shell` (running `pytest`, `grep`, applying edits) and from
saved/looked-up helper functions (e.g. a "run tests and summarize failures"
function). In `mnemosyne-only` mode this is the sharpest ergonomics test of
shell-through-Clojure on someone else's stack; in `mixed` mode it asks the
honest question of whether the model bothers with Mnemosyne at all when the
task language doesn't match — if adoption is ~zero here, that's a finding
about positioning, not a test failure. A TypeScript twin (`bugfix-ts`, same
recipe with `vitest`) is a cheap later addition if we want a second data
point; Python first since it's the most common case and keeps the image lean.

**Grading:** identical machinery to `bugfix-clj`: hidden `pytest` suite
(superset including regressions for both seeded bugs) run in the grader
container against the agent's tree, plus byte-identity check on visible test
files. Score: hidden-suite pass rate.

**Expected Mnemosyne usage:** `clojure_eval` + `mnemosyne.shell` as the
edit/test loop; `save_function` for reusable dev-workflow helpers (test
runner, failure parser) rather than domain logic — which is exactly the kind
of function Phase 2 carryover should reward.

### 3. `sadserver-webstack` — diagnose a malfunctioning system

**Fixture:** the agent container itself is the sick box (sadservers.com
style): nginx reverse-proxying a small app backed by SQLite, plus a cron'd
log writer. A `break.sh` runs at container start and applies 2–3 faults from
a small catalog, selected by seed — e.g. nginx upstream pointing at the
wrong port, the app's env file corrupted, disk nearly full from a runaway
log, a wrong file permission on the SQLite db. The catalog lives outside the
image; `break.sh` receives only the chosen fault implementations, not the
catalog or descriptions.

**Prompt:** "`curl localhost:8080/health` should return 200 with body `ok`
and `POST /items` should persist. It doesn't. Diagnose and fix. The fix must
survive a service restart."

**Grading:** grader script executes health probes via `docker exec` into the
(still running) agent container *after* the harness exits: restart the
services, then check `/health`, a write-read round trip, and that free disk
is above a threshold. (This is the one scenario where the grader touches the
live container rather than a mounted snapshot, since the state under test is
running processes; the harness process is killed first.)

**Expected Mnemosyne usage:** `mnemosyne.shell` from `clojure_eval` for
inspection (`ps`, `ss`, config files); building small parse/check functions;
in `mnemosyne-only` mode this is a strong test that shell-through-Clojure is
ergonomic enough for ops work.

### 4. `logs-analysis` — derive facts from access logs

**Fixture:** `generate.py` (runs on the host at task-build time) produces
~500 MB of combined-format nginx access logs at `/task/logs/` with injected
ground-truth phenomena: a scraper bot with a distinctive UA rotating through
an IP block, a 14-minute 5xx burst caused by one endpoint, a top-N traffic
distribution, and a low-and-slow credential-stuffing pattern against
`/login`. Ground truth (`answers.json`) is emitted on the host and never
enters the image.

**Prompt:** "Analyze the logs in `/task/logs/`. Write your answers to
`/task/answers.json` with this schema: top 5 client IPs by total bytes; the
UTC start of the worst 5xx window and the endpoint responsible; the
user-agent string of the scraper; the number of distinct IPs involved in the
`/login` attack."

**Grading:** grader compares `/task/answers.json` field-by-field against
ground truth (exact for categorical answers, tolerance windows for
timestamps). Partial credit per field.

**Expected Mnemosyne usage:** this is the `save_function` showcase — log-line
parser, windowed aggregation, top-N — and the natural seed for Phase 2
carryover.

## Phase 2: memory carryover (the differentiator)

Run task pairs against the **same `/mnemosyne-data` volume** but a fresh
agent session (and fresh task container):

- `logs-analysis` → `logs-analysis-2` (same schema, new seed, one new
  question). Measure: does session 2 `function_lookup` and reuse the parser
  saved in session 1? Time/turns/tokens delta between the sessions is the
  headline number.
- `bugfix-clj` → `bugfix-clj-2` (related library, overlapping domain).
- `bugfix-py` → `bugfix-py-2` (different Python project, same workflow).
  The interesting reuse here is *cross-project dev tooling* — the test-runner
  and failure-parser helpers from session 1 apply verbatim to a new codebase,
  unlike domain functions.

Detection is mechanical: the data dir is a git repo, so "functions saved in
session 1" is a commit range, and the session-2 transcript shows whether
`function_lookup` returned them and whether `clojure_eval` called them. A
control run of session 2 against an *empty* volume gives the within-phase-2
comparison without needing the full baseline matrix.

## Repository layout

```
e2e/
  README.md
  run.sh                    # orchestrator: build → run agent → stop → grade → collect
  docker/
    agent.Dockerfile        # harness + mnemosyne-mcp-server (built with `semantic`
                            #   feature; embedding model pre-fetched at build time
                            #   since the task network has no HuggingFace access)
    litellm/config.yaml     # model routing; the only file touched to switch models
  tasks/
    <task-id>/
      task.yaml             # prompt, mode, io-policy flags, timeout, max-turns, seed
      fixture/              # baked into the task layer of the agent image
      grader/               # NEVER copied into any agent-reachable image
  results/<run-id>/
      transcript.jsonl      # stream-json from the harness
      grade.json            # grader output (score, per-check detail)
      metrics.json          # turns, wall time, cache-aware token/cost fields,
                            #   per-tool call counts, Mnemosyne token
                            #   attribution, saves/lookups/reuse
      litellm-usage.jsonl   # raw per-request proxy log, kept verbatim so
                            #   token analyses can be redone without re-running
      mnemosyne-data.tar    # snapshot of the function store after the run
```

`task.yaml` sketch:

```yaml
id: logs-analysis
mode: mnemosyne-only          # or: mixed
io_policy: allow-all          # server flags for the run
timeout_minutes: 30
max_turns: 80
seed: 42
prompt_file: prompt.md
grader: grader/grade.py       # run on host / in grader container
```

## Metrics

Per run, extracted from the transcript + LiteLLM + grader:

| Metric | Source |
|---|---|
| Task score (0–1, per-check detail) | grader |
| Turns, wall time | transcript |
| Token usage and $ cost (cache-aware; see below) | LiteLLM logs + transcript `usage` |
| Calls per tool (`clojure_eval`, `function_lookup`, `save_function`, `annotate_function`) | transcript |
| Mnemosyne token attribution (see below) | transcript |
| Eval errors / protocol errors / tool-call failures | transcript + server stderr |
| Functions saved; annotated fraction | data-dir git log |
| (Phase 2) stored-function reuse: lookups that returned session-1 functions and were subsequently eval'd | transcript × git log |

Every scenario should run with ≥3 seeds before believing any number; LLM
task success is high-variance.

### Token efficiency

Token efficiency is a first-class metric — "reuse stored functions instead
of re-deriving them" is in large part a token-economy claim. The numbers are
imperfect (estimates below are estimates); we record them anyway, because a
measurable proxy beats an unmeasured claim. Two independent sources, both
kept for every run from Phase 0 onward so analyses can be added
retroactively without re-spending credits:

- **LiteLLM proxy logs** — ground truth for total spend: per-request
  prompt/completion/cache tokens and computed cost, including retries and
  anything the harness does not self-report. One proxy instance per run (it
  is a throwaway sidecar), so attribution is free.
- **Harness transcript** — Claude Code's final `result` message carries a
  `usage` block (input, output, cache-creation, cache-read tokens) and
  `total_cost_usd`; the per-message stream is what enables the decomposition
  below. The two sources cross-check each other; disagreement beyond
  retry noise is itself a bug report.

**Cache-aware definitions.** An agent loop resends the growing conversation
every turn, so *raw* input tokens grow roughly quadratically with turns while
most of them are cache reads billed at a small fraction of the fresh-token
price. Raw input tokens therefore overstate the cost of long runs and must
not be used as the headline. `metrics.json` records:

| Field | Definition |
|---|---|
| `cost_usd` | headline; from LiteLLM (absent for local models — compare tokens instead) |
| `output_tokens` | headline; tracks actual generation work |
| `input_fresh_tokens` | non-cached input + cache-creation tokens |
| `input_cache_read_tokens` | reported separately, never summed into a headline |
| `turns` | denominator for per-turn normalization |

Cross-model comparisons use `cost_usd` only (tokenizers differ);
within-model comparisons — Mnemosyne vs plain, carryover session 1 vs 2 —
may use token counts directly. That is exactly the shape of the Phase 2/3
questions, so the restriction costs nothing.

**Mnemosyne attribution.** Total cost says whether a run was expensive; the
decomposition says whether *Mnemosyne* was the expensive part. Mnemosyne
consumes context three ways, all visible in the transcript:

1. fixed per-turn overhead: the four tool schemas + `initialize`
   instructions text (constant per run configuration; measured once);
2. `function_lookup` results — full source blobs, the price of retrieval;
3. `clojure_eval` / other tool-result payloads.

The `metrics.json` post-processor sums `tool_result` content sizes per tool
and converts to approximate tokens (chars/4, or a local tokenizer — this is
for decomposition, not billing, so an estimate is acceptable). This is what
lets a carryover result be stated as "session 2 spent 8k input tokens on
lookup results and saved ~30k output tokens of re-derivation" rather than a
single opaque delta — and, in `mixed` mode, what shows whether lookup
results are cheaper than the shell exploration they replace.

## Phases

1. **Phase 0 — plumbing smoke test.** No LLM. A scripted MCP client (or
   `claude -p` with a trivial one-tool prompt) inside the container:
   handshake, `tools/list`, one call per tool, one deliberately malformed
   call, one 10 MB eval output. Cheap enough for CI on every commit.
2. **Phase 1 — the four scenarios**, single-session, both modes, 3 seeds
   each (24 runs). Deliverables: pass rates, tool-adoption stats, and a
   qualitative failure review from transcripts. Expect this phase to mostly
   surface *server* issues (instruction text the model misreads, eval output
   truncation, error messages the model can't act on) — that's the point.
3. **Phase 2 — memory carryover pairs** as above, plus empty-volume controls.
4. **Phase 3 (future) — baselines.** Same tasks, same seeds: (a) harness with
   native tools only, no Mnemosyne (`mixed` minus MCP config); (b) model
   matrix via LiteLLM (frontier vs cheap vs local). The infrastructure above
   is deliberately already shaped for this — a baseline run is just a
   `task.yaml` with `mode: no-mnemosyne`.

## Open questions

- **Semantic feature in-container:** first semantic `function_lookup`
  downloads a BERT model from HuggingFace, which the sealed network forbids.
  Plan: pre-fetch into the image at build time. Fallback: build without
  `semantic` and accept full-text-only lookup for early phases (worth testing
  both, since fulltext-fallback behavior is itself a claim in the README).
- **`clojure_eval` as sole shell:** the `sadserver` scenario in
  `mnemosyne-only` mode may be gated on `mnemosyne.shell` ergonomics
  (timeouts, stderr capture, exit codes). If Phase 1 shows the model fighting
  it, that's a feature request for the tool layer, not a reason to soften the
  scenario.
- **Grader-in-container for sadserver:** `docker exec` grading trusts the
  container's own binaries (`curl`, `sh`) which the agent could have
  tampered with. Acceptable for now; if it ever matters, mount a static
  busybox/probe binary read-only at grade time.
