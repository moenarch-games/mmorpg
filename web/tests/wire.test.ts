import { describe, expect, test } from "bun:test";
import { encodeCommand, encodeObservedCommand, type WorldCommand } from "../src/command-wire";
import { decodeSnapshot, projectionBytes, SnapshotBuffer, TICK_HZ, yawFromRadians, type EntityState, type ZoneSnapshot } from "../src/replication";
import { playerEntity, testSnapshot } from "./support/snapshots";
import { encodeSnapshot } from "./support/wire";

const player = (entityId: number, x: number, facing = 0): EntityState => playerEntity(entityId, [x, 90, 0], [21, 0, 0], facing);
const snapshot = (tick: number | bigint, entities = [player(1, Number(tick) * 12)]): ZoneSnapshot => testSnapshot({
  zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, viewerId: 1, entities,
});

describe("the mmorpg-wasm wire boundary (#541)", () => {
  test("a projection crosses as the browser shape and keeps the bytes it came from", () => {
    const sent = { ...snapshot(18_446_744_073_709_551_615n, [player(1, 0), player(2, 40)]), chat: [{ speaker: 2, channel: "emote" as const, emote: "wave" as const }] };
    const bytes = encodeSnapshot(sent);
    const decoded = decodeSnapshot(bytes);
    expect(decoded).toEqual({ ...sent, stats: null });
    expect(projectionBytes(decoded)).toEqual(bytes);
    // The Rust decoder still rejects what it always did, such as a projection that does not lead with its viewer.
    expect(() => decodeSnapshot(encodeSnapshot({ ...sent, entities: [player(2, 40), player(1, 0)] }))).toThrow("viewer");
    expect(() => decodeSnapshot(bytes.subarray(0, -1))).toThrow();
  });

  test("encodeCommand rejects intent the zone's decoder would reject", () => {
    for (const invalid of [
      { kind: "move", forward: 2, strafe: 0, facing: 0 },
      { kind: "move", forward: 0, strafe: -2, facing: 0 },
      { kind: "move", forward: 0, strafe: 0, facing: 65_536 },
      { kind: "move", forward: 0, strafe: 0, facing: -1 },
      { kind: "move", forward: 0, strafe: 0, facing: 1.5 },
      { kind: "select-target", target: { kind: "creature", id: -1 } },
      { kind: "select-target", target: { kind: "creature", id: 2 ** 32 } },
      { kind: "select-target", target: { kind: "npc", id: 0.5 } },
      { kind: "loot", creatureId: -1, diedAt: 0n },
      { kind: "loot", creatureId: 1, diedAt: -1n },
      { kind: "loot", creatureId: 1, diedAt: 1n << 64n },
      { kind: "use-ability", ability: 256, target: null },
      { kind: "emote", emote: "dance" },
      { kind: "directed-strike", ability: 1, facing: 0, aim: { side: "up", height: "low" } },
      { kind: "choose-class", classId: 0, sex: 256 },
      { kind: "use-waystone", waystone: 0 },
      { kind: "repair", piece: 65_535 },
      { kind: "chat", channel: "say", text: " " },
    ]) {
      expect(() => encodeCommand(invalid as WorldCommand)).toThrow();
    }
    expect(() => encodeObservedCommand({ kind: "jump" }, -1n)).toThrow();
    expect(() => encodeObservedCommand({ kind: "jump" }, 1n << 64n)).toThrow();
    expect(encodeObservedCommand({ kind: "jump" }, 5n).byteLength).toBe(encodeCommand({ kind: "jump" }).byteLength + 9);
  });
});

describe("snapshot presentation", () => {
  test("interpolates irregular server ticks and holds during packet loss", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(10));
    buffer.push(snapshot(13));
    expect(buffer.sample(11n, 0.5)[0]?.position).toEqual([138, 90, 0]);
    expect(buffer.sample(100n)[0]?.position).toEqual([156, 90, 0]);
    expect(buffer.push(snapshot(12))).toBe(false);
    expect(buffer.push(snapshot(13))).toBe(false);
    expect(buffer.sample(13n)[0]?.position).toEqual([156, 90, 0]);
  });

  test("interpolates facing along the shorter arc", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(0, [player(1, 0, 65_000)]));
    buffer.push(snapshot(2, [player(1, 0, 500)]));
    expect(buffer.sample(1n)[0]?.facing).toBe(65_518);
    expect(buffer.sample(0n, 0.5)[0]?.facing).toBe(65_259);
    buffer.reset();
    buffer.push(snapshot(0, [player(1, 0, 500)]));
    buffer.push(snapshot(2, [player(1, 0, 65_000)]));
    expect(buffer.sample(1n)[0]?.facing).toBe(65_518);
    expect(buffer.sample(2n)[0]?.facing).toBe(65_000);
  });

  test("appearance and removal occur at authoritative ticks", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1, [player(1, 0), player(2, 10)]));
    buffer.push(snapshot(2, [player(1, 12), player(3, 20)]));
    expect(buffer.sample(1n, 0.5).map((unit) => unit.entityId)).toEqual([1, 2]);
    expect(buffer.sample(2n).map((unit) => unit.entityId)).toEqual([1, 3]);
  });

  test("does not blend across zone, content, viewer, or explicit authority resets", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1));
    expect(() => buffer.push({ ...snapshot(2), zoneId: 2 })).toThrow("reset");
    expect(() => buffer.push({ ...snapshot(2), contentRevision: 8n })).toThrow("reset");
    expect(() => buffer.push({ ...snapshot(2), viewerId: 2 })).toThrow("reset");
    buffer.reset();
    expect(buffer.sample(0n)).toEqual([]);
    expect(buffer.push({ ...snapshot(0), zoneId: 2 })).toBe(true);
  });

  test("bounds history, handles large ticks, and drops obsolete movement after a stall", () => {
    const buffer = new SnapshotBuffer();
    for (let tick = 0; tick < 40; tick++) buffer.push(snapshot(tick));
    expect(buffer.sample(0n)[0]?.position[0]).toBe(8 * 12);
    // More than 30 s after the last projection: the buffer snaps instead of interpolating.
    const stalled = 40 + 30 * TICK_HZ;
    buffer.push(snapshot(stalled));
    expect(buffer.sample(39n)[0]?.position[0]).toBe(stalled * 12);
    buffer.reset();
    const tick = 18_446_744_073_709_550_000n;
    buffer.push(snapshot(tick, [player(1, 0)]));
    buffer.push(snapshot(tick + 2n, [player(1, 24)]));
    expect(buffer.sample(tick + 1n)[0]?.position[0]).toBe(12);
    expect(() => buffer.sample(tick, Number.NaN)).toThrow();
  });
});

describe("yaw conversion", () => {
  test("maps radians onto the shared u16 yaw convention", () => {
    expect(yawFromRadians(0)).toBe(0);
    expect(yawFromRadians(Math.PI / 2)).toBe(16_384);
    expect(yawFromRadians(Math.PI)).toBe(32_768);
    expect(yawFromRadians(-Math.PI / 2)).toBe(49_152);
    expect(yawFromRadians(2 * Math.PI - 1e-9)).toBe(0);
    expect(yawFromRadians(5 * Math.PI)).toBe(32_768);
  });
});
