#!/usr/bin/env bash
# Explicit opt-in input for #346, never required by ordinary gameplay builds.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
EXPECTED=bd7799b47478d1bef1d864f96f51f3cb34b03a7497ba5d5429b2099834f6311b
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
