#!/usr/bin/env bash
# CI's Bun: the latest release (AGENTS.md BUN-001 exception), which a `uses:` step cannot retry.
# Each download retries transient failures and the archive is checked against the release's
# SHASUMS256.txt. Later steps find it through $GITHUB_PATH; the installing step adds ~/.bun/bin itself.
set -euo pipefail

ASSET=bun-linux-x64.zip
RELEASES=https://github.com/oven-sh/bun/releases
INSTALL="${HOME}/.bun/bin"
if [[ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]]; then
  echo "scripts/ci-install-bun.sh only supports Linux x86_64 runners." >&2
  exit 1
fi

CURL=(curl --proto '=https' --tlsv1.2 -fsSL --retry 5 --retry-delay 5 --retry-connrefused)
latest="$("${CURL[@]}" --max-time 60 -o /dev/null -w '%{url_effective}' "${RELEASES}/latest")"
tag="${latest##*/}"
if [[ ! "${tag}" =~ ^bun-v[0-9] ]]; then
  echo "Unexpected latest Bun release: ${latest}" >&2
  exit 1
fi

if [[ ! -x "${INSTALL}/bun" || "$("${INSTALL}/bun" --version)" != "${tag#bun-v}" ]]; then
  work="$(mktemp -d)"
  trap 'rm -rf -- "${work}"' EXIT
  "${CURL[@]}" --max-time 300 -o "${work}/${ASSET}" "${RELEASES}/download/${tag}/${ASSET}"
  "${CURL[@]}" --max-time 60 -o "${work}/SHASUMS256.txt" "${RELEASES}/download/${tag}/SHASUMS256.txt"
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
