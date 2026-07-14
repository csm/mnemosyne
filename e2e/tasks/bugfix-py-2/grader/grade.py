#!/usr/bin/env python3
"""Grader for bugfix-py-2.

Runs OUTSIDE the agent's reach (see e2e/README.md "Anti-cheating"): invoked
by run.sh in a network-less container that mounts the agent's /task
read-only plus this grader/ directory. Never baked into any agent image.

Score = fraction of the hidden regression suite (superset of the visible
happy-path suite, see hidden_tests/test_regressions.py) that passes against
the agent's fixed-up project, gated on the visible test file being
byte-identical to the original (an agent "fixing" tests instead of code
scores zero). Structurally identical to bugfix-py/grader/grade.py.
"""
import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def check_visible_tests_untouched(project_dir: Path) -> tuple[bool, str]:
    golden = HERE / "golden" / "test_happy_path.py"
    candidate = project_dir / "tests" / "test_happy_path.py"
    if not candidate.exists():
        return False, "tests/test_happy_path.py is missing"
    if candidate.read_bytes() != golden.read_bytes():
        return False, "tests/test_happy_path.py was modified"
    return True, ""


def run_hidden_suite(project_dir: Path, workdir: Path) -> dict:
    # The submitted /task snapshot is mounted read-only in the grader
    # container. Copy the project into our writable tempdir before adding the
    # hidden test suite so grading does not depend on mutating /task/project.
    run_project_dir = workdir / "project"
    shutil.copytree(
        project_dir,
        run_project_dir,
        ignore=shutil.ignore_patterns(".git", "__pycache__", ".pytest_cache", "tests_hidden"),
    )
    shutil.copytree(HERE / "hidden_tests", run_project_dir / "tests_hidden")

    proc = subprocess.run(
        [
            sys.executable, "-m", "pytest",
            "tests", "tests_hidden",
            "-q", "--tb=short",
            f"--junit-xml={workdir / 'junit.xml'}",
        ],
        cwd=run_project_dir,
        capture_output=True,
        text=True,
        timeout=300,
    )
    return {
        "returncode": proc.returncode,
        "stdout_tail": proc.stdout[-4000:],
        "stderr_tail": proc.stderr[-2000:],
    }


def parse_junit_counts(junit_path: Path) -> dict:
    import xml.etree.ElementTree as ET

    if not junit_path.exists():
        return {"total": 0, "passed": 0, "failed": 0, "errored": 0}
    root = ET.parse(junit_path).getroot()
    suite = root if root.tag == "testsuite" else root.find("testsuite")
    total = int(suite.get("tests", 0))
    failed = int(suite.get("failures", 0))
    errored = int(suite.get("errors", 0))
    skipped = int(suite.get("skipped", 0))
    passed = total - failed - errored - skipped
    return {"total": total, "passed": passed, "failed": failed, "errored": errored}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--task-dir", required=True, help="snapshot of the agent's /task")
    ap.add_argument("--out", required=True, help="where to write grade.json")
    args = ap.parse_args()

    task_dir = Path(args.task_dir)
    project_dir = task_dir / "project"
    out_path = Path(args.out)

    result = {"task": "bugfix-py-2", "score": 0.0, "checks": {}}

    if not project_dir.exists():
        result["checks"]["project_present"] = {"pass": False}
        out_path.write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
        return

    ok, reason = check_visible_tests_untouched(project_dir)
    result["checks"]["visible_tests_untouched"] = {"pass": ok, "reason": reason}
    if not ok:
        out_path.write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
        return

    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        workdir = Path(tmp)
        run_info = run_hidden_suite(project_dir, workdir)
        counts = parse_junit_counts(workdir / "junit.xml")

    result["checks"]["hidden_suite"] = counts
    result["checks"]["hidden_suite"]["returncode"] = run_info["returncode"]
    result["stdout_tail"] = run_info["stdout_tail"]
    if counts["total"] > 0:
        result["score"] = counts["passed"] / counts["total"]

    out_path.write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
