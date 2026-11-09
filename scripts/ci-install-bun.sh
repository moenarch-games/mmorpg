#!/usr/bin/env bash
# CI's Bun: the latest release (AGENTS.md BUN-001 exception), which a `uses:` step cannot retry.
# Each download retries transient failures and the archive is checked against the release's
# SHASUMS256.txt. With a token it comes through the GitHub API, otherwise (or if that fails) from
# the public release URLs. Later steps find it through $GITHUB_PATH; the installing step adds
# ~/.bun/bin itself.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/github-release.sh
source "${ROOT_DIR}/scripts/lib/github-release.sh"
github_release_report_mode

REPOSITORY=oven-sh/bun
ASSET=bun-linux-x64.zip
RELEASES="https://github.com/${REPOSITORY}/releases"
INSTALL="${HOME}/.bun/bin"
if [[ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]]; then
  echo "scripts/ci-install-bun.sh only supports Linux x86_64 runners." >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
CURL=(curl --proto '=https' --tlsv1.2 -fsSL --retry 5 --retry-delay 5 --retry-connrefused)

# Downloads release asset `name` to `out`, by the API when the release's JSON is available.
download() {
  local name="$1" out="$2" max_time="$3"
  if [[ -s "${work}/release.json" ]]; then
    if github_release_asset "${REPOSITORY}" "${work}/release.json" "${name}" "${out}" "${max_time}"; then
      return 0
    fi
    echo "Authenticated download of ${name} failed; trying the public URL." >&2
  fi
  "${CURL[@]}" --max-time "${max_time}" -o "${out}" "${RELEASES}/download/${tag}/${name}"
}

tag=""
if [[ -n "${GITHUB_RELEASE_TOKEN}" ]] && github_release_json "${REPOSITORY}/releases/latest" "${work}/release.json"; then
  tag="$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["tag_name"])' "${work}/release.json")"
else
  rm -f -- "${work}/release.json"
  latest="$("${CURL[@]}" --max-time 60 -o /dev/null -w '%{url_effective}' "${RELEASES}/latest")"
  tag="${latest##*/}"
fi
if [[ ! "${tag}" =~ ^bun-v[0-9] ]]; then
  echo "Unexpected latest Bun release: ${tag}" >&2
  exit 1
fi

if [[ ! -x "${INSTALL}/bun" || "$("${INSTALL}/bun" --version)" != "${tag#bun-v}" ]]; then
  download "${ASSET}" "${work}/${ASSET}" 300
  download SHASUMS256.txt "${work}/SHASUMS256.txt" 60
  (cd "${work}" && grep -E "[[:space:]]${ASSET}\$" SHASUMS256.txt | sha256sum -c -)
  unzip -q "${work}/${ASSET}" -d "${work}"
  mkdir -p "${INSTALL}"
  install -m 0755 "${work}/${ASSET%.zip}/bun" "${INSTALL}/bun"
  ln -sf bun "${INSTALL}/bunx"
  if [[ "$("${INSTALL}/bun" --version)" != "${tag#bun-v}" ]]; then
    echo "The installed Bun does not report ${tag}." >&2
    exit 1
  fi
fi
if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${INSTALL}" >> "${GITHUB_PATH}"
fi
"${INSTALL}/bun" --version
