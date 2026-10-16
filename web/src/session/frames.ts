/**
 * game-server session frames (protocol version 3), mirrored from the pinned
 * game-server's `src/protocol.rs`. `fixtures/protocol/session-frames-v3.hex`
 * holds these codecs and the Rust encoders to the same bytes. The MMO
 * payloads inside them (commands, player projections) are opaque here.
 * All multibyte fields are big-endian.
 */
export const SESSION_PROTOCOL_VERSION = 3;
/** `game_server::BROWSER_PROTOCOL_CONTRACT.route_version`. */
export const BROWSER_ROUTE_VERSION = 1;
export const COMMAND_HEADER_BYTES = 8;
export const SNAPSHOT_HEADER_BYTES = 20;
export const SNAPSHOT_FRAGMENT_HEADER_BYTES = 14;
export const WELCOME_BYTES = 46;
export const RECONNECT_TOKEN_BYTES = 16;
export const MAX_COMMAND_PAYLOAD_BYTES = 1_024;
export const MAX_SNAPSHOT_PAYLOAD_BYTES = 0xffff;
export const MAX_SNAPSHOT_FRAME_BYTES = SNAPSHOT_HEADER_BYTES + MAX_SNAPSHOT_PAYLOAD_BYTES;
export const MAX_SNAPSHOT_FRAGMENTS = 64;

const COMMAND_KIND = 1;
const SNAPSHOT_KIND = 2;
const WELCOME_KIND = 3;
const SNAPSHOT_FRAGMENT_KIND = 4;
const MAX_SEQUENCE = 0xffff_ffff;

/** What the host tells a connection once it is admitted or resumed. */
export type Welcome = {
  playerId: number;
  tickHz: number;
  maxPlayers: number;
  currentTick: bigint;
  connectionEpoch: number;
  /** The one-time resume capability; never logged or put in a page URL. */
  reconnectToken: Uint8Array;
  reconnectGraceTicks: bigint;
};

export type SnapshotFrame = { tick: bigint; stateHash: bigint; payload: Uint8Array };

/** One piece of a snapshot frame that exceeded the host's datagram budget. */
export type SnapshotFragment = { tick: bigint; index: number; count: number; chunk: Uint8Array };

/** A host-to-client realtime datagram: a whole snapshot or one fragment of one. */
export type SnapshotDatagram =
  | { kind: "snapshot"; frame: SnapshotFrame }
  | { kind: "fragment"; fragment: SnapshotFragment };

/** A malformed or inconsistent session frame; the counterpart of `game_server::ProtocolError`. */
export class SessionProtocolError extends Error {
  override readonly name = "SessionProtocolError";
}

function view(bytes: Uint8Array): DataView {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}

function requireLength(bytes: Uint8Array, expected: number): void {
  if (bytes.byteLength !== expected) {
    throw new SessionProtocolError(`expected ${expected} bytes, received ${bytes.byteLength}`);
  }
}

function requireMinimum(bytes: Uint8Array, minimum: number): void {
  if (bytes.byteLength < minimum) {
    throw new SessionProtocolError(`expected ${minimum} bytes, received ${bytes.byteLength}`);
  }
}

function requireHeader(bytes: Uint8Array, kind: number): void {
  const version = bytes[0];
  if (version !== SESSION_PROTOCOL_VERSION) {
    throw new SessionProtocolError(`unsupported protocol version ${version}`);
  }
  if (bytes[1] !== kind) {
    throw new SessionProtocolError(`unexpected frame kind ${bytes[1]}`);
  }
}

/** A command datagram: `[version, kind = 1, sequence: u32, length: u16, payload]`. */
export function encodeCommandFrame(sequence: number, payload: Uint8Array): Uint8Array {
  if (!Number.isInteger(sequence) || sequence < 1 || sequence > MAX_SEQUENCE) {
    throw new SessionProtocolError("command sequence must be a non-zero u32");
  }
  if (payload.byteLength > MAX_COMMAND_PAYLOAD_BYTES) {
    throw new SessionProtocolError(`payload size ${payload.byteLength} exceeds maximum ${MAX_COMMAND_PAYLOAD_BYTES}`);
  }
  const bytes = new Uint8Array(COMMAND_HEADER_BYTES + payload.byteLength);
  const header = view(bytes);
  header.setUint8(0, SESSION_PROTOCOL_VERSION);
  header.setUint8(1, COMMAND_KIND);
  header.setUint32(2, sequence);
  header.setUint16(6, payload.byteLength);
  bytes.set(payload, COMMAND_HEADER_BYTES);
  return bytes;
}

