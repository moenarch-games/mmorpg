#!/usr/bin/env bash
# Explicit opt-in input for #346, never required by ordinary gameplay builds.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
EXPECTED=3e8297d956d9460008e377839d60e7f53293d0b2e077bc3a5efa43d5fdb02704
DEST="${ROOT_DIR}/web/public/review/greyhaven.nature"
if [[ -f "${DEST}/SHA256" && "$(cat "${DEST}/SHA256")" == "${EXPECTED}" ]]; then
  exit 0
fi
WORK="$(mktemp -d)"
trap 'rm -rf -- "${WORK}"' EXIT
curl -fsSL --retry 3 --max-time 300 -o "${WORK}/bundle.tar" \
  https://github.com/moritzbrantner/asset-tooling/releases/download/greyhaven-nature/greyhaven.nature.tar
printf '%s  %s\n' "${EXPECTED}" "${WORK}/bundle.tar" | sha256sum --check --status
mkdir "${WORK}/bundle"
tar -xf "${WORK}/bundle.tar" -C "${WORK}/bundle"
printf '%s\n' "${EXPECTED}" > "${WORK}/bundle/SHA256"
mkdir -p -- "$(dirname -- "${DEST}")"
rm -rf -- "${DEST}"
mv -- "${WORK}/bundle" "${DEST}"
echo "Fetched the pinned Greyhaven nature review input."
