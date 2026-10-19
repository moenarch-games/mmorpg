#!/usr/bin/env bash
# Optional development-only Blender authoring, never used by game hosts or CI.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
layout="$root/content/greyhaven/outpost-layout.json"
project="$root/artifacts/greyhaven/outpost.blend"
context="$root/artifacts/greyhaven/outpost-context.json"
bridge="$root/scripts/blender/greyhaven_layout.py"
blender="${BLENDER_BIN:-blender}"

if ! command -v "$blender" >/dev/null 2>&1; then
  echo "Blender 5.2.2 LTS is a development-only prerequisite. Install from https://www.blender.org/download/ or set BLENDER_BIN." >&2
  exit 1
fi
version="$("$blender" --version)"
first_line="${version%%$'\n'*}"
if [[ "$first_line" != "Blender 5.2.2" ]]; then
  echo "Expected Blender 5.2.2 LTS; found: $first_line" >&2
  exit 1
fi

case "${1:-open}" in
  open)
    mkdir -p -- "$(dirname -- "$project")"
    if [[ -f "$project" ]]; then
      exec "$blender" "$project"
    fi
    (
      cd -- "$root"
      cargo run --quiet --locked -p mmorpg-scenery --example export-blender-layout > "$context"
    )
    exec "$blender" --python-exit-code 1 --python "$bridge" -- \
      import --layout "$layout" --context "$context" --blend "$project"
    ;;
  export)
    if [[ ! -f "$project" ]]; then
      echo "No saved Outpost Blender project. Run: bash scripts/dev-blender.sh open" >&2
      exit 1
    fi
    exec "$blender" --background "$project" --python-exit-code 1 --python "$bridge" -- \
      export --layout "$layout" --root "$root"
    ;;
  *)
    echo "Usage: bash scripts/dev-blender.sh [open|export]" >&2
    exit 2
    ;;
esac
