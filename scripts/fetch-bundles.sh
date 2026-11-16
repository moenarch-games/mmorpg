#!/usr/bin/env bash
# Downloads the asset-tooling bundles the clients draw: the Ranger figures (Atlas 6.3.7) and
# their equipment into web/public/characters/humanoid.ranger.{male,female} and
# web/public/characters/humanoid.equipment, the Redbrand faction
# kit (#675) into web/public/characters/redbrand.kit, the hair and beard kit (#677) into
# web/public/characters/hair.kit and the
# Greyhaven inn (#158) into web/public/buildings/greyhaven.inn and the Sunken Shrine kit
# (#431) into web/public/scenery/shrine.kit and the ambient animals (#419) into
# web/public/scenery/greyhaven.ambient-animals and the farm animals (#849) into
# web/public/scenery/greyhaven.farm-animals and the wildlife (#857) into
# web/public/scenery/greyhaven.wildlife and the Outpost building kit (#420) into
# web/public/buildings/outpost.kit and the settlement props (#421) into
# web/public/scenery/settlement.props and Elden's house (#158) into
# web/public/buildings/greyhaven.marshal-house and the CC0 combat sounds (#447) into
# web/public/sounds/combat-sounds and Greyhaven's trees and rocks (#49) into
# web/public/scenery/greyhaven.nature and Greyhaven's ground materials (#525) into
# web/public/scenery/greyhaven.terrain and the climbing kit (#621) into web/public/scenery/climbing.kit and the
# crag kit (#668) into web/public/scenery/crag.kit, where the
# browser serves them and the native client build embeds them,
# and the Battle Royale fortress kit (#591) into web/public/scenery/fortress.kit for the browser's arena.
# The directories are ignored build input, never committed.
# With --identity it acquires nothing and prints one "<release> <identity> <path>" line per
# release, for scripts/ci-asset-cache.sh.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASES="https://github.com/moritzbrantner/asset-tooling/releases/download"
IDENTITY="${1:-}"
case "${IDENTITY}" in
  "" | --identity) ;;
  *)
    echo "usage: $0 [--identity]" >&2
    exit 2
    ;;
esac
# curl's --retry covers 408, 429, 5xx and timeouts; no other 4xx is retried.
CURL_RETRY=(--retry 5 --retry-delay 5 --retry-connrefused)
# shellcheck source=scripts/lib/github-release.sh
source "${ROOT_DIR}/scripts/lib/github-release.sh"
github_release_report_mode

# Downloads release `tag`'s asset `file` to `out`: through the REST API with a token, and through
# the public URL without one or when the API fails, for example on a rate limit.
download_asset() {
  local tag="$1" file="$2" out="$3" max_time="$4" work="$5"
  if [[ -n "${GITHUB_RELEASE_TOKEN}" ]]; then
    if [[ -s "${work}/release.json" ]] ||
      github_release_json "moritzbrantner/asset-tooling/releases/tags/${tag}" "${work}/release.json"; then
      if github_release_asset moritzbrantner/asset-tooling "${work}/release.json" "${file}" "${out}" "${max_time}"; then
        return 0
      fi
    fi
    echo "Authenticated download of ${tag}/${file} failed; trying the public URL." >&2
  fi
  curl -fsSL "${CURL_RETRY[@]}" --max-time "${max_time}" -o "${out}" "${RELEASES}/${tag}/${file}"
}

# Unpacks release `tag`'s `name`.tar into `work`/bundle.
unpack() {
  local tag="$1" name="$2" work="$3"
  download_asset "${tag}" "${name}.tar" "${work}/bundle.tar" 300 "${work}"
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
  if [[ -n "${IDENTITY}" ]]; then
    printf '%s immutable web/public/%s\n' "${tag}" "$3"
    return 0
  fi
  if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]]; then
    return 0
  fi
  local work
  work="$(mktemp -d)"
  unpack "${tag}" "${name}" "${work}"
  place "${tag}" "${dest}" "${work}"
}

