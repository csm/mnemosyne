#!/usr/bin/env bash
# Host-side fixture prep, invoked by ../../../run.sh before the Docker
# build. Delegates to compose_break.py (see there for why it's host-side:
# the fault catalog it reads lives in ../grader, which must never be part
# of any agent-reachable image).
set -euo pipefail

SEED="$1"
FIXTURE_DIR="$2"
# $3 (ground-truth path) is unused: sadserver-webstack is graded as a
# black-box functional check after a restart (see grader/grade.py), not
# against a stored answer key.

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 "$HERE/compose_break.py" --seed "$SEED" --out "$FIXTURE_DIR/break.sh"
