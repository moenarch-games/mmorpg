#!/usr/bin/env bash
# Re-records every committed golden fixture (#487) by running only the tests that own one with
# MMORPG_UPDATE_FIXTURES=1, then prints what changed. Minutes instead of the whole workspace;
# useful after a rebase onto a wire or scenery change. Review the diff like any other change.
# A wire layout change still needs scripts/new-protocol-id.sh first (ADR 0006).
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"
export MMORPG_UPDATE_FIXTURES=1

run() {
  cargo nextest run --locked --no-fail-fast "$@"
}

# Command, snapshot, loot and observation wire bytes.
run -p mmorpg-protocol
# Pinned game-server session frames.
run -p mmorpg-game-server --test session_frames
# Presentation gameplay trace (determinism oracle).
run -p mmorpg-core --test presentation_gameplay
# Bot and control-plane scenario outputs.
run -p mmorpg-scenarios --test scenarios
# Scenery stable hash, export fingerprint and the generated Action Lab baseline.
run -p mmorpg-scenery -p mmorpg-wasm

git status --short -- fixtures crates
git diff --stat -- fixtures crates
