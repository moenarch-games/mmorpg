import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  BROWSER_ROUTE_VERSION,
  MAX_COMMAND_PAYLOAD_BYTES,
  MAX_SNAPSHOT_FRAGMENTS,
  MAX_SNAPSHOT_PAYLOAD_BYTES,
  RECONNECT_TOKEN_BYTES,
  SESSION_PROTOCOL_VERSION,
  decodeSnapshotFragment,
  decodeSnapshotFrame,
  decodeWelcome,
  encodeCommandFrame,
  snapshotHash,
} from "../src/session/frames";
import { parseSessionRoute, reconnectUrl } from "../src/session/route";
import {
  SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES,
  SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS,
  SNAPSHOT_REASSEMBLY_MAX_PENDING,
  SnapshotReassembler,
} from "../src/session/snapshot-reassembler";
import {
  decodeCommandFrame,
  encodeSnapshotFragments,
  encodeSnapshotFrame,
  encodeWelcomeFrame,
} from "./support/session-host-frames";

// Rendered and verified by crates/mmorpg-game-server/tests/session_frames.rs from the pinned game-server.
const fixture = readFileSync(new URL("../../fixtures/protocol/session-frames-v3.hex", import.meta.url), "utf8");

type Line = { encoded: string; fields: Record<string, string> };

function lines(kind: string): Line[] {
  return fixture
    .split("\n")
    .filter((line) => line.startsWith(`${kind} `))
    .map((line) => {
      const [, ...rest] = line.split(" ");
      const encoded = rest[0]?.includes("=") ? "" : (rest.shift() ?? "");
      const fields = Object.fromEntries(rest.map((field) => {
        const split = field.indexOf("=");
        return [field.slice(0, split), field.slice(split + 1)];
      }));
      return { encoded, fields };
    });
}

const bytes = (hex: string) => Uint8Array.from(Buffer.from(hex, "hex"));
const hex = (value: Uint8Array) => Buffer.from(value).toString("hex");
const field = (line: Line, name: string) => {
  const value = line.fields[name];
  if (value === undefined) throw new Error(`fixture line lacks ${name}`);
  return value;
};

/** The simple reference: 64-bit FNV-1a over `bigint`, like `game_server::snapshot_hash`. */
function referenceHash(tick: bigint, payload: Uint8Array): bigint {
  const prefix = new Uint8Array(16);
  new DataView(prefix.buffer).setBigUint64(0, tick);
  new DataView(prefix.buffer).setBigUint64(8, BigInt(payload.byteLength));
  let hash = 0xcbf2_9ce4_8422_2325n;
  for (const byte of [...prefix, ...payload]) {
    hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100_0000_01b3n);
  }
  return hash;
}

