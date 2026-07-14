#!/usr/bin/env python3
"""Phase 2 memory-carryover analysis (see ../doc/E2E-TESTING.md, "Phase 2:
memory carryover" and "Metrics").

Detection is mechanical, per the plan doc: `/mnemosyne-data` is a plain git
repo (functions live at `code/src/<namespace path>.clj`, one commit per
`save_function` call), so "functions saved in session 1" is exactly the set
of top-level `defn`/`defn-`/`defmacro`/`def` forms present in that repo at
the end of session 1. Given that set, this script:

  1. Diffs session 1's and session 2's `mnemosyne-data.tar` snapshots to
     recover that set and the commit range session 2 added on top of it.
  2. Scans session 2's `transcript.jsonl` for `function_lookup` calls whose
     results resolve to a session-1 function (by namespace/name, and by
     commit ancestry when the lookup pinned an exact `@commit`), and for
     `clojure_eval` calls after such a lookup that invoke the looked-up
     name -- the "lookup returned it and eval called it" reuse signal the
     plan doc asks for.
  3. Pulls each run's turn/token/cost figures out of its transcript so the
     carryover-vs-control delta (the Phase 2 headline number) is reported
     alongside the reuse detail, not just a bare pass/fail.

Every transcript field this script reads is read defensively (`.get` with
fallbacks, `try`/`except` around JSON parsing) because the exact
stream-json shape emitted by the harness version in use can drift; a
missing field degrades a metric to `null` rather than crashing the whole
report.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import tarfile
import tempfile
from pathlib import Path

DEF_RE = re.compile(r"^\s*\((?:defn-|defn|defmacro|def)\s+([^\s()\[\]{}\"]+)", re.MULTILINE)
# `;; ns/name@commit` header emitted by function_lookup's exact-lookup path
# (see mnemosyne-mcp/src/tools/lookup.rs `exact()`).
EXACT_HEADER_RE = re.compile(
    r";;\s*([A-Za-z][\w.\-]*)/([A-Za-z][\w\-?!*+<>=]*)@([0-9a-f]{7,40})\b"
)
# `N. ns/name  (score ..., file_path)` search-hit lines (semantic/fulltext
# path; see lookup.rs `render_hits()`), which carry a name but no commit.
SEARCH_HIT_RE = re.compile(
    r"^\d+\.\s+([A-Za-z][\w.\-]*)/([A-Za-z][\w\-?!*+<>=]*)\s+\(score", re.MULTILINE
)


def git(repo_dir: Path, *args: str) -> str:
    proc = subprocess.run(
        ["git", "-C", str(repo_dir), *args],
        capture_output=True, text=True,
    )
    return proc.stdout.strip() if proc.returncode == 0 else ""


def git_ok(repo_dir: Path, *args: str) -> bool:
    """Like `git()`, but for commands (e.g. `cat-file -e`) whose success is
    signaled purely by exit code with no stdout to check truthiness of."""
    proc = subprocess.run(
        ["git", "-C", str(repo_dir), *args],
        capture_output=True, text=True,
    )
    return proc.returncode == 0


def extract_tar(tar_path: Path, dest: Path) -> Path | None:
    """Extract a mnemosyne-data.tar snapshot and return its `code/`
    (git repo) subdirectory, or None if the tar/repo isn't there."""
    if not tar_path.exists():
        return None
    with tarfile.open(tar_path) as tf:
        tf.extractall(dest)
    code_dir = dest / "code"
    return code_dir if (code_dir / ".git").exists() else None


def path_to_namespace(rel_path: str) -> str | None:
    stripped = rel_path[len("src/"):] if rel_path.startswith("src/") else rel_path
    for suffix in (".clj", ".cljc", ".cljs"):
        if stripped.endswith(suffix):
            stripped = stripped[: -len(suffix)]
            break
    else:
        return None
    return stripped.replace("/", ".").replace("_", "-")


def functions_at_head(repo_dir: Path) -> set[tuple[str, str]]:
    """(namespace, name) pairs for every top-level def in the repo's
    current HEAD, mirroring mnemosyne-mcp's clj::top_level_defs well enough
    for detection purposes (this is a metrics script, not the real parser)."""
    head = git(repo_dir, "rev-parse", "HEAD")
    if not head:
        return set()  # empty repo, no commits yet
    out = set()
    files = git(repo_dir, "ls-tree", "-r", "--name-only", "HEAD", "--", "src")
    for rel_path in filter(None, files.splitlines()):
        ns = path_to_namespace(rel_path)
        if ns is None:
            continue
        content = git(repo_dir, "show", f"HEAD:{rel_path}")
        for m in DEF_RE.finditer(content):
            out.add((ns, m.group(1)))
    return out


