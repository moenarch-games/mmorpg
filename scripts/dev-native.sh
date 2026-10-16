#!/usr/bin/env bash
# Refresh and supervise the local native multiplayer environment.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT_DIR
cd -- "$ROOT_DIR"
exec python3 "$ROOT_DIR/scripts/lib/dev-native.py" "$@"