describe("game-server session frames (shared golden fixture)", () => {
  test("the browser pins the same browser protocol contract and reassembly bounds", () => {
    const [contract] = lines("contract");
    if (!contract) throw new Error("fixture lacks the contract line");
    expect(Number(field(contract, "route_version"))).toBe(BROWSER_ROUTE_VERSION);
    expect(Number(field(contract, "protocol_version"))).toBe(SESSION_PROTOCOL_VERSION);
    expect(Number(field(contract, "reconnect_token_bytes"))).toBe(RECONNECT_TOKEN_BYTES);
    expect(Number(field(contract, "max_command_payload_bytes"))).toBe(MAX_COMMAND_PAYLOAD_BYTES);
    expect(Number(field(contract, "max_snapshot_payload_bytes"))).toBe(MAX_SNAPSHOT_PAYLOAD_BYTES);
    expect(Number(field(contract, "max_snapshot_fragments"))).toBe(MAX_SNAPSHOT_FRAGMENTS);
    expect(Number(field(contract, "reassembly_max_pending"))).toBe(SNAPSHOT_REASSEMBLY_MAX_PENDING);
    expect(Number(field(contract, "reassembly_max_buffered_bytes"))).toBe(SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES);
    expect(Number(field(contract, "reassembly_max_idle_datagrams"))).toBe(SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS);
  });

  test("the resume route is the hosted reconnect route, and a page never accepts one", () => {
    const [route] = lines("route");
    if (!route) throw new Error("fixture lacks the route line");
    const admission = parseSessionRoute(`https://127.0.0.1:4433${field(route, "prefix")}/matches/${field(route, "match_id")}`);
    expect(admission).toMatchObject({ prefix: "/game", matchId: "zone-1", zoneId: 1 });
    const resume = reconnectUrl(admission, bytes(field(route, "token")));
    expect(resume).toBe(`https://127.0.0.1:4433${route.encoded}`);
    expect(() => parseSessionRoute(resume)).toThrow();
  });

  test("welcomes decode to the Rust fields and the test host encodes the same bytes", () => {
    const welcomes = lines("welcome");
    expect(welcomes.length).toBe(2);
    for (const line of welcomes) {
      const welcome = decodeWelcome(bytes(line.encoded));
      expect(welcome).toEqual({
        playerId: Number(field(line, "player_id")),
        tickHz: Number(field(line, "tick_hz")),
        maxPlayers: Number(field(line, "max_players")),
        currentTick: BigInt(field(line, "current_tick")),
        connectionEpoch: Number(field(line, "connection_epoch")),
        reconnectToken: bytes(field(line, "reconnect_token")),
        reconnectGraceTicks: BigInt(field(line, "reconnect_grace_ticks")),
      });
      expect(hex(encodeWelcomeFrame(welcome))).toBe(line.encoded);
    }
  });

  test("welcomes with a wrong length, version or kind are rejected", () => {
    const valid = bytes(lines("welcome")[0]!.encoded);
    expect(() => decodeWelcome(valid.slice(0, -1))).toThrow("expected 46 bytes");
    expect(() => decodeWelcome(Uint8Array.of(...valid, 0))).toThrow("expected 46 bytes");
    for (const [offset, value, message] of [[0, 2, "version"], [1, 2, "kind"]] as const) {
      const invalid = valid.slice();
      invalid[offset] = value;
      expect(() => decodeWelcome(invalid)).toThrow(message);
    }
  });

  test("command frames encode to the Rust bytes and reject what the host would", () => {
    const commands = lines("command");
    expect(commands.length).toBe(3);
    for (const line of commands) {
      const sequence = Number(field(line, "sequence"));
      const payload = bytes(field(line, "payload"));
      expect(hex(encodeCommandFrame(sequence, payload))).toBe(line.encoded);
      expect(decodeCommandFrame(bytes(line.encoded))).toEqual({ sequence, payload });
    }
    expect(() => encodeCommandFrame(0, new Uint8Array())).toThrow("non-zero u32");
    expect(() => encodeCommandFrame(0x1_0000_0000, new Uint8Array())).toThrow("non-zero u32");
    expect(() => encodeCommandFrame(1.5, new Uint8Array())).toThrow("non-zero u32");
    expect(() => encodeCommandFrame(1, new Uint8Array(MAX_COMMAND_PAYLOAD_BYTES + 1))).toThrow("exceeds maximum");
    expect(encodeCommandFrame(1, new Uint8Array(MAX_COMMAND_PAYLOAD_BYTES)).byteLength).toBe(8 + MAX_COMMAND_PAYLOAD_BYTES);
  });

  test("snapshot frames decode with a verified state hash", () => {
    const snapshots = lines("snapshot");
    expect(snapshots.length).toBe(3);
    for (const line of snapshots) {
      const tick = BigInt(field(line, "tick"));
      const payload = bytes(field(line, "payload"));
      const stateHash = BigInt(`0x${field(line, "state_hash")}`);
      expect(decodeSnapshotFrame(bytes(line.encoded))).toEqual({ tick, stateHash, payload });
      expect(snapshotHash(tick, payload)).toBe(stateHash);
      expect(hex(encodeSnapshotFrame(tick, payload))).toBe(line.encoded);
    }
  });

  test("snapshot frames with a forged hash, wrong length, version or kind are rejected", () => {
    const valid = bytes(lines("snapshot")[1]!.encoded);
    const forged = valid.slice();
    forged[forged.length - 1]! ^= 0xff;
    expect(() => decodeSnapshotFrame(forged)).toThrow("snapshot hash mismatch");
    expect(() => decodeSnapshotFrame(valid.slice(0, -1))).toThrow("expected");
    expect(() => decodeSnapshotFrame(Uint8Array.of(...valid, 0))).toThrow("expected");
    expect(() => decodeSnapshotFrame(valid.slice(0, 19))).toThrow("expected 20 bytes");
    for (const [offset, value] of [[0, 4], [1, 1]] as const) {
      const invalid = valid.slice();
      invalid[offset] = value;
      expect(() => decodeSnapshotFrame(invalid)).toThrow();
    }
  });

  test("fragment datagrams reassemble into the fixture's largest snapshot", () => {
    const fragments = lines("fragment");
    expect(fragments.length).toBe(3);
    const whole = lines("snapshot").at(-1)!;
    const reassembler = new SnapshotReassembler();
    const delivered = fragments.map((line) => reassembler.accept(bytes(line.encoded)));
    expect(delivered.slice(0, -1)).toEqual([null, null]);
    expect(delivered.at(-1)).toEqual(decodeSnapshotFrame(bytes(whole.encoded)));
    expect(reassembler.stats()).toMatchObject({ reassembledSnapshots: 1, bufferedFragments: 3 });
    for (const line of fragments) {
      expect(decodeSnapshotFragment(bytes(line.encoded))).toMatchObject({
        tick: BigInt(field(line, "tick")),
        index: Number(field(line, "index")),
        count: Number(field(line, "count")),
      });
    }
    // The fixture splits the frame at a 38-byte datagram budget; the test host splits identically.
    const budget = bytes(fragments[0]!.encoded).byteLength;
    expect(encodeSnapshotFragments(bytes(whole.encoded), budget).map(hex)).toEqual(fragments.map((line) => line.encoded));
  });

  test("the two-halves snapshot hash equals the bigint FNV-1a reference", () => {
    let state = 0x2545_f491;
    const next = () => {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      return state >>> 0;
    };
    for (let round = 0; round < 64; round += 1) {
      const payload = Uint8Array.from({ length: next() % 300 }, () => next() & 0xff);
      const tick = (BigInt(next()) << 32n) | BigInt(next());
      expect(snapshotHash(tick, payload)).toBe(referenceHash(tick, payload));
    }
    expect(snapshotHash(0xffff_ffff_ffff_ffffn, new Uint8Array(0))).toBe(referenceHash(0xffff_ffff_ffff_ffffn, new Uint8Array(0)));
  });
});
