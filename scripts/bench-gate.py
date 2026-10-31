#!/usr/bin/env python3
"""Fails when any criterion benchmark regressed by more than the threshold (default 10%).

Run after `cargo bench -- --baseline <name>`, which makes criterion write a
change/estimates.json per benchmark comparing this run with the saved baseline.

    python3 scripts/bench-gate.py [--threshold 0.10] [--criterion-dir services/gateway/target/criterion]
"""

import argparse
import json
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--threshold", type=float, default=0.10)
    parser.add_argument("--criterion-dir", default="services/gateway/target/criterion")
    args = parser.parse_args()

    root = Path(args.criterion_dir)
    changes = sorted(root.rglob("change/estimates.json"))
    if not changes:
        print(f"no comparisons under {root}; run `cargo bench -- --baseline <name>` first")
        return 1

    failed = False
    for path in changes:
        bench = path.parent.parent.relative_to(root).as_posix()
        mean = json.loads(path.read_text())["mean"]
        change = mean["point_estimate"]
        # Only fail when the whole confidence interval is past the threshold: noise isn't a regression.
        lower = mean["confidence_interval"]["lower_bound"]
        verdict = "REGRESSED" if lower > args.threshold else "ok"
        failed |= verdict == "REGRESSED"
        print(f"{verdict:>9}  {change:+7.1%}  {bench}")
    if failed:
        print(f"\nAt least one benchmark is more than {args.threshold:.0%} slower than the baseline.")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
