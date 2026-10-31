#!/usr/bin/env bash
# Replaces the wire protocol ID (docs/adr/0006-merge-safe-format-identities.md) in
# crates/mmorpg-protocol/src/lib.rs with fresh random bytes; the browser reads it through
# mmorpg-wasm (#541). Run it once in any change to the command, projection, observation or
# canonical layout, then re-record the fixtures with `scripts/rerecord.sh`.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
id="$(od -An -N3 -tx1 /dev/urandom | tr -d ' \n')"
sed -i -E "s/^pub const PROTOCOL_ID: u32 = 0x00[0-9a-f]{2}_[0-9a-f]{4};$/pub const PROTOCOL_ID: u32 = 0x00${id:0:2}_${id:2:4};/" \
  "${ROOT_DIR}/crates/mmorpg-protocol/src/lib.rs"
grep -q "0x00${id:0:2}_${id:2:4};" "${ROOT_DIR}/crates/mmorpg-protocol/src/lib.rs"
echo "protocol ID is now 0x${id}"