def commits_since(repo_dir: Path, since_commit: str) -> list[dict]:
    if not since_commit:
        rev_range = "HEAD"
    else:
        if not git_ok(repo_dir, "cat-file", "-e", since_commit):
            return []  # session 1's HEAD isn't reachable in this repo at all
        rev_range = f"{since_commit}..HEAD"
    log = git(repo_dir, "log", rev_range, "--format=%H%x1f%s")
    commits = []
    for line in filter(None, log.splitlines()):
        parts = line.split("\x1f", 1)
        commits.append({"commit": parts[0], "message": parts[1] if len(parts) > 1 else ""})
    return commits


def is_ancestor(repo_dir: Path, maybe_ancestor: str, descendant: str) -> bool:
    if not maybe_ancestor or not descendant:
        return False
    proc = subprocess.run(
        ["git", "-C", str(repo_dir), "merge-base", "--is-ancestor", maybe_ancestor, descendant],
        capture_output=True,
    )
    return proc.returncode == 0


def load_transcript(path: Path) -> list[dict]:
    events = []
    if not path.exists():
        return events
    for line in path.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return events


def flatten_content(msg: dict) -> list[dict]:
    """Best-effort extraction of a `content` block list from a stream-json
    line, regardless of whether it's nested under `message` (assistant/user
    events) or at the top level."""
    content = msg.get("message", {}).get("content")
    if content is None:
        content = msg.get("content")
    if isinstance(content, str):
        return [{"type": "text", "text": content}]
    if isinstance(content, list):
        return [c for c in content if isinstance(c, dict)]
    return []


def transcript_metrics(events: list[dict]) -> dict:
    """Turn/token/cost figures from the harness's final `result` event, per
    doc/E2E-TESTING.md's "Token efficiency" table. Any field the running
    harness version doesn't emit is left null rather than guessed at."""
    result_events = [e for e in events if e.get("type") == "result"]
    if not result_events:
        return {
            "turns": None, "cost_usd": None,
            "output_tokens": None, "input_fresh_tokens": None,
            "input_cache_read_tokens": None,
        }
    last = result_events[-1]
    usage = last.get("usage") or {}
    input_fresh = None
    if "input_tokens" in usage or "cache_creation_input_tokens" in usage:
        input_fresh = usage.get("input_tokens", 0) + usage.get("cache_creation_input_tokens", 0)
    return {
        "turns": last.get("num_turns"),
        "cost_usd": last.get("total_cost_usd"),
        "output_tokens": usage.get("output_tokens"),
        "input_fresh_tokens": input_fresh,
        "input_cache_read_tokens": usage.get("cache_read_input_tokens"),
    }


def tool_name_matches(name: str, tool: str) -> bool:
    # Tool names on the wire may be bare ("function_lookup") or namespaced
    # by MCP server ("mcp__mnemosyne__function_lookup").
    return name == tool or name.endswith(f"__{tool}")