export function decodeWelcome(bytes: Uint8Array): Welcome {
  requireLength(bytes, WELCOME_BYTES);
  requireHeader(bytes, WELCOME_KIND);
  const fields = view(bytes);
  return {
    playerId: fields.getUint32(2),
    tickHz: fields.getUint16(6),
    maxPlayers: fields.getUint16(8),
    currentTick: fields.getBigUint64(10),
    connectionEpoch: fields.getUint32(18),
    reconnectToken: bytes.slice(22, 22 + RECONNECT_TOKEN_BYTES),
    reconnectGraceTicks: fields.getBigUint64(38),
  };
}

/** Checks a snapshot frame's header and exact length, without the hash, and returns its tick. */
function snapshotFrameTick(bytes: Uint8Array): bigint {
  requireMinimum(bytes, SNAPSHOT_HEADER_BYTES);
  requireHeader(bytes, SNAPSHOT_KIND);
  const header = view(bytes);
  requireLength(bytes, SNAPSHOT_HEADER_BYTES + header.getUint16(18));
  return header.getBigUint64(2);
}

/** Decodes a whole snapshot frame and verifies its state hash. */
export function decodeSnapshotFrame(bytes: Uint8Array): SnapshotFrame {
  const tick = snapshotFrameTick(bytes);
  const stateHash = view(bytes).getBigUint64(10);
  const payload = bytes.slice(SNAPSHOT_HEADER_BYTES);
  const expected = snapshotHash(tick, payload);
  if (expected !== stateHash) {
    throw new SessionProtocolError(
      `snapshot hash mismatch: expected 0x${expected.toString(16).padStart(16, "0")}, got 0x${stateHash.toString(16).padStart(16, "0")}`,
    );
  }
  return { tick, stateHash, payload };
}

/** Decodes one fragment datagram's header; its chunk is verified only after reassembly. */
export function decodeSnapshotFragment(bytes: Uint8Array): SnapshotFragment {
  requireMinimum(bytes, SNAPSHOT_FRAGMENT_HEADER_BYTES);
  requireHeader(bytes, SNAPSHOT_FRAGMENT_KIND);
  const header = view(bytes);
  const index = header.getUint8(10);
  const count = header.getUint8(11);
  if (count === 0 || count > MAX_SNAPSHOT_FRAGMENTS) {
    throw new SessionProtocolError(`snapshot fragment count ${count} is outside 1..=${MAX_SNAPSHOT_FRAGMENTS}`);
  }
  if (index >= count) {
    throw new SessionProtocolError(`snapshot fragment index ${index} is outside fragment count ${count}`);
  }
  const chunkLength = header.getUint16(12);
  if (chunkLength === 0) {
    throw new SessionProtocolError("snapshot fragment carries no bytes");
  }
  requireLength(bytes, SNAPSHOT_FRAGMENT_HEADER_BYTES + chunkLength);
  return { tick: header.getBigUint64(2), index, count, chunk: bytes.subarray(SNAPSHOT_FRAGMENT_HEADER_BYTES) };
}

/** Classifies one host datagram like `game_server::decode_snapshot_datagram`. */
export function decodeSnapshotDatagram(bytes: Uint8Array): SnapshotDatagram {
  return bytes[1] === SNAPSHOT_FRAGMENT_KIND
    ? { kind: "fragment", fragment: decodeSnapshotFragment(bytes) }
    : { kind: "snapshot", frame: decodeSnapshotFrame(bytes) };
}

const FNV_OFFSET_HIGH = 0xcbf2_9ce4;
const FNV_OFFSET_LOW = 0x8422_2325;
/** The FNV-1a 64-bit prime is `0x100_0000_01b3`: 2^40 + 0x1b3. */
const FNV_PRIME_LOW = 0x1b3;
const TWO_POW_32 = 0x1_0000_0000;

/**
 * `game_server::snapshot_hash`: 64-bit FNV-1a over the big-endian tick, the
 * big-endian `u64` payload length and the payload. It runs on two 32-bit
 * halves; tests compare it with a plain `bigint` reference.
 */
export function snapshotHash(tick: bigint, payload: Uint8Array): bigint {
  const prefix = new Uint8Array(16);
  const fields = view(prefix);
  fields.setBigUint64(0, BigInt.asUintN(64, tick));
  fields.setBigUint64(8, BigInt(payload.byteLength));
  let high = FNV_OFFSET_HIGH;
  let low = FNV_OFFSET_LOW;
  for (const bytes of [prefix, payload]) {
    for (let index = 0; index < bytes.length; index += 1) {
      low = (low ^ bytes[index]!) >>> 0;
      // (high·2^32 + low) · (2^40 + 0x1b3) mod 2^64; every partial product stays below 2^53.
      const lowProduct = low * FNV_PRIME_LOW;
      const carry = Math.floor(lowProduct / TWO_POW_32);
      high = (Math.imul(high, FNV_PRIME_LOW) + ((low << 8) >>> 0) + carry) >>> 0;
      low = lowProduct >>> 0;
    }
  }
  return (BigInt(high) << 32n) | BigInt(low);
}
