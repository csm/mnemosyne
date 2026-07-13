# Mnemosyne E2E harness

Implements **Phase 1** of [`doc/E2E-TESTING.md`](../doc/E2E-TESTING.md): the
four task scenarios, run single-session in both tool-access modes, against a
real MCP client (Claude Code) driving `mnemosyne-mcp-server`.

Phase 0 (the no-LLM plumbing smoke test) lives in
`mnemosyne-mcp-server/tests/smoke.rs` and runs under plain `cargo test`; it
needs no Docker network and isn't part of this directory.

## Layout

```
e2e/
  run.sh                     orchestrator: build -> run agent -> stop -> grade -> collect
  docker/
    agent.Dockerfile          base image: mnemosyne-mcp-server (built with `semantic`) + Claude Code
    litellm/config.yaml       model routing; the only file touched to switch models
  tasks/<task-id>/
    task.yaml                 prompt file, mode, io-policy, timeout, max-turns, seed
    prompt.md                 the task prompt handed to the harness
    fixture/                  baked into the task's agent image layer (Dockerfile + task files)
    grader/                   NEVER copied into any agent-reachable image; used only after the run
  results/<run-id>/           transcript.jsonl, grade.json, metrics.json, litellm-usage.jsonl, mnemosyne-data.tar
```

## Running a task

```sh
export ANTHROPIC_API_KEY=sk-...       # real key, held only by the litellm sidecar
./run.sh bugfix-py --mode mnemosyne-only --seed 1
./run.sh bugfix-py --mode mixed --seed 1
```

`run.sh`:

1. builds `mnemosyne-e2e-agent-base` from `docker/agent.Dockerfile` (repo root
   as build context, so it can `cargo build` the workspace);
2. builds `mnemosyne-e2e-task-<task-id>` from `tasks/<task-id>/fixture/Dockerfile`,
   which `FROM`s the base image and layers in the task's fixture;
3. creates the `tasknet` internal Docker network (no default egress) and
   starts the `litellm` sidecar dual-homed on `tasknet` and the default
   bridge (the only path to the real model API);
4. runs the agent container on `tasknet` only, with `--allowedTools` set per
   `mode`, and a *named volume* for `/mnemosyne-data` (`mnemosyne-data-<task-id>`
   by default, so Phase 2 carryover pairs can point two runs at the same
   volume);
5. on exit (or `timeout_minutes`), stops the harness and, for
   `sadserver-webstack` only, leaves the agent container **running** so the
   grader can exercise it live (see below), then stops it;
6. runs `tasks/<task-id>/grader/grade.py` in a network-less grader container
   that mounts the agent's `/task` read-only plus the grader directory and
   host-side ground truth — nothing the agent could have tampered with or
   exfiltrated from is ever in the agent's reach;
7. collects `results/<run-id>/*` and removes the task-run containers (the
   data volume is kept; pass `--reset-data` to drop it first).

Run the same task 3+ times with different `--seed` values before trusting a
pass rate; LLM task success is high-variance (see "Metrics" in the plan doc).

## Anti-cheating

Ground truth and grader scripts live under `tasks/<id>/grader/` and are never
part of the Docker build context for the agent image (`agent.Dockerfile` and
`fixture/Dockerfile` only ever `COPY` `fixture/`). `run.sh` enforces this by
building the agent image from a context rooted at `fixture/`, not the task
directory, so a stray `COPY . .` cannot reach `grader/` even by mistake.

## Known limitations of this checkout

This environment has no Docker daemon and no live model credentials, so
`run.sh`'s container/network orchestration is written and reviewed but
**not exercised end-to-end**. Everything that *could* be verified without
Docker or an LLM was, standalone, against the real code the grader will
actually run:

- **bugfix-py**: the fixture imports cleanly; its visible tests pass with
  both seeded bugs present; a from-scratch hidden regression suite (superset
  of the visible tests) fails exactly the two bug-related assertions against
  the buggy fixture and passes 15/15 once both are fixed; `grader/grade.py`
  was run against buggy, fixed, and tampered-visible-test copies and scored
  0.53, 1.0, and 0.0 respectively.
- **bugfix-clj**: loaded and ran against the *actual*
  `mnemosyne-execution-engine` runtime (compiled directly, not
  reimplemented) — same interpreter `clojure_eval` uses. Confirmed both
  seeded bugs are invisible to the visible suite and caught by the hidden
  one (12 tests, 5 failing on the buggy fixture, 0 failing once fixed).
  `grader/build.sh` + the compiled grader binary were run end-to-end against
  buggy/fixed/tampered snapshots with the same 0.58 / 1.0 / 0.0 pattern.
  **This surfaced a real bug in the vendored `cljrs` interpreter** (see
  below) that shaped the fixture's design.
- **logs-analysis**: `generate.py` runs standalone; every injected phenomenon
  (top-5 bytes, the 5xx burst window/endpoint, the scraper UA, the
  credential-stuffing IP count) was independently recomputed from the raw
  log lines with a plain regex scan and matched the generator's own ground
  truth exactly — the signal is genuinely recoverable, not just echoed back.
  `grader/grade.py` scores a correct submission 1.0.
- **sadserver-webstack**: the Flask app's routes were exercised directly
  (health/create/list); `compose_break.py`'s seed-based fault selection was
  checked for valid, syntactically-correct shell across several seeds. The
  full container lifecycle (nginx + the app + the log writer + `docker exec`
  grading) is unexercised — it needs Docker and is the least-verified task.

### A real finding: `re-matches` is broken in `cljrs` 0.1.223

While building the bugfix-clj fixture, direct use of `clojure.core/re-matches`
(full-string regex match) reliably returned `nil` for every pattern, including
trivial ones like `(re-matches #"abc" "abc")`. The cause is in
`cljrs-value-0.1.223/src/regex.rs`'s `Matcher::next`: for a full-match
(`match_all`) matcher it gates the state transition on
`cap.len() == haystack.len()` — comparing the *capture-group count* to the
*string's byte length*, which is essentially never true. The match is found
but never surfaces, and `re-matches` silently returns `nil` instead of the
match or an error. `re-find` (not full-match) is unaffected and was used in
`dates/parse.clj` instead. This is exactly the category of "server issue"
Phase 1 is meant to surface (see `doc/E2E-TESTING.md`'s Phase 1 description)
— worth a fix or at least a tracked issue upstream in `cljrs-value` before
relying on `re-matches` anywhere else in this codebase.

Separately: `(ns ...)` is a real special form (creates genuinely isolated
namespaces) but `require`/`load-file`/`in-ns` are stubbed no-ops in this
runtime, and `mnemosyne-mcp-server` never configures `cljrs`'s source-path
resolution — so multi-file Clojure projects can't `require` each other the
way a normal `lein`/`clj` project would. The bugfix-clj fixture works around
this by never calling `(ns ...)` in its own source files (everything loads
flat into `user`, with module-prefixed function names to avoid collisions)
and documents the required load order in `tasks/bugfix-clj/fixture/project/README.md`.
Whether an external agent discovers and works within this constraint on its
own is itself one of the things Phase 1 should measure.

Before a real run: build the images, confirm `claude` and
`mnemosyne-mcp-server` are on `PATH` inside the base image, and confirm the
LiteLLM sidecar reaches the configured model host from wherever `run.sh` is
invoked.
