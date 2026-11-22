import { describe, expect, test } from "bun:test";
import { encodeCommand, encodeObservedCommand, type WorldCommand } from "../src/command-wire";
import { decodeSnapshot, ingestedProjection, ProjectionDecoder, type EntityState, type InventorySlot, type ZoneSnapshot } from "../src/replication";
import { NO_EQUIPMENT, NO_QUESTS, NO_STATS, playerEntity, testSnapshot } from "./support/snapshots";
import { encodeSnapshot } from "./support/wire";

const player = (entityId: number, x: number, facing = 0): EntityState => playerEntity(entityId, [x, 90, 0], [21, 0, 0], facing);
const snapshot = (tick: number | bigint, entities = [player(1, Number(tick) * 12)]): ZoneSnapshot => testSnapshot({
  zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, viewerId: 1, entities,
});

describe("the mmorpg-wasm wire boundary (#541)", () => {
  test("a projection crosses as the browser shape", () => {
    const sent = { ...snapshot(18_446_744_073_709_551_615n, [player(1, 0), player(2, 40)]), chat: [{ speaker: 2, channel: "emote" as const, emote: "wave" as const }] };
    const bytes = encodeSnapshot(sent);
    const decoded = decodeSnapshot(bytes);
    expect(decoded).toEqual({ ...sent, stats: null, party: null, depthsLoot: null });
    // A one-off decode belongs to no session: the unit presenter does not read it.
    expect(ingestedProjection(decoded)).toBeUndefined();
    // The Rust decoder still rejects what it always did, such as a projection that does not lead with its viewer.
    expect(() => decodeSnapshot(encodeSnapshot({ ...sent, entities: [player(2, 40), player(1, 0)] }))).toThrow("viewer");
    expect(() => decodeSnapshot(bytes.subarray(0, -1))).toThrow();
  });

  test("an online session keeps the sheets it received and echoes their revisions (#544)", () => {
    const sheets = new ProjectionDecoder();
    const jump = encodeCommand({ kind: "jump" });
    // Nothing held yet: the acknowledgement wraps the command with an empty `u16` held mask and no revisions (#897).
    const none = sheets.acknowledge(jump);
    expect(none.length).toBe(jump.length + 1 + 2);
    expect(none[1]).toBe(31);
    expect([...none.subarray(2, 4)]).toEqual([0, 0]);
    expect([...none.subarray(4)]).toEqual([...jump.subarray(1)]);
    const sent = snapshot(5);
    const bytes = encodeSnapshot(sent);
    const decoded = sheets.decode(bytes);
    expect(decoded).toEqual({ ...sent, stats: null, party: null, depthsLoot: null });
    expect(ingestedProjection(decoded)?.id).toBe(0);
    // Now it holds the sheets it received: the held mask names them and a nonzero revision follows for each.
    const held = sheets.acknowledge(jump);
    const mask = (held[2]! << 8) | held[3]!;
    const count = [...Array(16).keys()].filter((bit) => (mask >> bit) & 1).length;
    expect(count).toBeGreaterThan(0);
    expect(held.length).toBe(jump.length + 1 + 2 + 4 * count);
    for (let index = 0; index < count; index += 1) {
      expect([...held.subarray(4 + 4 * index, 8 + 4 * index)].some((byte) => byte !== 0)).toBe(true);
    }
    expect([...held.subarray(4 + 4 * count)]).toEqual([...jump.subarray(1)]);
  });

  test("a session's unchanged sheets stay in WASM and the views share the copy received first (#1009)", () => {
    const sheets = new ProjectionDecoder();
    const bag = (fur: number): InventorySlot[] => [{ itemId: 1, quantity: fur }, ...Array<null>(15).fill(null)];
    const withBag = (tick: number, fur: number, revision: bigint) => encodeSnapshot(testSnapshot({
      zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, viewerId: 1, entities: [player(1, tick)],
      inventoryRevision: revision, inventory: bag(fur), equipment: [...NO_EQUIPMENT], stats: NO_STATS, quests: NO_QUESTS,
    }));
    const first = sheets.decode(withBag(1, 3, 1n));
    const second = sheets.decode(withBag(2, 3, 1n));
    expect(second.inventory).toEqual(bag(3));
    // Unchanged: the second view was handed no sheets and reuses the objects the first one carried.
    expect(second.inventory).toBe(first.inventory);
    expect(second.equipment).toBe(first.equipment);
    expect(second.stats).toBe(first.stats);
    expect(second.quests).toBe(first.quests);
    const third = sheets.decode(withBag(3, 4, 2n));
    expect(third.inventory).toEqual(bag(4));
    expect(third.inventory).not.toBe(first.inventory);
    expect(third.quests).toBe(first.quests);
    expect(ingestedProjection(third)?.store).toBe(ingestedProjection(first)?.store);
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
      { kind: "use-ability", ability: 2 ** 32, target: null, facing: null },
      { kind: "emote", emote: "dance" },
      { kind: "directed-strike", ability: 1, facing: 0, aim: { side: "up", height: "low" } },
      { kind: "choose-class", classId: 0, sex: 256 },
      { kind: "use-waystone", waystone: 0 },
      { kind: "repair", piece: 2 ** 32 - 1 },
      { kind: "chat", channel: "say", text: " " },
    ]) {
      expect(() => encodeCommand(invalid as WorldCommand)).toThrow();
    }
    expect(() => encodeObservedCommand({ kind: "jump" }, -1n)).toThrow();
    expect(() => encodeObservedCommand({ kind: "jump" }, 1n << 64n)).toThrow();
    expect(encodeObservedCommand({ kind: "jump" }, 5n).byteLength).toBe(encodeCommand({ kind: "jump" }).byteLength + 9);
  });
});
