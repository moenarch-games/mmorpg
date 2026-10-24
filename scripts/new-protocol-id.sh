#!/usr/bin/env bash
# Replaces the wire protocol ID (docs/adr/0006-merge-safe-format-identities.md) with fresh
# random bytes in both copies: crates/mmorpg-protocol/src/lib.rs and web/src/protocol-id.ts.
# Run it once in any change to the command, projection, observation or canonical layout, then
# re-record the fixtures with `MMORPG_UPDATE_FIXTURES=1 cargo test --workspace --locked`.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
id="$(od -An -N3 -tx1 /dev/urandom | tr -d ' \n')"
sed -i -E "s/^pub const PROTOCOL_ID: u32 = 0x00[0-9a-f]{2}_[0-9a-f]{4};$/pub const PROTOCOL_ID: u32 = 0x00${id:0:2}_${id:2:4};/" \
  "${ROOT_DIR}/crates/mmorpg-protocol/src/lib.rs"
sed -i -E "s/^export const PROTOCOL_ID = 0x[0-9a-f]{6};$/export const PROTOCOL_ID = 0x${id};/" \
  "${ROOT_DIR}/web/src/protocol-id.ts"
grep -q "0x00${id:0:2}_${id:2:4};" "${ROOT_DIR}/crates/mmorpg-protocol/src/lib.rs"
grep -q "0x${id};" "${ROOT_DIR}/web/src/protocol-id.ts"
echo "protocol ID is now 0x${id}"
