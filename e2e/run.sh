#!/usr/bin/env bash
# Orchestrator for a single e2e task run: build -> run agent -> stop -> grade -> collect.
#
# Usage:
#   ./run.sh <task-id> [--mode mnemosyne-only|mixed] [--seed N] [--reset-data]
#             [--data-volume NAME] [--run-id ID]
#
# --data-volume overrides the default per-task-id named volume (used by
# run-phase2.sh to point two different task ids at the same /mnemosyne-data
# volume for a Phase 2 carryover pair). --run-id overrides the generated
# run id/results directory name, so a caller can predict where a run's
# transcript.jsonl and mnemosyne-data.tar will land without scraping stdout.
#
# See ../doc/E2E-TESTING.md ("Architecture", "Harness", "Repository layout")
# for the design this implements.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TASK_ID="${1:-}"
if [[ -z "$TASK_ID" || "$TASK_ID" == -* ]]; then
  echo "usage: $0 <task-id> [--mode mnemosyne-only|mixed] [--seed N] [--reset-data] [--data-volume NAME] [--run-id ID]" >&2
  exit 2
fi
shift

TASK_DIR="$SCRIPT_DIR/tasks/$TASK_ID"
if [[ ! -f "$TASK_DIR/task.yaml" ]]; then
  echo "no such task: $TASK_ID (expected $TASK_DIR/task.yaml)" >&2
  exit 2
fi

# --- parse task.yaml (flat key: value schema, see any tasks/*/task.yaml) ---
yaml_get() {
  local key="$1" default="${2:-}"
  local line
  line="$(grep -E "^${key}:" "$TASK_DIR/task.yaml" | head -1 || true)"
  if [[ -z "$line" ]]; then
    echo "$default"
    return
  fi
  # strip "key:", surrounding whitespace, and a trailing "# comment"
  echo "$line" | sed -E "s/^${key}:[[:space:]]*//; s/[[:space:]]*#.*$//"
}

MODE="$(yaml_get mode mnemosyne-only)"
IO_POLICY="$(yaml_get io_policy allow-all)"
TIMEOUT_MINUTES="$(yaml_get timeout_minutes 30)"
MAX_TURNS="$(yaml_get max_turns 80)"
SEED="$(yaml_get seed 1)"
PROMPT_FILE="$(yaml_get prompt_file prompt.md)"
GRADER_REL="$(yaml_get grader grader/grade.py)"

RESET_DATA=0
DATA_VOLUME_OVERRIDE=""
RUN_ID_OVERRIDE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --seed) SEED="$2"; shift 2 ;;
    --reset-data) RESET_DATA=1; shift ;;
    --data-volume) DATA_VOLUME_OVERRIDE="$2"; shift 2 ;;
    --run-id) RUN_ID_OVERRIDE="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ "$MODE" != "mnemosyne-only" && "$MODE" != "mixed" ]]; then
  echo "invalid mode: $MODE (expected mnemosyne-only or mixed)" >&2
  exit 2
fi

RUN_ID="${RUN_ID_OVERRIDE:-${TASK_ID}-${MODE}-seed${SEED}-$(date +%Y%m%d%H%M%S)}"
RESULTS_DIR="$SCRIPT_DIR/results/$RUN_ID"
mkdir -p "$RESULTS_DIR"

BASE_IMAGE="mnemosyne-e2e-agent-base"
# Seed-suffixed: several task fixtures (e.g. logs-analysis) bake seed-derived
# content at build time via --build-arg SEED, so the image itself is
# seed-specific, not just the container.
TASK_IMAGE="mnemosyne-e2e-task-${TASK_ID}-seed${SEED}"
# Defaults to one volume per task id, so re-running the same task-id reuses
# its function store across invocations. Phase 2 carryover pairs (see
# ../doc/E2E-TESTING.md, "Phase 2") point a *different* task-id's run at an
# explicit shared volume via --data-volume -- see run-phase2.sh.
DATA_VOLUME="${DATA_VOLUME_OVERRIDE:-mnemosyne-data-${TASK_ID}}"
NETWORK="tasknet"
AGENT_CONTAINER="mnemosyne-e2e-agent-${RUN_ID}"
LITELLM_CONTAINER="mnemosyne-e2e-litellm-${RUN_ID}"

echo "== task=$TASK_ID mode=$MODE seed=$SEED run_id=$RUN_ID =="

echo "-- building base agent image ($BASE_IMAGE) --"
docker build -f "$SCRIPT_DIR/docker/agent.Dockerfile" -t "$BASE_IMAGE" "$REPO_ROOT"

