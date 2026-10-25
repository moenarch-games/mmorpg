/**
 * The browser's copy of `mmorpg_protocol::PROTOCOL_ID`: a random 24-bit token naming the
 * host <-> client wire format, not a counter. `scripts/new-protocol-id.sh` rerolls both copies
 * together, and a Rust test holds them equal (docs/adr/0006-merge-safe-format-identities.md).
 */
export const PROTOCOL_ID = 0xb20c8a;
/** Snapshots lead with the token's three bytes. */
export const PROTOCOL_ID_BYTES = [(PROTOCOL_ID >> 16) & 0xff, (PROTOCOL_ID >> 8) & 0xff, PROTOCOL_ID & 0xff] as const;
/** Commands lead with the token's low byte. */
export const PROTOCOL_BYTE = PROTOCOL_ID & 0xff;
