#!/usr/bin/env bash
# Host-side fixture prep for the Phase 2 companion of logs-analysis (see
# ../../../../doc/E2E-TESTING.md, "Phase 2: memory carryover"): same schema,
# a new seed, plus one new question (total_request_count, see prompt.md).
#
# Reuses logs-analysis' own generate.py rather than forking it -- the
# schema is deliberately identical (total_request_count is just
# `len(records)`, already implicit in generate.py's output), so there is
# nothing task-specific to regenerate.
set -euo pipefail

SEED="$1"
FIXTURE_DIR="$2"
GROUND_TRUTH_OUT="$3"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GENERATE_PY="$HERE/../../logs-analysis/fixture/generate.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 "$GENERATE_PY" --seed "$SEED" --out-dir "$TMP" --hours 6

rm -rf "$FIXTURE_DIR/logs"
mv "$TMP/logs" "$FIXTURE_DIR/logs"
mkdir -p "$(dirname "$GROUND_TRUTH_OUT")"
mv "$TMP/answers.json" "$GROUND_TRUTH_OUT"