if [[ -x "$TASK_DIR/fixture/prepare.sh" ]]; then
  echo "-- preparing fixture (host-side, seed-dependent generation) --"
  # Runs on the host, e.g. logs-analysis' generate.py: writes seed-derived
  # content into fixture/ for the Docker build below to COPY, and writes
  # ground truth to $RESULTS_DIR -- which is mounted into the *grader*
  # container later but is never part of the agent image's build context.
  "$TASK_DIR/fixture/prepare.sh" "$SEED" "$TASK_DIR/fixture" "$RESULTS_DIR/ground-truth.json"
fi

echo "-- building task image ($TASK_IMAGE) --"
# Build context is fixture/ only, never the task dir: grader/ must be
# structurally unreachable from a COPY in this Dockerfile, not just unused.
docker build \
  --build-arg BASE_IMAGE="$BASE_IMAGE" \
  --build-arg SEED="$SEED" \
  -f "$TASK_DIR/fixture/Dockerfile" \
  -t "$TASK_IMAGE" \
  "$TASK_DIR/fixture"

if [[ "$RESET_DATA" -eq 1 ]]; then
  echo "-- resetting data volume $DATA_VOLUME --"
  docker volume rm -f "$DATA_VOLUME" >/dev/null 2>&1 || true
fi
docker volume create "$DATA_VOLUME" >/dev/null

echo "-- ensuring internal network $NETWORK --"
docker network create --internal "$NETWORK" >/dev/null 2>&1 || true

echo "-- starting litellm sidecar ($LITELLM_CONTAINER) --"
docker run -d --name "$LITELLM_CONTAINER" \
  --network "$NETWORK" \
  -e "ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY:-}" \
  -e "OPENAI_API_KEY=${OPENAI_API_KEY:-}" \
  -e "TOGETHER_API_KEY=${TOGETHER_API_KEY:-}" \
  -e "OPENROUTER_API_KEY=${OPENROUTER_API_KEY:-}" \
  -v "$SCRIPT_DIR/docker/litellm/config.yaml:/etc/litellm/config.yaml:ro" \
  -v "$RESULTS_DIR:/results" \
  ghcr.io/berriai/litellm:main-stable \
  --config /etc/litellm/config.yaml --port 4000 \
  --detailed_debug --log_file /results/litellm-usage.jsonl >/dev/null

# The proxy is the only container dual-homed onto a network with real
# egress; it needs a second, non-internal network to reach the model API.
docker network connect bridge "$LITELLM_CONTAINER" >/dev/null 2>&1 || true

case "$MODE" in
  mnemosyne-only) ALLOWED_TOOLS="mcp__mnemosyne__*" ;;
  mixed) ALLOWED_TOOLS="mcp__mnemosyne__*,Bash,Edit,Write,Read,Grep,Glob" ;;
esac

MCP_ARGS="--data-dir /mnemosyne-data"
case "$IO_POLICY" in
  allow-all) MCP_ARGS="$MCP_ARGS --allow-all" ;;
  allow-file-io) MCP_ARGS="$MCP_ARGS --allow-file-io" ;;
  allow-network) MCP_ARGS="$MCP_ARGS --allow-network" ;;
  none|"") : ;;
  *) echo "unknown io_policy: $IO_POLICY" >&2; exit 2 ;;
esac

cat > "$RESULTS_DIR/mcp.json" <<EOF
{
  "mcpServers": {
    "mnemosyne": {
      "command": "/usr/local/bin/mnemosyne-mcp-server",
      "args": [$(echo "$MCP_ARGS" | sed -E 's/(\S+)/"\1",/g' | sed 's/,$//')]
    }
  }
}
EOF

HARNESS_CMD="claude -p \"\$(cat /task/${PROMPT_FILE})\" \
  --mcp-config /etc/mcp.json --strict-mcp-config \
  --allowedTools \"${ALLOWED_TOOLS}\" \
  --output-format stream-json \
  --max-turns ${MAX_TURNS} \
  > /results/transcript.jsonl"

echo "-- running agent container ($AGENT_CONTAINER), timeout ${TIMEOUT_MINUTES}m --"
set +e
if [[ "$TASK_ID" == "sadserver-webstack" ]]; then
  # The container's own ENTRYPOINT is the sick box's services (nginx in the
  # foreground as PID 1) -- that has to keep running for the harness to
  # diagnose it live, so the harness runs as a second process via `docker
  # exec` into an already-started, detached container, instead of being
  # the container's main command like every other task.
  docker run -d --name "$AGENT_CONTAINER" \
    --network "$NETWORK" \
    -e ANTHROPIC_BASE_URL="http://${LITELLM_CONTAINER}:4000" \
    -e ANTHROPIC_AUTH_TOKEN=dummy-key \
    -e MNEMOSYNE_TASK_SEED="$SEED" \
    -v "$DATA_VOLUME:/mnemosyne-data" \
    -v "$RESULTS_DIR/mcp.json:/etc/mcp.json:ro" \
    -v "$RESULTS_DIR:/results" \
    "$TASK_IMAGE" >/dev/null
  sleep 3 # let nginx/the app/the log writer come up before the agent probes them
  docker exec --user mnemosyne "$AGENT_CONTAINER" \
    timeout "${TIMEOUT_MINUTES}m" bash -c "$HARNESS_CMD"
  HARNESS_EXIT=$?