# A rolling release is read at whatever it currently holds, unpinned (#507): the local copy is
# keyed on the live release's export.ref.json, so a new upload refreshes it and an unchanged
# release is not downloaded again. Offline, an existing copy of the release is kept, except in
# CI, where it may be a restored cache of an older upload.
fetch_rolling() {
  local tag="$1" name="$2" dest="${ROOT_DIR}/web/public/$3"
  local work
  work="$(mktemp -d)"
  if ! download_asset "${tag}" export.ref.json "${work}/export.ref.json" 60 "${work}"; then
    if [[ -z "${IDENTITY}" && "${GITHUB_ACTIONS:-}" != true && -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]]; then
      echo "Could not reach the ${tag} release; keeping ${dest}" >&2
      rm -rf -- "${work}"
      return 0
    fi
    echo "Could not download ${tag}/export.ref.json" >&2
    rm -rf -- "${work}"
    return 1
  fi
  if [[ -n "${IDENTITY}" ]]; then
    printf '%s %s web/public/%s\n' "${tag}" "$(sha256sum "${work}/export.ref.json" | cut -d' ' -f1)" "$3"
    rm -rf -- "${work}"
    return 0
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

# The release's assets as "<name> <sha256>" lines, sorted, from its REST description `json`:
# every file but the review tar, which the file-by-file releases also publish.
release_digests() {
  python3 -I -c 'import json, re, sys
for asset in sorted(json.load(open(sys.argv[1]))["assets"], key=lambda asset: asset["name"]):
    name, digest = asset["name"], asset.get("digest") or ""
    if name.endswith((".tar", ".tar.sha256")):
        continue
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        sys.exit(f"unexpected release asset {name!r} ({digest!r})")
    print(name, digest.removeprefix("sha256:"))' "$1"
}

# Downloads release `tag`'s asset `name` into `work`/bundle and checks its SHA-256 `digest`.
fetch_file() {
  local tag="$1" name="$2" digest="$3" work="$4"
  download_asset "${tag}" "${name}" "${work}/bundle/${name}" 300 "${work}"
  if [[ "$(sha256sum "${work}/bundle/${name}" | cut -d' ' -f1)" != "${digest}" ]]; then
    echo "${tag}/${name} does not match its release digest" >&2
    return 1
  fi
}

# Writes the REST description of release `tag` to `out`: with the token when there is one.
release_json() {
  local tag="$1" out="$2"
  if [[ -n "${GITHUB_RELEASE_TOKEN}" ]]; then
    github_release_json "moritzbrantner/asset-tooling/releases/tags/${tag}" "${out}"
  else
    curl -fsSL "${CURL_RETRY[@]}" --max-time 60 -H 'Accept: application/vnd.github+json' \
      -o "${out}" "${GITHUB_RELEASE_API}/moritzbrantner/asset-tooling/releases/tags/${tag}"
  fi
}

# A rolling release published file by file, as the Ranger figures and their equipment are
# (#866): every file but the review tar, each refreshed when its SHA-256 digest in the live
# release changes, so a re-upload that keeps export.ref.json still reaches both clients, and only
# the changed files are downloaded again. Offline, an existing copy is kept, except in CI.
fetch_rolling_files() {
  local tag="$1" dest="${ROOT_DIR}/web/public/$2"
  local work
  work="$(mktemp -d)"
  if ! release_json "${tag}" "${work}/release.json" ||
    ! release_digests "${work}/release.json" > "${work}/DIGESTS"; then
    if [[ -z "${IDENTITY}" && "${GITHUB_ACTIONS:-}" != true && -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]]; then
      echo "Could not read the ${tag} release; keeping ${dest}" >&2
      rm -rf -- "${work}"
      return 0
    fi
    echo "Could not read the ${tag} release's file digests" >&2
    rm -rf -- "${work}"
    return 1
  fi
  if [[ -n "${IDENTITY}" ]]; then
    printf '%s %s web/public/%s\n' "${tag}" "$(sha256sum "${work}/DIGESTS" | cut -d' ' -f1)" "$2"
    rm -rf -- "${work}"
    return 0
  fi
  if [[ -f "${dest}/RELEASE" && "$(cat "${dest}/RELEASE")" == "${tag}" ]] &&
    cmp -s "${work}/DIGESTS" "${dest}/DIGESTS"; then
    rm -rf -- "${work}"
    return 0
  fi
  mkdir "${work}/bundle"
  local name digest pid failed=0
  local -a pids=()
  while read -r name digest; do
    if [[ -f "${dest}/DIGESTS" && -f "${dest}/${name}" ]] && grep -Fxq -- "${name} ${digest}" "${dest}/DIGESTS"; then
      cp -- "${dest}/${name}" "${work}/bundle/${name}"
      continue
    fi
    # Eight downloads at a time: a figure release holds a few hundred files.
    if ((${#pids[@]} >= 8)); then
      wait "${pids[0]}" || failed=1
      pids=("${pids[@]:1}")
    fi
    fetch_file "${tag}" "${name}" "${digest}" "${work}" &
    pids+=("$!")
  done < "${work}/DIGESTS"
  for pid in "${pids[@]}"; do
    wait "${pid}" || failed=1
  done
  if ((failed)); then
    echo "Could not download the ${tag} release" >&2
    rm -rf -- "${work}"
    return 1
  fi
  cp -- "${work}/DIGESTS" "${work}/bundle/DIGESTS"
  printf '%s\n' "${tag}" > "${work}/bundle/RELEASE"
  place "${tag}" "${dest}" "${work}"
}

# The Ranger figures (Atlas 6.3.7, #493) and the equipment both carry (asset-tooling #517), file by
# file: the browser loads each entry it draws and the native build embeds them as published.
fetch_rolling_files humanoid-ranger-male characters/humanoid.ranger.male
fetch_rolling_files humanoid-ranger-female characters/humanoid.ranger.female
fetch_rolling_files humanoid-equipment characters/humanoid.equipment
# The Redbrand faction kit (#675, asset-tooling#522): the bandits' figures on the male rig and
# their belt and bracers, with KTX2 textures; they play the male figure's clips by joint name.
fetch_rolling redbrand-kit redbrand.kit characters/redbrand.kit
# The hair and beard kit (#677, asset-tooling#525): the male figures' hairstyles and beards as
# skinned pieces on his head joint, with KTX2 textures.
fetch_rolling hair-kit hair.kit characters/hair.kit
fetch_rolling monuments monuments.kit buildings/monuments.kit
fetch greyhaven-inn-v1 greyhaven.inn buildings/greyhaven.inn
fetch_rolling shrine-kit shrine.kit scenery/shrine.kit
# Greyhaven's cosmetic cat, dog and rodent (#419).
fetch_rolling greyhaven-ambient-animals greyhaven.ambient-animals scenery/greyhaven.ambient-animals
# Greyhaven's farm animals (#849): cows, horses, a donkey, pigs, sheep, goats and chickens.
fetch_rolling greyhaven-farm-animals greyhaven.farm-animals scenery/greyhaven.farm-animals
# Greyhaven's wildlife (#857): deer, foxes, rabbits, boar, crows, herons and songbirds.
fetch_rolling greyhaven-wildlife greyhaven.wildlife scenery/greyhaven.wildlife
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
# The rock modules that dress Greyhaven's crags and the Redbrand Hollow's cliffs (#668, asset-tooling#480).
fetch_rolling crag-kit crag.kit scenery/crag.kit
# The limestone blocks Greyhaven's rocks are drawn as (asset-tooling#536).
fetch_rolling limestone-rocks limestone-rocks.kit scenery/limestone-rocks.kit
# The stones along Greyhaven's watercourses (asset-tooling#538).
fetch_rolling river-stones river-stones.kit scenery/river-stones.kit
# Greyhaven's trees and rocks with their LOD chains and rock collision proxies (#49).
fetch_rolling greyhaven-nature greyhaven.nature scenery/greyhaven.nature
# Cosmetic meadow ground detail (#782), using the existing placed-kit loaders.
fetch_rolling greyhaven-meadow greyhaven-meadow.kit scenery/greyhaven-meadow.kit
# The Battle Royale fortress's authored art (#591, asset-tooling#458/#461, pass 2), fitted to its
# collision.
fetch_rolling fortress-kit fortress.kit scenery/fortress.kit
