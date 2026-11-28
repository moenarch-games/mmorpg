#!/usr/bin/env python3
"""Runtime budgets of named scenarios (#1256, Atlas 7.1.i1256), from `.performance/runtime.json`.

Each budgeted metric has an absolute ceiling (`max`) or floor (`min`): crossing it is an error and fails
the run. Against a baseline (main's latest values) a metric worse than the section's `warnRatio` only
warns: a GitHub `::warning::` annotation under Actions, a printed warning otherwise. Timings decide
these budgets only; correctness never depends on them.

    python3 scripts/runtime_budget.py native [--baseline FILE] [--out FILE]

runs the native zone scenarios (`mmorpg-protocol`'s `runtime_scenarios` example, release) and checks
them; `--out` writes the measured values, the baseline format. `scripts/profile-browser.py --budgets`
checks the `browser` section against a GPU profile through `check` below.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUDGETS = ROOT / ".performance" / "runtime.json"
VALUES_SCHEMA = "mmorpg.runtime-values/v1"


def metric_value(scenario: dict, path: str):
    """The number at the dotted `path` in a scenario's measurements, or None."""
    value = scenario
    for key in path.split("."):
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def values(section: dict, scenarios: dict) -> dict:
    """The budgeted metrics' measured values, `{scenario: {metric: value}}` (the baseline format's `values`)."""
    return {
        name: {budget["metric"]: metric_value(scenarios.get(name, {}), budget["metric"]) for budget in budgets}
        for name, budgets in section["scenarios"].items()
    }


def annotate(level: str, message: str) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level}::{message}")
    else:
        print(f"{level}: {message}", file=sys.stderr)


def check(budgets: dict, section_name: str, scenarios: dict, baseline: dict | None) -> dict:
    """Checks `scenarios` against the `section_name` budgets and, when given, a baseline's values; prints an
    annotation per finding. Returns `{"values", "errors", "warnings"}`."""
    section = budgets["sections"][section_name]
    ratio = section.get("warnRatio", budgets["warnRatio"])
    measured = values(section, scenarios)
    previous = (baseline or {}).get("values", {})
    errors, warnings = [], []
    for name, budgets_of in section["scenarios"].items():
        for budget in budgets_of:
            metric = budget["metric"]
            value = measured[name][metric]
            label = f"{section_name} {name} {metric}"
            if value is None:
                errors.append(f"{label}: not measured")
                continue
            if "max" in budget and value > budget["max"]:
                errors.append(f"{label} = {value:g}, above its ceiling {budget['max']:g}")
            if "min" in budget and value < budget["min"]:
                errors.append(f"{label} = {value:g}, below its floor {budget['min']:g}")
            before = previous.get(name, {}).get(metric)
            if not isinstance(before, (int, float)) or before <= 0:
                continue
            worse = value > before * ratio if "max" in budget else value < before / ratio
            if worse:
                warnings.append(f"{label} = {value:g}, {value / before:.2f}x main's {before:g} (warns beyond {ratio:g}x)")
    for message in warnings:
        annotate("warning", f"Runtime regression: {message}")
    for message in errors:
        annotate("error", f"Runtime budget: {message}")
    if baseline is None:
        print(f"No {section_name} baseline: ceilings checked, no regression comparison.", file=sys.stderr)
    return {"values": measured, "errors": errors, "warnings": warnings}


def load_baseline(path: Path | None) -> dict | None:
    if path is None or not path.is_file():
        return None
    baseline = json.loads(path.read_text())
    return baseline if baseline.get("schema") == VALUES_SCHEMA else None


def values_document(section_name: str, result: dict, **extra) -> dict:
    return {"schema": VALUES_SCHEMA, "section": section_name, **extra, "values": result["values"]}


def native(args) -> int:
    budgets = json.loads(args.budgets.read_text())
    section = budgets["sections"]["native"]
    run = subprocess.run(section["command"], cwd=ROOT, check=True, capture_output=True, text=True, timeout=3600)
    scenarios = json.loads(run.stdout)["scenarios"]
    result = check(budgets, "native", scenarios, load_baseline(args.baseline))
    print(json.dumps(result, indent=2))
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(values_document("native", result, commit=os.environ.get("GITHUB_SHA")), indent=2) + "\n")
    return 1 if result["errors"] else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("section", choices=["native"], help="the budget section to measure and check")
    parser.add_argument("--budgets", type=Path, default=BUDGETS)
    parser.add_argument("--baseline", type=Path, help="main's values to warn against; a missing file only skips the comparison")
    parser.add_argument("--out", type=Path, help="write the measured values here (the baseline format)")
    return native(parser.parse_args())


if __name__ == "__main__":
    sys.exit(main())
