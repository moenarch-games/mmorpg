#!/usr/bin/env bash
# Writes the Actions cache `key` and `paths` of the asset-tooling downloads to $GITHUB_OUTPUT. The
# key follows every live release's identity and the scripts that select what is fetched. When a
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
    cat - "${ROOT_DIR}/scripts/fetch-bundles.sh" "${ROOT_DIR}/scripts/fetch-ranger.py" "${ROOT_DIR}/scripts/lib/ranger_assets.py" |
    sha256sum | cut -d' ' -f1
)"
{
  printf 'key=mmorpg-assets-%s\n' "${digest}"
  echo 'paths<<PATHS'
  # Only native jobs generate native/; a Pages build from a warm cache must not ship it. The
  # figures use /* because actions/cache tars a listed directory whole, ignoring exclusions below it.
  printf '%s\n' "${identity}" |
    awk '$3 ~ /\/humanoid\.ranger\./ { print $3 "/*"; print "!" $3 "/native"; next } { print $3 }'
  echo 'PATHS'
} >> "${GITHUB_OUTPUT:-/dev/stdout}"
