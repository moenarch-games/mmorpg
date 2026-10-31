import { wireCatalog, type ZoneSnapshot } from "../../src/replication";
import { wasm } from "../../src/wasm";

/** The wire protocol ID (ADR 0006), from `mmorpg_protocol` through the preloaded module. */
export const PROTOCOL_ID = wireCatalog().protocolId;
/** The ID's low byte, which leads every command. */
export const PROTOCOL_BYTE = PROTOCOL_ID & 0xff;
/** The three ID bytes that lead every snapshot. */
export const PROTOCOL_ID_BYTES = [(PROTOCOL_ID >> 16) & 0xff, (PROTOCOL_ID >> 8) & 0xff, PROTOCOL_BYTE] as const;

/** Real projection bytes for `snapshot`, from the Rust encoder (#541). */
export function encodeSnapshot(snapshot: ZoneSnapshot): Uint8Array {
  return wasm.encodeSnapshot(snapshot);
}
