#!/usr/bin/env bash
# Build the native host and client with the optimised release profile and play once.
# Same local zone, certificate and ports as dev-native.sh, without the source watch.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT_DIR
cd -- "$ROOT_DIR"
exec python3 "$ROOT_DIR/scripts/lib/dev-native.py" --release --no-watch "$@"
