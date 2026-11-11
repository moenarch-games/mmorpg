#!/usr/bin/env python3
"""Emits `coding-tooling/work-evidence/v1` for `.performance/contract.json` (#138, #645).

Runs the deterministic zone (`mmorpg-protocol`) and client presentation
(`mmorpg-presentation`) work examples and wraps their samples with the
contract's suite and SHA-256. Counters only; nothing is timed.
"""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLES = [("mmorpg-protocol", "operation_work"), ("mmorpg-presentation", "presentation_work")]


def main() -> int:
    contract_bytes = (ROOT / ".performance" / "contract.json").read_bytes()
    contract = json.loads(contract_bytes)
    operations = []
    for package, example in EXAMPLES:
        run = subprocess.run(
            ["cargo", "run", "-q", "--locked", "--release", "-p", package, "--example", example],
            cwd=ROOT, check=True, capture_output=True, text=True, timeout=1800,
        )
        operations.extend(json.loads(run.stdout)["operations"])
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
