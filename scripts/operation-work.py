#!/usr/bin/env python3
"""Emits `coding-tooling/work-evidence/v1` for `.performance/contract.json` (#138).

Runs the deterministic `operation_work` example and wraps its samples with the
contract's suite and SHA-256. Counters only; nothing is timed.
"""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    contract_bytes = (ROOT / ".performance" / "contract.json").read_bytes()
    contract = json.loads(contract_bytes)
    run = subprocess.run(
        ["cargo", "run", "-q", "--locked", "--release", "-p", "mmorpg-protocol", "--example", "operation_work"],
        cwd=ROOT, check=True, capture_output=True, text=True, timeout=1800,
    )
    samples = json.loads(run.stdout)
    evidence = {
        "schemaVersion": "coding-tooling/work-evidence/v1",
        "suite": contract["suite"],
        "contractSha256": hashlib.sha256(contract_bytes).hexdigest(),
        "operations": samples["operations"],
    }
    json.dump(evidence, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
