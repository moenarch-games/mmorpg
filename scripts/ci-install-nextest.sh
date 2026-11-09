#!/usr/bin/env bash
# CI's cargo-nextest, downloaded to a file first so a failed or partial download is retried or
# fails the step instead of reaching tar. With a token it comes through the GitHub API, otherwise
# (or if that fails) from nexte.st, which redirects to the same public release asset.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/github-release.sh
source "${ROOT_DIR}/scripts/lib/github-release.sh"
github_release_report_mode

BIN="${CARGO_HOME:-${HOME}/.cargo}/bin"
work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT

# The newest stable cargo-nextest release, which shares its repository with other crates' releases.
fetch_authenticated() {
  local name
  github_release_json 'nextest-rs/nextest/releases?per_page=100' "${work}/releases.json" || return 1
  name="$(python3 -I -c 'import json, sys
release = next(r for r in json.load(open(sys.argv[1])) if r["tag_name"].startswith("cargo-nextest-") and not r["prerelease"])
json.dump(release, open(sys.argv[2], "w"))
print(release["tag_name"] + "-x86_64-unknown-linux-gnu.tar.gz")' "${work}/releases.json" "${work}/release.json")" || return 1
  github_release_asset nextest-rs/nextest "${work}/release.json" "${name}" "${work}/nextest.tar.gz" 300
}

if [[ -z "${GITHUB_RELEASE_TOKEN}" ]] || ! fetch_authenticated; then
  if [[ -n "${GITHUB_RELEASE_TOKEN}" ]]; then
    echo "Authenticated nextest download failed; trying get.nexte.st." >&2
  fi
  curl --proto '=https' --tlsv1.2 -fsSL --retry 5 --retry-delay 5 --retry-connrefused --max-time 300 \
    -o "${work}/nextest.tar.gz" https://get.nexte.st/latest/linux
fi
tar zxf "${work}/nextest.tar.gz" -C "${BIN}"
"${BIN}/cargo-nextest" nextest --version
