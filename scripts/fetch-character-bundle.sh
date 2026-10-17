#!/usr/bin/env bash
# Downloads the asset-tooling Ranger humanoid bundle (Atlas 6.3.7) once into
# web/public/characters/humanoid.ranger.male, where the browser serves it and
# the native client build embeds it. Idempotent: an existing download of the
# same release is kept. The directory is ignored build input, never committed.
set -euo pipefail

readonly TAG="humanoid-ranger-male-v5"
readonly URL="https://github.com/moritzbrantner/asset-tooling/releases/download/${TAG}/humanoid.ranger.male.tar"
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly DEST="${ROOT_DIR}/web/public/characters/humanoid.ranger.male"

if [[ -f "${DEST}/RELEASE" && "$(cat "${DEST}/RELEASE")" == "${TAG}" ]]; then
  exit 0
fi
work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
curl -fsSL --retry 3 --max-time 300 -o "${work}/bundle.tar" "${URL}"
mkdir "${work}/bundle"
tar -xf "${work}/bundle.tar" -C "${work}/bundle"
printf '%s\n' "${TAG}" > "${work}/bundle/RELEASE"
rm -rf -- "${DEST}"
mkdir -p -- "$(dirname -- "${DEST}")"
mv -- "${work}/bundle" "${DEST}"
echo "Fetched ${TAG} into ${DEST}"