def find_carryover_reuse(
    events: list[dict],
    session1_functions: set[tuple[str, str]],
    session2_repo: Path,
    session1_head: str,
) -> list[dict]:
    """Walk the session-2 transcript in order, pairing each function_lookup
    tool_use with its tool_result, flagging results that resolve to a
    session-1 function, and checking whether a later clojure_eval call
    invokes that function's bare name."""
    # First pass: collect tool_use calls (id, name, input, position) and
    # tool_result blocks (tool_use_id, text) in transcript order.
    tool_uses: list[dict] = []
    tool_results: dict[str, str] = {}
    for idx, event in enumerate(events):
        for block in flatten_content(event):
            btype = block.get("type")
            if btype == "tool_use":
                tool_uses.append({
                    "index": idx, "id": block.get("id"),
                    "name": block.get("name", ""), "input": block.get("input", {}),
                })
            elif btype == "tool_result":
                text = block.get("content")
                if isinstance(text, list):
                    text = "".join(
                        c.get("text", "") for c in text if isinstance(c, dict)
                    )
                if isinstance(text, str) and block.get("tool_use_id"):
                    tool_results[block["tool_use_id"]] = text

    lookups = [t for t in tool_uses if tool_name_matches(t["name"], "function_lookup")]
    evals = [t for t in tool_uses if tool_name_matches(t["name"], "clojure_eval")]

    hits = []
    for lookup in lookups:
        result_text = tool_results.get(lookup["id"], "")
        if not result_text:
            continue
        matched: tuple[str, str, str | None] | None = None

        for ns, name, commit in EXACT_HEADER_RE.findall(result_text):
            in_s1_set = (ns, name) in session1_functions
            in_s1_history = bool(session1_head) and is_ancestor(session2_repo, commit, session1_head)
            if in_s1_set or in_s1_history:
                matched = (ns, name, commit)
                break

        if matched is None:
            for ns, name in SEARCH_HIT_RE.findall(result_text):
                if (ns, name) in session1_functions:
                    matched = (ns, name, None)
                    break

        if matched is None:
            continue

        ns, name, commit = matched
        reused_eval = next(
            (e for e in evals
             if e["index"] > lookup["index"]
             and re.search(rf"\b{re.escape(name)}\b", e.get("input", {}).get("code", ""))),
            None,
        )
        hits.append({
            "query": lookup["input"].get("query"),
            "matched_namespace": ns,
            "matched_name": name,
            "matched_commit": commit,
            "lookup_event_index": lookup["index"],
            "reused_in_later_eval": reused_eval is not None,
            "reused_eval_event_index": reused_eval["index"] if reused_eval else None,
        })
    return hits


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--session1-dir", required=True, help="results/<run-id> for session 1")
    ap.add_argument("--session2-dir", required=True, help="results/<run-id> for session 2 (carryover)")
    ap.add_argument("--control-dir", required=True, help="results/<run-id> for session 2 (empty-volume control)")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    session1_dir = Path(args.session1_dir)
    session2_dir = Path(args.session2_dir)
    control_dir = Path(args.control_dir)

    report: dict = {
        "session1_dir": str(session1_dir),
        "session2_dir": str(session2_dir),
        "control_dir": str(control_dir),
    }

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        repo1 = extract_tar(session1_dir / "mnemosyne-data.tar", tmp / "s1")
        repo2 = extract_tar(session2_dir / "mnemosyne-data.tar", tmp / "s2")
        repo_control = extract_tar(control_dir / "mnemosyne-data.tar", tmp / "control")

        session1_functions: set[tuple[str, str]] = set()
        session1_head = ""
        session2_new_commits: list[dict] = []
        session2_functions_end: set[tuple[str, str]] = set()
        control_functions_end: set[tuple[str, str]] = set()
        reuse_hits: list[dict] = []

        if repo1 is not None:
            session1_head = git(repo1, "rev-parse", "HEAD")
            session1_functions = functions_at_head(repo1)

        if repo2 is not None:
            session2_functions_end = functions_at_head(repo2)
            session2_new_commits = commits_since(repo2, session1_head) if session1_head else \
                commits_since(repo2, "")

            events2 = load_transcript(session2_dir / "transcript.jsonl")
            reuse_hits = find_carryover_reuse(events2, session1_functions, repo2, session1_head)
        else:
            events2 = load_transcript(session2_dir / "transcript.jsonl")

        if repo_control is not None:
            control_functions_end = functions_at_head(repo_control)

        report["session1_functions_at_end"] = [f"{ns}/{name}" for ns, name in sorted(session1_functions)]
        report["session2_functions_saved_new"] = sorted(
            f"{ns}/{name}" for ns, name in (session2_functions_end - session1_functions)
        )
        report["control_functions_saved_new"] = sorted(
            f"{ns}/{name}" for ns, name in control_functions_end
        )
        report["session2_new_commit_count"] = len(session2_new_commits)
        report["carryover_lookup_hits"] = reuse_hits
        report["carryover_lookup_hit_count"] = len(reuse_hits)
        report["carryover_reused_in_eval_count"] = sum(
            1 for h in reuse_hits if h["reused_in_later_eval"]
        )

    session1_metrics = transcript_metrics(load_transcript(session1_dir / "transcript.jsonl"))
    session2_metrics = transcript_metrics(events2)
    control_metrics = transcript_metrics(load_transcript(control_dir / "transcript.jsonl"))
    report["metrics"] = {
        "session1": session1_metrics,
        "session2_carryover": session2_metrics,
        "session2_control": control_metrics,
    }

    def delta(key: str):
        a, b = session2_metrics.get(key), control_metrics.get(key)
        if isinstance(a, (int, float)) and isinstance(b, (int, float)):
            return a - b
        return None

    report["carryover_vs_control_delta"] = {
        key: delta(key) for key in
        ("turns", "cost_usd", "output_tokens", "input_fresh_tokens", "input_cache_read_tokens")
    }

    Path(args.out).write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
