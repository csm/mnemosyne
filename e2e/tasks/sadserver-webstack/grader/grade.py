#!/usr/bin/env python3
"""Grader for sadserver-webstack.

Unlike the other tasks, this one probes the agent's container while it is
still *running* (see e2e/README.md and doc/E2E-TESTING.md's "Architecture":
"the one scenario where the grader touches the live container rather than
a mounted snapshot, since the state under test is running processes"). It
runs on the HOST, invoked by run.sh with `docker exec` access to the
container by name -- there is no answer key here; grading is a black-box
functional check, so no `grader/fault_catalog.py` content or knowledge of
which faults were injected is needed (or used) here.

Caveat inherited from doc/E2E-TESTING.md's open questions: this trusts the
container's own `curl`/`sh`, which the agent could in principle have
tampered with. Acceptable for Phase 1; a mounted static probe binary would
close that gap if it ever matters.
"""
import argparse
import json
import subprocess
import sys
import time
import uuid


def docker_exec(container: str, *cmd: str, timeout: int = 15) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["docker", "exec", container, *cmd],
        capture_output=True, text=True, timeout=timeout,
    )


def check_health(container: str) -> bool:
    r = docker_exec(container, "curl", "-sf", "-o", "/dev/null", "-w", "%{http_code}",
                     "http://127.0.0.1:8080/health")
    return r.returncode == 0 and r.stdout.strip() == "200"


def create_item(container: str, name: str) -> bool:
    r = docker_exec(
        container, "curl", "-sf", "-X", "POST", "http://127.0.0.1:8080/items",
        "-H", "Content-Type: application/json", "-d", json.dumps({"name": name}),
    )
    return r.returncode == 0


def item_present(container: str, name: str) -> bool:
    r = docker_exec(container, "curl", "-sf", "http://127.0.0.1:8080/items")
    if r.returncode != 0:
        return False
    try:
        items = json.loads(r.stdout).get("items", [])
    except json.JSONDecodeError:
        return False
    return any(item.get("name") == name for item in items)


def disk_ok(container: str, min_free_mb: int = 32) -> bool:
    r = docker_exec(container, "df", "--output=avail", "-BM", "/data")
    if r.returncode != 0:
        return False
    lines = [line.strip().rstrip("M") for line in r.stdout.strip().splitlines()]
    if len(lines) < 2:
        return False
    try:
        return int(lines[1]) >= min_free_mb
    except ValueError:
        return False


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--live-container", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    container = args.live_container
    marker = f"e2e-check-{uuid.uuid4().hex[:8]}"

    checks = {}
    checks["health_before_restart"] = check_health(container)
    checks["write_before_restart"] = create_item(container, marker)
    checks["read_before_restart"] = item_present(container, marker)

    restart = docker_exec(container, "/usr/local/bin/restart-services.sh", timeout=30)
    checks["restart_ran"] = restart.returncode == 0
    time.sleep(2)

    checks["health_after_restart"] = check_health(container)
    checks["data_survived_restart"] = item_present(container, marker)
    checks["disk_ok"] = disk_ok(container)

    score = sum(1 for v in checks.values() if v) / len(checks)
    result = {"task": "sadserver-webstack", "score": score, "checks": checks}
    with open(args.out, "w") as f:
        json.dump(result, f, indent=2)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
