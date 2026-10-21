#!/usr/bin/env bash
# Explicit acquisition/preparation of native inputs using the asset-processing owner.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="${XDG_CACHE_HOME:-${HOME}/.cache}/mmorpg/ranger-tools"
PROCESSOR_REV=c13e053800d0a296d9594f84d84e60a9c0f944ee
PROCESSOR="${ASSET_TOOLING_ROOT:-${TOOLS}/asset-tooling-${PROCESSOR_REV}}"
KTX="${ASSET_TOOLING_KTX:-ktx}"
command -v bun >/dev/null || { echo 'Native asset preparation requires Bun.' >&2; exit 1; }
mkdir -p "${TOOLS}"
if [[ -n "${ASSET_TOOLING_ROOT:-}" && ! -f "${PROCESSOR}/scripts/prepare-native-downloads.ts" ]]; then
  echo 'ASSET_TOOLING_ROOT must contain scripts/prepare-native-downloads.ts.' >&2
  exit 1
fi
if [[ ! -f "${PROCESSOR}/scripts/prepare-native-downloads.ts" ]]; then
  work="$(mktemp -d "${TOOLS}/processor.XXXXXX")"
  trap 'rm -rf -- "${work}"' EXIT
  curl -fsSL --retry 3 -o "${work}/source.tar.gz" "https://codeload.github.com/moritzbrantner/asset-tooling/tar.gz/${PROCESSOR_REV}"
  mkdir "${work}/source"
  tar -xzf "${work}/source.tar.gz" --strip-components=1 -C "${work}/source"
  mv "${work}/source" "${PROCESSOR}"
  rm -rf -- "${work}"
  trap - EXIT
fi
if ! command -v "${KTX}" >/dev/null; then
  if [[ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]]; then
    echo 'Install Khronos KTX-Software 4.4.2 and set ASSET_TOOLING_KTX to its ktx CLI.' >&2
    exit 1
  fi
  KTX="${TOOLS}/ktx-4.4.2/bin/ktx"
  if [[ ! -x "${KTX}" ]]; then
    work="$(mktemp -d "${TOOLS}/ktx.XXXXXX")"
    trap 'rm -rf -- "${work}"' EXIT
    curl -fsSL --retry 3 -o "${work}/ktx.tar.bz2" 'https://github.com/KhronosGroup/KTX-Software/releases/download/v4.4.2/KTX-Software-4.4.2-Linux-x86_64.tar.bz2'
    mkdir -p "${TOOLS}/ktx-4.4.2"
    tar -xjf "${work}/ktx.tar.bz2" --strip-components=1 -C "${TOOLS}/ktx-4.4.2"
    rm -rf -- "${work}"
    trap - EXIT
  fi
fi
if [[ ! -d "${PROCESSOR}/node_modules" ]]; then
  (cd "${PROCESSOR}" && bun install --frozen-lockfile)
fi
ASSET_TOOLING_KTX="${KTX}" bun "${PROCESSOR}/scripts/prepare-native-downloads.ts" \
  --index "${ROOT_DIR}/web/public/characters/humanoid.ranger.male/humanoid-ranger-male.index.json" \
  --entries humanoid.ranger.male/default,equipment.sword.arming/default,equipment.shield.heater/default,equipment.cap.ranger/default,equipment.helm.ironcrest/default,hair.simple-parted/hair-1 \
  --out "${ROOT_DIR}/web/public/characters/humanoid.ranger.male/native"
