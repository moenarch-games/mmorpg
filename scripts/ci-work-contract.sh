#!/usr/bin/env bash
# Full validation's work-complexity contract check (#1120): `coding-tooling performance work` validates
# .performance/contract.json, runs `performance:work` from .coding-tooling.json (scripts/operation-work.py:
# the Rust work examples and web/scripts/work-evidence.ts) and fails on an invalid contract, missing
# evidence or a crossed bound. Needs the fetched asset bundles, Bun with `bun install` done in web/, and
# the wasm-bindgen CLI (the browser measurements build the release WASM module).
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# moritzbrantner/coding-tooling main, pinned by commit; bump it intentionally.
CODING_TOOLING_REV=aba13db56c5e612fa82b1fbc6a2577ba5fb72dcd

work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
git -C "${work}" init -q
git -C "${work}" fetch -q --depth 1 https://github.com/moritzbrantner/coding-tooling.git "${CODING_TOOLING_REV}"
git -C "${work}" checkout -q FETCH_HEAD
(cd "${work}" && bun install --frozen-lockfile)

start=${SECONDS}
bun "${work}/src/router.ts" performance work --root "${ROOT_DIR}"
echo "Work-complexity contract checked in $(( SECONDS - start )) s."
