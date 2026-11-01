#!/usr/bin/env bash
# Downloads the asset-tooling bundles the clients draw: the Ranger
# humanoid (Atlas 6.3.7) into web/public/characters/humanoid.ranger.male and the
# Greyhaven inn (#158) into web/public/buildings/greyhaven.inn and the Sunken Shrine kit
# (#431) into web/public/scenery/shrine.kit and the ambient animals (#419) into
# web/public/scenery/greyhaven.ambient-animals and the Outpost building kit (#420) into
# web/public/buildings/outpost.kit and the settlement props (#421) into
# web/public/scenery/settlement.props and Elden's house (#158) into
# web/public/buildings/greyhaven.marshal-house and the CC0 combat sounds (#447) into
# web/public/sounds/combat-sounds and Greyhaven's trees and rocks (#49) into
# web/public/scenery/greyhaven.nature and Greyhaven's ground materials (#525) into
# web/public/scenery/greyhaven.terrain and the climbing kit (#621) into web/public/scenery/climbing.kit, where the
# browser serves them and the native client build embeds them,
# and the Battle Royale fortress kit (#591) into web/public/scenery/fortress.kit for the browser's arena.
# The directories are ignored build input, never committed.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASES="https://github.com/moritzbrantner/asset-tooling/releases/download"

# Unpacks release `tag`'s `name`.tar into `work`/bundle.
unpack() {
  local tag="$1" name="$2" work="$3"
  curl -fsSL --retry 3 --max-time 300 -o "${work}/bundle.tar" "${RELEASES}/${tag}/${name}.tar"
  mkdir "${work}/bundle"
  tar -xf "${work}/bundle.tar" -C "${work}/bundle"
  printf '%s\n' "${tag}" > "${work}/bundle/RELEASE"
}

# Swaps the unpacked bundle in for `dest`.
place() {
  local tag="$1" dest="$2" work="$3"
  rm -rf -- "${dest}"
  mkdir -p -- "$(dirname -- "${dest}")"
  mv -- "${work}/bundle" "${dest}"
  rm -rf -- "${work}"
  echo "Fetched ${tag} into ${dest}"
}

# An immutable, versioned release: an existing download of the same release is kept.
fetch() {
  local tag="$1" name="$2" dest="${ROOT_DIR}/web/public/$3"
  if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]]; then
    return 0
  fi
  local work
  work="$(mktemp -d)"
  unpack "${tag}" "${name}" "${work}"
  place "${tag}" "${dest}" "${work}"
}

# A rolling release pinned by the SHA-256 of its tar: a re-upload is adopted by updating the pin.
fetch_pinned() {
  local tag="$1" name="$2" dest="${ROOT_DIR}/web/public/$3" sha="$4"
  if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}@${sha}" ]]; then
    return 0
  fi
  local work
  work="$(mktemp -d)"
  curl -fsSL --retry 3 --max-time 300 -o "${work}/bundle.tar" "${RELEASES}/${tag}/${name}.tar"
  printf '%s  %s\n' "${sha}" "${work}/bundle.tar" | sha256sum --check --status
  mkdir "${work}/bundle"
  tar -xf "${work}/bundle.tar" -C "${work}/bundle"
  printf '%s\n' "${tag}@${sha}" > "${work}/bundle/RELEASE"
  place "${tag}" "${dest}" "${work}"
}

# A rolling release is read at whatever it currently holds, unpinned (#507): the local copy is
# keyed on the live release's export.ref.json, so a new upload refreshes it and an unchanged
# release is not downloaded again. Offline, an existing copy of the release is kept.
fetch_rolling() {
  local tag="$1" name="$2" dest="${ROOT_DIR}/web/public/$3"
  local work
  work="$(mktemp -d)"
  if ! curl -fsSL --retry 3 --max-time 60 -o "${work}/export.ref.json" "${RELEASES}/${tag}/export.ref.json"; then
    if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]]; then
      echo "Could not reach the ${tag} release; keeping ${dest}" >&2
      rm -rf -- "${work}"
      return 0
    fi
    echo "Could not download ${tag}/export.ref.json" >&2
    rm -rf -- "${work}"
    return 1
  fi
  if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]] &&
    cmp -s "${work}/export.ref.json" "${dest}/export.ref.json"; then
    rm -rf -- "${work}"
    return 0
  fi
  unpack "${tag}" "${name}" "${work}"
  cp -- "${work}/export.ref.json" "${work}/bundle/export.ref.json"
  place "${tag}" "${dest}" "${work}"
}

python3 "${ROOT_DIR}/scripts/fetch-ranger.py"
fetch greyhaven-inn-v1 greyhaven.inn buildings/greyhaven.inn
fetch_rolling shrine-kit shrine.kit scenery/shrine.kit
# Greyhaven's cosmetic cat, dog and rodent (#419).
fetch_rolling greyhaven-ambient-animals greyhaven.ambient-animals scenery/greyhaven.ambient-animals
# The Outpost building kit (#420): the smithy and one house.
fetch_rolling outpost-kit outpost.kit buildings/outpost.kit
# Greyhaven's reusable settlement props (#421): the market square before house 106.
fetch_rolling settlement-props settlement.props scenery/settlement.props
# Marshal Elden Greywatch's furnished private home on house 103 (#158, asset-tooling#413).
fetch_rolling greyhaven-marshal-house greyhaven.marshal-house buildings/greyhaven.marshal-house
# The CC0 combat sounds both clients play (#447, asset-tooling#297).
fetch_rolling combat-sounds combat-sounds sounds/combat-sounds
# Greyhaven's CC0 ground materials: grass, forest floor, scree, mud, sand, paths and rock (#525).
fetch_rolling greyhaven-terrain greyhaven.terrain scenery/greyhaven.terrain
# Greyhaven's ladders, climbable ropes and climbing ivy with their LOD chains (#621).
fetch_rolling climbing-kit climbing.kit scenery/climbing.kit
# Greyhaven's trees and rocks with their LOD chains and rock collision proxies (#49), pinned
# (asset-tooling's consumers.json re-pins this line after a re-upload).
NATURE_TAR_SHA256=c0d578faec0b1286ab243241fb69f764141f4c9e72c88ccd86326fa0b415f44e
fetch_pinned greyhaven-nature greyhaven.nature scenery/greyhaven.nature "${NATURE_TAR_SHA256}"
# The Battle Royale fortress's authored art (#591, asset-tooling#458/#461, pass 2), fitted to its
# collision, pinned like the nature bundle.
FORTRESS_TAR_SHA256=58185d2310b713642cf881212457dcf5c1be11976a5af8e5363cf868792cc241
fetch_pinned fortress-kit fortress.kit scenery/fortress.kit "${FORTRESS_TAR_SHA256}"
