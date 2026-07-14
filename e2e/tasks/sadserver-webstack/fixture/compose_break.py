#!/usr/bin/env python3
"""Host-side: pick 2-3 faults from ../grader/fault_catalog.py by seed and
write fixture/break.sh containing only their shell bodies, concatenated,
with no names or descriptions -- so the image and the agent inside it never
see the catalog, only the resulting breakage. See fixture/prepare.sh.
"""
import argparse
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "grader"))
from fault_catalog import FAULT_CATALOG, FAULT_NAMES  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--out", required=True, help="path to write break.sh")
    args = ap.parse_args()

    rng = random.Random(args.seed)
    n = rng.randint(2, 3)
    chosen = rng.sample(FAULT_NAMES, n)

    lines = ["#!/usr/bin/env bash", "set -e", ""]
    for name in chosen:
        lines.append(FAULT_CATALOG[name].strip())
        lines.append("")
    Path(args.out).write_text("\n".join(lines) + "\n")
    Path(args.out).chmod(0o755)


if __name__ == "__main__":
    main()
