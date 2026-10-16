import {
  COMMAND_HEADER_BYTES,
  MAX_SNAPSHOT_FRAGMENTS,
  SESSION_PROTOCOL_VERSION,
  SNAPSHOT_FRAGMENT_HEADER_BYTES,
  SNAPSHOT_HEADER_BYTES,
  WELCOME_BYTES,
  snapshotHash,
  type Welcome,
} from "../../src/session/frames";

/**
 * Test-only host side of the game-server session frames: the encoders a zone
 * host uses, mirrored from the pinned `game_server::protocol`. The golden
 * fixture `fixtures/protocol/session-frames-v3.hex` holds them to the Rust
 * bytes, so test hosts built on them stay protocol-faithful (TEST-021).
 */
const COMMAND_KIND = 1;
const SNAPSHOT_KIND = 2;
const WELCOME_KIND = 3;
const SNAPSHOT_FRAGMENT_KIND = 4;

function view(bytes: Uint8Array): DataView {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}

export function encodeWelcomeFrame(welcome: Welcome): Uint8Array {
  const bytes = new Uint8Array(WELCOME_BYTES);
  const fields = view(bytes);
  fields.setUint8(0, SESSION_PROTOCOL_VERSION);
  fields.setUint8(1, WELCOME_KIND);
  fields.setUint32(2, welcome.playerId);
  fields.setUint16(6, welcome.tickHz);
  fields.setUint16(8, welcome.maxPlayers);
  fields.setBigUint64(10, welcome.currentTick);
  fields.setUint32(18, welcome.connectionEpoch);
  bytes.set(welcome.reconnectToken, 22);
  fields.setBigUint64(38, welcome.reconnectGraceTicks);
  return bytes;
}

/** `game_server::encode_snapshot` of a frame whose state hash is computed from its payload. */
export function encodeSnapshotFrame(tick: bigint, payload: Uint8Array): Uint8Array {
  const bytes = new Uint8Array(SNAPSHOT_HEADER_BYTES + payload.byteLength);
  const fields = view(bytes);
  fields.setUint8(0, SESSION_PROTOCOL_VERSION);
  fields.setUint8(1, SNAPSHOT_KIND);
  fields.setBigUint64(2, tick);
  fields.setBigUint64(10, snapshotHash(tick, payload));
  fields.setUint16(18, payload.byteLength);
  bytes.set(payload, SNAPSHOT_HEADER_BYTES);
  return bytes;
}

/** One fragment datagram with arbitrary (possibly invalid) header fields. */
export function rawFragment(tick: bigint, index: number, count: number, chunk: Uint8Array): Uint8Array {
  const bytes = new Uint8Array(SNAPSHOT_FRAGMENT_HEADER_BYTES + chunk.byteLength);
  const fields = view(bytes);
  fields.setUint8(0, SESSION_PROTOCOL_VERSION);
  fields.setUint8(1, SNAPSHOT_FRAGMENT_KIND);
  fields.setBigUint64(2, tick);
  fields.setUint8(10, index);
  fields.setUint8(11, count);
  fields.setUint16(12, chunk.byteLength);
  bytes.set(chunk, SNAPSHOT_FRAGMENT_HEADER_BYTES);
  return bytes;
}

/** `game_server::encode_snapshot_fragments`: full chunks except the last, in index order. */
export function encodeSnapshotFragments(frame: Uint8Array, maxDatagramBytes: number): Uint8Array[] {
  const tick = view(frame).getBigUint64(2);
  const capacity = Math.min(maxDatagramBytes - SNAPSHOT_FRAGMENT_HEADER_BYTES, 0xffff);
  if (capacity <= 0) {
    throw new Error(`datagram budget ${maxDatagramBytes} cannot carry a fragment`);
  }
  const count = Math.ceil(frame.byteLength / capacity);
  if (count > MAX_SNAPSHOT_FRAGMENTS) {
    throw new Error(`snapshot needs ${count} fragments`);
  }
  return Array.from({ length: count }, (_, index) =>
    rawFragment(tick, index, count, frame.subarray(index * capacity, (index + 1) * capacity)));
}

export type CommandFrame = { sequence: number; payload: Uint8Array };

/** `game_server::decode_command`, as a host reads a client datagram. */
export function decodeCommandFrame(bytes: Uint8Array): CommandFrame {
  if (bytes.byteLength < COMMAND_HEADER_BYTES || bytes[0] !== SESSION_PROTOCOL_VERSION || bytes[1] !== COMMAND_KIND) {
    throw new Error("not a command frame");
  }
  const fields = view(bytes);
  const sequence = fields.getUint32(2);
  if (sequence === 0) {
    throw new Error("command sequence 0");
  }
  if (bytes.byteLength !== COMMAND_HEADER_BYTES + fields.getUint16(6)) {
    throw new Error("command length mismatch");
  }
  return { sequence, payload: bytes.slice(COMMAND_HEADER_BYTES) };
}
