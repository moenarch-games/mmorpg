#!/usr/bin/env bash
# Downloads the asset-tooling bundles the clients draw, once each: the Ranger
# humanoid (Atlas 6.3.7) into web/public/characters/humanoid.ranger.male and the
# Greyhaven inn (#158) into web/public/buildings/greyhaven.inn and the Sunken Shrine kit
# (#431) into web/public/scenery/shrine.kit and the ambient animals (#419) into
# web/public/scenery/greyhaven.ambient-animals and the Outpost building kit (#420) into
# web/public/buildings/outpost.kit and the settlement props (#421) into
# web/public/scenery/settlement.props, where the browser
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

# The rolling shrine-kit release (#431) is pinned by its tar sha256 and the consumer's copy of
# its export.ref.json (asset-tooling docs/asset-bundles.md); a replaced release fails here.
fetch_pinned() {
  local tag="$1" name="$2" dest="${ROOT_DIR}/web/public/$3" sha="$4" ref="${ROOT_DIR}/$5"
  if [[ -f "${dest}/SHA256" && "$(cat "${dest}/SHA256")" == "${sha}" ]]; then
    return 0
  fi
  local work
  work="$(mktemp -d)"
  curl -fsSL --retry 3 --max-time 300 -o "${work}/bundle.tar" \
    "https://github.com/moritzbrantner/asset-tooling/releases/download/${tag}/${name}.tar"
  printf '%s  %s\n' "${sha}" "${work}/bundle.tar" | sha256sum --check --status
  mkdir "${work}/bundle"
  tar -xf "${work}/bundle.tar" -C "${work}/bundle"
  cmp -s "${ref}" "${work}/bundle/export.ref.json"
  printf '%s\n' "${sha}" > "${work}/bundle/SHA256"
  rm -rf -- "${dest}"
  mkdir -p -- "$(dirname -- "${dest}")"
  mv -- "${work}/bundle" "${dest}"
  rm -rf -- "${work}"
  echo "Fetched ${tag} into ${dest}"
}

python3 "${ROOT_DIR}/scripts/fetch-ranger.py"
fetch greyhaven-inn-v1 greyhaven.inn buildings/greyhaven.inn
fetch_pinned shrine-kit shrine.kit scenery/shrine.kit \
  a1938eb814e570cec9ad4c50fde54b62d260c40d80d9416eed0961e8572255db web/assets/shrine-kit/export.ref.json
# Greyhaven's cosmetic cat, dog and rodent (#419).
fetch_pinned greyhaven-ambient-animals greyhaven.ambient-animals scenery/greyhaven.ambient-animals \
  03793700a4d1a15314aa89c3f3535fbe5c4f7017a748caf6f32f3946ed2b5df9 web/assets/greyhaven-ambient-animals/export.ref.json
# The Outpost building kit (#420): the smithy and one house.
fetch_pinned outpost-kit outpost.kit buildings/outpost.kit \
  a510c90c598b6e3410478b33610d001f0f667c5bdc45375b23c3e206eff1dc4a web/assets/outpost-kit/export.ref.json
# Greyhaven's reusable settlement props (#421): the market square before house 106.
fetch_pinned settlement-props settlement.props scenery/settlement.props \
  a4fbf22d6727da7c218ee9cfe2c574e716ace53245a19641710afc62406ffcd4 web/assets/settlement-props/export.ref.json