else
  docker run --name "$AGENT_CONTAINER" \
    --network "$NETWORK" \
    --user mnemosyne \
    -e ANTHROPIC_BASE_URL="http://${LITELLM_CONTAINER}:4000" \
    -e ANTHROPIC_AUTH_TOKEN=dummy-key \
    -e MNEMOSYNE_TASK_SEED="$SEED" \
    -v "$DATA_VOLUME:/mnemosyne-data" \
    -v "$RESULTS_DIR/mcp.json:/etc/mcp.json:ro" \
    -v "$RESULTS_DIR:/results" \
    "$TASK_IMAGE" \
    timeout "${TIMEOUT_MINUTES}m" bash -c "$HARNESS_CMD"
  HARNESS_EXIT=$?
fi
set -e
echo "$HARNESS_EXIT" > "$RESULTS_DIR/harness_exit_code"

if [[ "$TASK_ID" == "sadserver-webstack" ]]; then
  echo "-- sadserver-webstack: grading against the live container before teardown --"
  # The one scenario where the grader touches the running agent container
  # rather than a snapshot, because the state under test is running
  # processes surviving a restart. The harness is already dead (docker run
  # above returned); the container itself stays up until the grader is done.
  python3 "$TASK_DIR/grader/grade.py" \
    --live-container "$AGENT_CONTAINER" \
    --out "$RESULTS_DIR/grade.json"
  docker stop "$AGENT_CONTAINER" >/dev/null
else
  echo "-- snapshotting /task from the agent container --"
  SNAPSHOT_DIR="$RESULTS_DIR/task-snapshot"
  mkdir -p "$SNAPSHOT_DIR"
  docker cp "${AGENT_CONTAINER}:/task/." "$SNAPSHOT_DIR" 2>/dev/null || true
  docker stop "$AGENT_CONTAINER" >/dev/null 2>&1 || true

  echo "-- grading (network-less grader container) --"
  if [[ -x "$TASK_DIR/grader/build.sh" ]]; then
    # Host-side build step for graders that aren't plain scripts (e.g.
    # bugfix-clj's grader links mnemosyne-execution-engine itself, so it's
    # cargo-built here against the full repo checkout and only the
    # resulting binary -- never the repo source -- ends up under grader/,
    # keeping the Docker build context below limited to grader/ alone).
    "$TASK_DIR/grader/build.sh"
  fi
  GRADER_IMAGE="python:3.11-slim"
  if [[ -f "$TASK_DIR/grader/Dockerfile" ]]; then
    # Grader has its own deps (e.g. pytest/flask, or a Clojure runtime) that
    # must be baked in at build time: the grader container runs with
    # --network none, so nothing can be installed at grade time.
    GRADER_IMAGE="mnemosyne-e2e-grader-${TASK_ID}"
    docker build -t "$GRADER_IMAGE" "$TASK_DIR/grader" >/dev/null
  fi
  EXTRA_GRADER_ARGS=()
  if [[ -f "$RESULTS_DIR/ground-truth.json" ]]; then
    EXTRA_GRADER_ARGS+=(--ground-truth /results/ground-truth.json)
  fi
  GRADER_CMD=(python3 "/grader/$(basename "$GRADER_REL")")
  if [[ "$GRADER_REL" != *.py ]]; then
    GRADER_CMD=("/grader/$(basename "$GRADER_REL")")
  fi
  docker run --rm \
    --network none \
    -v "$SNAPSHOT_DIR:/task:ro" \
    -v "$TASK_DIR/grader:/grader:ro" \
    -v "$RESULTS_DIR:/results" \
    "$GRADER_IMAGE" \
    "${GRADER_CMD[@]}" --task-dir /task --out /results/grade.json \
    "${EXTRA_GRADER_ARGS[@]}"
fi

echo "-- collecting mnemosyne-data snapshot --"
docker run --rm -v "$DATA_VOLUME:/data:ro" -v "$RESULTS_DIR:/results" alpine \
  tar -C /data -cf /results/mnemosyne-data.tar .

echo "-- tearing down run containers --"
docker rm -f "$AGENT_CONTAINER" >/dev/null 2>&1 || true
docker rm -f "$LITELLM_CONTAINER" >/dev/null 2>&1 || true

echo "== done: $RESULTS_DIR (harness exit $HARNESS_EXIT) =="
