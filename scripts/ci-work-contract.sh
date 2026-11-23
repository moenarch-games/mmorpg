#!/usr/bin/env bash
# Full validation's work-complexity contract check (#1120): `coding-tooling performance work` validates
# .performance/contract.json, runs `performance:work` from .coding-tooling.json (scripts/operation-work.py:
# the Rust work examples and web/scripts/work-evidence.ts) and fails on an invalid contract, missing
# evidence or a crossed bound. Needs the fetched asset bundles, Bun with `bun install` done in web/, and
# the wasm-bindgen CLI (the browser measurements build the release WASM module).
#
# `scripts/ci-work-contract.sh structure` (#1132) only validates the contract's structure with
# `coding-tooling performance applicability`: no builds and no evidence, so it needs Bun alone. CI runs it
# on every change to .performance/.
set -euo pipefail

MODE="${1:-work}"

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# moritzbrantner/coding-tooling at the head of main (owner decision 2026-11-23, #1126: follow main).
CODING_TOOLING_REF=main

work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
git -C "${work}" init -q
git -C "${work}" fetch -q --depth 1 https://github.com/moritzbrantner/coding-tooling.git "${CODING_TOOLING_REF}"
git -C "${work}" checkout -q FETCH_HEAD
(cd "${work}" && bun install --frozen-lockfile)

if [[ "${MODE}" == structure ]]; then
  bun "${work}/src/router.ts" performance applicability --root "${ROOT_DIR}"
  echo "Work-complexity contract structure is valid."
  exit 0
fi

start=${SECONDS}
bun "${work}/src/router.ts" performance work --root "${ROOT_DIR}"
echo "Work-complexity contract checked in $(( SECONDS - start )) s."
