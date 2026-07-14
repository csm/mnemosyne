#!/usr/bin/env python3
"""Grader for logs-analysis-2 (Phase 2 companion of logs-analysis; see
../../../../doc/E2E-TESTING.md, "Phase 2: memory carryover").

Runs OUTSIDE the agent's reach (see e2e/README.md "Anti-cheating"): invoked
by run.sh in a network-less container that mounts the agent's /task
read-only, this grader/ directory, and /results (which holds
ground-truth.json, written on the host by fixture/prepare.sh and never part
of any agent-reachable image).

Same schema and scoring as logs-analysis/grader/grade.py, plus the one new
question this task pair adds: `total_request_count`, checked to within a
1% tolerance (an exact line-count match is brittle to off-by-one
double-counting of a boundary request, which isn't the thing being
measured here).
"""
import argparse
import json
from datetime import datetime, timezone
from pathlib import Path

TIMESTAMP_TOLERANCE_SECONDS = 120


def parse_ts(raw: str) -> datetime:
    return datetime.fromisoformat(raw.replace("Z", "+00:00")).astimezone(timezone.utc)


def grade(answers: dict, truth: dict) -> dict:
    checks = {}

    submitted_top5 = answers.get("top_5_ips_by_bytes") or []
    truth_top5 = truth["top_5_ips_by_bytes"]
    overlap = len(set(submitted_top5) & set(truth_top5))
    checks["top_5_ips_by_bytes"] = {
        "score": overlap / len(truth_top5),
        "submitted": submitted_top5,
        "expected": truth_top5,
    }

    ts_ok = False
    ts_reason = "missing or unparsable"
    submitted_ts = answers.get("worst_5xx_window_start_utc")
    if submitted_ts:
        try:
            delta = abs((parse_ts(submitted_ts) - parse_ts(truth["worst_5xx_window_start_utc"])).total_seconds())
            ts_ok = delta <= TIMESTAMP_TOLERANCE_SECONDS
            ts_reason = f"delta_seconds={delta}"
        except ValueError as exc:
            ts_reason = str(exc)
    checks["worst_5xx_window_start_utc"] = {
        "score": 1.0 if ts_ok else 0.0,
        "submitted": submitted_ts,
        "expected": truth["worst_5xx_window_start_utc"],
        "reason": ts_reason,
    }

    endpoint_ok = answers.get("worst_5xx_window_endpoint") == truth["worst_5xx_window_endpoint"]
    checks["worst_5xx_window_endpoint"] = {
        "score": 1.0 if endpoint_ok else 0.0,
        "submitted": answers.get("worst_5xx_window_endpoint"),
        "expected": truth["worst_5xx_window_endpoint"],
    }

    ua_ok = answers.get("scraper_user_agent") == truth["scraper_user_agent"]
    checks["scraper_user_agent"] = {
        "score": 1.0 if ua_ok else 0.0,
        "submitted": answers.get("scraper_user_agent"),
        "expected": truth["scraper_user_agent"],
    }

    submitted_count = answers.get("credential_stuffing_distinct_ip_count")
    expected_count = truth["credential_stuffing_distinct_ip_count"]
    count_score = 0.0
    if isinstance(submitted_count, (int, float)):
        tolerance = max(2, round(expected_count * 0.1))
        count_score = 1.0 if abs(submitted_count - expected_count) <= tolerance else 0.0
    checks["credential_stuffing_distinct_ip_count"] = {
        "score": count_score,
        "submitted": submitted_count,
        "expected": expected_count,
    }

    submitted_total = answers.get("total_request_count")
    expected_total = truth["total_request_count"]
    total_score = 0.0
    if isinstance(submitted_total, (int, float)):
        tolerance = max(5, round(expected_total * 0.01))
        total_score = 1.0 if abs(submitted_total - expected_total) <= tolerance else 0.0
    checks["total_request_count"] = {
        "score": total_score,
        "submitted": submitted_total,
        "expected": expected_total,
    }

    overall_score = sum(c["score"] for c in checks.values()) / len(checks)
    return {"score": overall_score, "checks": checks}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--task-dir", required=True)
    ap.add_argument("--ground-truth", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    result = {"task": "logs-analysis-2", "score": 0.0, "checks": {}}
    answers_path = Path(args.task_dir) / "answers.json"
    if not answers_path.exists():
        result["error"] = "/task/answers.json not found"
        Path(args.out).write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
        return

    try:
        answers = json.loads(answers_path.read_text())
    except json.JSONDecodeError as exc:
        result["error"] = f"answers.json is not valid JSON: {exc}"
        Path(args.out).write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
        return

    truth = json.loads(Path(args.ground_truth).read_text())
    result.update(grade(answers, truth))
    Path(args.out).write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
