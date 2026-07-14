#!/usr/bin/env bash
# Host-side build step, invoked by ../../../run.sh before the grader Docker
# build. Compiles harness/ (which depends on mnemosyne-execution-engine via
# a relative path into the full repo checkout) here on the host, where the
# repo source is actually available, and drops only the resulting binary
# into this directory. The grader Docker build context is this directory
# alone, so the repo source itself never enters any image -- only the
# compiled binary does, exactly like every other task's grader.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cargo build --release --manifest-path "$HERE/harness/Cargo.toml"
cp "$HERE/harness/target/release/bugfix-clj-2-grader" "$HERE/bugfix-clj-2-grader"
