#!/usr/bin/env bash
# CI's cargo-nextest, downloaded to a file first so a failed or partial download is retried or
# fails the step instead of reaching tar.
set -euo pipefail

BIN="${CARGO_HOME:-${HOME}/.cargo}/bin"
work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
curl --proto '=https' --tlsv1.2 -fsSL --retry 5 --retry-delay 5 --retry-connrefused --max-time 300 \
  -o "${work}/nextest.tar.gz" https://get.nexte.st/latest/linux
tar zxf "${work}/nextest.tar.gz" -C "${BIN}"
"${BIN}/cargo-nextest" nextest --version
