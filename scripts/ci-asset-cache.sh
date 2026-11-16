#!/usr/bin/env bash
# Writes the Actions cache `key` and `paths` of the asset-tooling downloads to $GITHUB_OUTPUT. The
# key follows every live release's identity and the script that fetches them. When a
# release cannot be reached it writes neither, so the job runs without the cache. A restored copy
# is only an optimisation: fetch-bundles.sh still compares every rolling release with the live one.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if ! identity="$("${ROOT_DIR}/scripts/fetch-bundles.sh" --identity)"; then
  echo '::warning::Could not read the asset release identities; running without the asset cache.'
  exit 0
fi
digest="$(
  printf '%s\n' "${identity}" |
    cat - "${ROOT_DIR}/scripts/fetch-bundles.sh" |
    sha256sum | cut -d' ' -f1
)"
{
  printf 'key=mmorpg-assets-%s\n' "${digest}"
  echo 'paths<<PATHS'
  printf '%s\n' "${identity}" | awk '{ print $3 }'
  echo 'PATHS'
} >> "${GITHUB_OUTPUT:-/dev/stdout}"
