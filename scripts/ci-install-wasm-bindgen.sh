#!/usr/bin/env bash
# CI's wasm-bindgen CLI at the version Cargo.lock pins. The browser's `@moritzbrantner/maps` git
# dependency builds its WASM package in its prepare script and needs this CLI on PATH, so every job
# that runs `bun install` in web/ (Pages, and the Rust job's native/Pages reference tests, #896)
# installs it first. The prebuilt release lands in Cargo's bin directory, which those jobs put on PATH.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/github-release.sh
source "${ROOT_DIR}/scripts/lib/github-release.sh"
github_release_report_mode

REPOSITORY=wasm-bindgen/wasm-bindgen
BIN="${CARGO_HOME:-${HOME}/.cargo}/bin"
version="$(python3 -I -c 'import sys, tomllib
versions = [p["version"] for p in tomllib.load(open(sys.argv[1], "rb"))["package"] if p["name"] == "wasm-bindgen"]
if len(versions) != 1:
    raise SystemExit(f"Expected exactly one wasm-bindgen version, got {versions}")
print(versions[0])' "${ROOT_DIR}/Cargo.lock")"
if [[ -x "${BIN}/wasm-bindgen" && "$("${BIN}/wasm-bindgen" --version)" == "wasm-bindgen ${version}" ]]; then
  "${BIN}/wasm-bindgen" --version
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
name="wasm-bindgen-${version}-x86_64-unknown-linux-musl"

fetch_authenticated() {
  github_release_json "${REPOSITORY}/releases/tags/${version}" "${work}/release.json" || return 1
  github_release_asset "${REPOSITORY}" "${work}/release.json" "${name}.tar.gz" "${work}/${name}.tar.gz" 300
}

if [[ -z "${GITHUB_RELEASE_TOKEN}" ]] || ! fetch_authenticated; then
  if [[ -n "${GITHUB_RELEASE_TOKEN}" ]]; then
    echo "Authenticated wasm-bindgen download failed; trying the public URL." >&2
  fi
  curl --proto '=https' --tlsv1.2 -fsSL --retry 5 --retry-delay 5 --retry-connrefused --max-time 300 \
    -o "${work}/${name}.tar.gz" "https://github.com/${REPOSITORY}/releases/download/${version}/${name}.tar.gz"
fi
tar zxf "${work}/${name}.tar.gz" -C "${work}"
mkdir -p "${BIN}"
install -m 0755 "${work}/${name}/wasm-bindgen" "${BIN}/wasm-bindgen"
"${BIN}/wasm-bindgen" --version
