#!/usr/bin/env bash
# Host-side fixture prep, invoked by ../../../run.sh before the Docker build.
# Runs generate.py on the HOST (this script never runs inside any
# agent-reachable image) and splits its output: `logs/` goes into this
# fixture directory for the Dockerfile to COPY; `answers.json` (ground
# truth) goes to the path run.sh gives us, which is only ever mounted into
# the grader container later.
set -euo pipefail

SEED="$1"
FIXTURE_DIR="$2"
GROUND_TRUTH_OUT="$3"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 "$HERE/generate.py" --seed "$SEED" --out-dir "$TMP" --hours 6

rm -rf "$FIXTURE_DIR/logs"
mv "$TMP/logs" "$FIXTURE_DIR/logs"
mkdir -p "$(dirname "$GROUND_TRUTH_OUT")"
mv "$TMP/answers.json" "$GROUND_TRUTH_OUT"
