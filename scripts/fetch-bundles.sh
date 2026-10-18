#!/usr/bin/env bash
# Downloads the asset-tooling bundles the clients draw, once each: the Ranger
# humanoid (Atlas 6.3.7) into web/public/characters/humanoid.ranger.male and the
# Greyhaven inn (#158) into web/public/buildings/greyhaven.inn, where the browser
# serves them and the native client build embeds them. Idempotent: an existing
# download of the same release is kept. The directories are ignored build input,
# never committed.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

fetch() {
  local tag="$1" name="$2" dest="${ROOT_DIR}/web/public/$3"
  if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]]; then
    return 0
  fi
  local work
  work="$(mktemp -d)"
  curl -fsSL --retry 3 --max-time 300 -o "${work}/bundle.tar" \
    "https://github.com/moritzbrantner/asset-tooling/releases/download/${tag}/${name}.tar"
  mkdir "${work}/bundle"
  tar -xf "${work}/bundle.tar" -C "${work}/bundle"
  printf '%s\n' "${tag}" > "${work}/bundle/RELEASE"
  rm -rf -- "${dest}"
  mkdir -p -- "$(dirname -- "${dest}")"
  mv -- "${work}/bundle" "${dest}"
  rm -rf -- "${work}"
  echo "Fetched ${tag} into ${dest}"
}

fetch humanoid-ranger-male-v7 humanoid.ranger.male characters/humanoid.ranger.male
fetch greyhaven-inn-v1 greyhaven.inn buildings/greyhaven.inn
