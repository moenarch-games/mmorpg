#!/usr/bin/env python3
"""Emits `coding-tooling/work-evidence/v1` for `.performance/contract.json` (#138, #645, #855, #1009, #1057, #1058, #1086, #1120, #1141, #1148, #1149).

Runs the deterministic zone (`mmorpg-protocol`, `mmorpg-core`, `sim-foundation`) and client presentation
(`mmorpg-presentation`, `mmorpg-wasm`) work examples and the browser operations' Bun entry
point (`web/scripts/work-evidence.ts`), and wraps their samples with the contract's suite and
SHA-256. Counters only; nothing is timed.
"""
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLES = [
    ("mmorpg-protocol", "operation_work"),
    ("mmorpg-core", "creature_tick_work"),
    ("sim-foundation", "grounded_work"),
    ("mmorpg-game-server", "claim_expiry_work"),
    ("sim-foundation", "mover_pairs_work"),
    ("mmorpg-presentation", "presentation_work"),
    ("mmorpg-wasm", "projection_ingest"),
    ("mmorpg-wasm", "unit_layout"),
    ("mmorpg-wasm", "seeded_content"),
]


def merge(operations: list, incoming: list) -> None:
    """Adds `incoming` operations; one already present (`unit-frame`, from `presentation_work` and
    `unit_layout`) gains their metrics in its sample of equal dimensions."""
    by_id = {operation["id"]: operation for operation in operations}
    for operation in incoming:
        known = by_id.get(operation["id"])
        if known is None:
            operations.append(operation)
            by_id[operation["id"]] = operation
            continue
        samples = {json.dumps(sample["dimensions"], sort_keys=True): sample for sample in known["samples"]}
        for sample in operation["samples"]:
            match = samples.get(json.dumps(sample["dimensions"], sort_keys=True))
            if match is None:
                known["samples"].append(sample)
            else:
                match["metrics"].update(sample["metrics"])


def browser_operations() -> list:
    """The browser operations' samples (#1120): `web/scripts/work-evidence.ts` measures them through the same helpers
    their bun tests assert through. It builds the release `mmorpg-wasm` module first and writes its JSON to a file, so
    build output on stdout cannot corrupt it. Needs `bun install` in web/ and the fetched asset bundles."""
    with tempfile.TemporaryDirectory() as directory:
        out = Path(directory) / "browser-work.json"
        subprocess.run(
            ["bun", "scripts/work-evidence.ts", str(out)],
            cwd=ROOT / "web", check=True, stdout=sys.stderr, timeout=1800,
        )
        return json.loads(out.read_text())["operations"]


def main() -> int:
    contract_bytes = (ROOT / ".performance" / "contract.json").read_bytes()
    contract = json.loads(contract_bytes)
    operations = []
    for package, example in EXAMPLES:
        run = subprocess.run(
            ["cargo", "run", "-q", "--locked", "--release", "-p", package, "--example", example],
            cwd=ROOT, check=True, capture_output=True, text=True, timeout=1800,
        )
        merge(operations, json.loads(run.stdout)["operations"])
    merge(operations, browser_operations())
    evidence = {
        "schemaVersion": "coding-tooling/work-evidence/v1",
        "suite": contract["suite"],
        "contractSha256": hashlib.sha256(contract_bytes).hexdigest(),
        "operations": operations,
    }
    json.dump(evidence, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
