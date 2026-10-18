#!/usr/bin/env python3
"""Fail when a criterion bench got slower than its saved baseline.

Run after `cargo bench ... -- --baseline <name>`: criterion leaves each
bench's comparison in target/criterion/<group>/<bench>/change/estimates.json.
A bench fails the gate when its mean got slower by more than the budget and
the whole 95% interval is past it, so noise alone doesn't trip it.

Usage: check.py [criterion dir] [budget, default 0.05]
"""
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "target/criterion")
budget = float(sys.argv[2]) if len(sys.argv) > 2 else 0.05
slower, checked = [], 0
for f in sorted(root.glob("**/change/estimates.json")):
    mean = json.loads(f.read_text())["mean"]
    name = str(f.parent.parent.relative_to(root))
    change, low = mean["point_estimate"], mean["confidence_interval"]["lower_bound"]
    checked += 1
    mark = ""
    if change > budget and low > budget:
        slower.append(name)
        mark = "  <- over budget"
    print(f"{change * 100:+6.1f}%  (95% from {low * 100:+.1f}%)  {name}{mark}")
if checked == 0:
    sys.exit(f"no comparisons under {root}: run the benches with --baseline first")
if slower:
    sys.exit(f"{len(slower)} of {checked} benches slower than {budget * 100:.0f}%: justify them in the commit, or fix them")
print(f"all {checked} benches within {budget * 100:.0f}%")
