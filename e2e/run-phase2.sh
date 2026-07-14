#!/usr/bin/env bash
# Orchestrator for a Phase 2 memory-carryover pair (see ../doc/E2E-TESTING.md,
# "Phase 2: memory carryover"): runs a task's session 1, then its session-2
# companion twice -- once against session 1's /mnemosyne-data volume
# (carryover) and once against a fresh, empty volume (control) -- then runs
# analyze-carryover.py to report whether session 2 looked up and reused
# functions session 1 saved, plus the turn/token deltas the plan doc calls
# the Phase 2 headline number.
#
# Usage:
#   ./run-phase2.sh <pair> [--mode mnemosyne-only|mixed] [--seed1 N] [--seed2 N]
#
# <pair> is the session-1 task id (logs-analysis, bugfix-clj, or bugfix-py);
# its session-2 companion is "<pair>-2" and must exist under tasks/.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

PAIR="${1:-}"
if [[ -z "$PAIR" || "$PAIR" == -* ]]; then
  echo "usage: $0 <pair> [--mode mnemosyne-only|mixed] [--seed1 N] [--seed2 N]" >&2
  exit 2
fi
shift

SESSION1_TASK="$PAIR"
SESSION2_TASK="${PAIR}-2"
for t in "$SESSION1_TASK" "$SESSION2_TASK"; do
  if [[ ! -f "$SCRIPT_DIR/tasks/$t/task.yaml" ]]; then
    echo "no such task: $t (expected $SCRIPT_DIR/tasks/$t/task.yaml)" >&2
    exit 2
  fi
done

MODE="mnemosyne-only"
SEED1=1
SEED2=2
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --seed1) SEED1="$2"; shift 2 ;;
    --seed2) SEED2="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ "$MODE" != "mnemosyne-only" && "$MODE" != "mixed" ]]; then
  echo "invalid mode: $MODE (expected mnemosyne-only or mixed)" >&2
  exit 2
fi

RUN_TAG="phase2-${PAIR}-${MODE}-$(date +%Y%m%d%H%M%S)"
# Ephemeral volumes scoped to this experiment only -- not the per-task-id
# volumes plain `run.sh` invocations default to -- so a phase-2 run never
# collides with (or contaminates) a standalone Phase 1 run of the same task.
CARRYOVER_VOLUME="mnemosyne-data-${RUN_TAG}-carryover"
CONTROL_VOLUME="mnemosyne-data-${RUN_TAG}-control"

RUN1_ID="${RUN_TAG}-session1"
RUN2_ID="${RUN_TAG}-session2-carryover"
RUN2_CONTROL_ID="${RUN_TAG}-session2-control"

echo "== phase 2: pair=$PAIR mode=$MODE seed1=$SEED1 seed2=$SEED2 tag=$RUN_TAG =="

echo "-- session 1: $SESSION1_TASK (fresh volume $CARRYOVER_VOLUME) --"
"$SCRIPT_DIR/run.sh" "$SESSION1_TASK" --mode "$MODE" --seed "$SEED1" \
  --data-volume "$CARRYOVER_VOLUME" --reset-data --run-id "$RUN1_ID"

echo "-- session 2 (carryover): $SESSION2_TASK on session 1's volume --"
"$SCRIPT_DIR/run.sh" "$SESSION2_TASK" --mode "$MODE" --seed "$SEED2" \
  --data-volume "$CARRYOVER_VOLUME" --run-id "$RUN2_ID"

echo "-- session 2 (control): $SESSION2_TASK on an empty volume --"
"$SCRIPT_DIR/run.sh" "$SESSION2_TASK" --mode "$MODE" --seed "$SEED2" \
  --data-volume "$CONTROL_VOLUME" --reset-data --run-id "$RUN2_CONTROL_ID"

REPORT_DIR="$SCRIPT_DIR/results/$RUN_TAG"
mkdir -p "$REPORT_DIR"

echo "-- analyzing carryover (git commit range x transcript cross-reference) --"
python3 "$SCRIPT_DIR/analyze-carryover.py" \
  --session1-dir "$SCRIPT_DIR/results/$RUN1_ID" \
  --session2-dir "$SCRIPT_DIR/results/$RUN2_ID" \
  --control-dir "$SCRIPT_DIR/results/$RUN2_CONTROL_ID" \
  --out "$REPORT_DIR/carryover-report.json"

echo "-- cleaning up phase-2 scratch volumes ($CARRYOVER_VOLUME, $CONTROL_VOLUME) --"
docker volume rm -f "$CARRYOVER_VOLUME" "$CONTROL_VOLUME" >/dev/null 2>&1 || true

echo "== phase 2 done: $REPORT_DIR/carryover-report.json =="
