import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { decodeSnapshot, findEntity, SnapshotBuffer, yawFromRadians } from "../src/replication.ts";
import { NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots.ts";

const hex = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v14.hex", import.meta.url), "utf8").trim();
const fixture = Uint8Array.from(Buffer.from(hex, "hex"));
const player = (entityId, x, facing = 0) => playerEntity(entityId, [x, 90, 0], [21, 0, 0], facing);
const snapshot = (tick, entities = [player(1, Number(tick) * 12)]) => testSnapshot({
  zoneId: 1, tick: BigInt(tick), contentRevision: 7n, acknowledgedSequence: 9, viewerId: 1, entities,
});

const VIEWER = { kind: "player", id: 7 };
const WOLF = { kind: "creature", id: 108 };
/** Byte offsets of the fixture's sections (docs/PROTOCOL.md). */
const CLASS = 59;
const CAST = CLASS + 8;
const DAMAGE = CAST + 6;
const COOLDOWNS = DAMAGE + 4;
const AURAS = COOLDOWNS + 1 + 2 * 3;
const TARGET_OF_TARGET = AURAS + 1 + 6;
const TARGET_CAST = TARGET_OF_TARGET + 5;
const TARGET_AURAS = TARGET_CAST + 6;
const INVENTORY = TARGET_AURAS + 1 + 6;
const EQUIPMENT = INVENTORY + 9 + 64;
const STATS = EQUIPMENT + 12;
/** The quest part ends the sheet: mask, one entry, two markers (16 bytes). */
const QUESTS = STATS + 8;
const EVENTS = INVENTORY + 8 + 1 + 84 + 16 + 1;
/** Two chat lines and an emote follow the sixteen events: 6 + 16, 6 + 8 and 6 bytes. */
const CHAT = EVENTS + 1 + 16 * 14;
const EMOTE = CHAT + 1 + 22 + 14;
const ENTITY_COUNT = EMOTE + 6;
const FIRST_ENTITY = ENTITY_COUNT + 2;

describe("Rust/browser snapshot contract", () => {
  test("validates bag slots, sheet presence and revisions, including the legacy v6 fixture", () => {
    const legacy = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v6.hex", import.meta.url), "utf8").trim();
    expect(() => decodeSnapshot(Uint8Array.from(Buffer.from(legacy, "hex")))).toThrow("version");
    const zeroRevision = fixture.slice();
    zeroRevision.fill(0, INVENTORY, INVENTORY + 8);
    expect(() => decodeSnapshot(zeroRevision)).toThrow("revision");
    const invalidFlag = fixture.slice();
    invalidFlag[INVENTORY + 8] = 2;
    expect(() => decodeSnapshot(invalidFlag)).toThrow("Reserved");
    for (const [slot, item, quantity] of [[0, 0, 3], [0, 11, 1], [0, 1, 0], [0, 1, 21], [1, 2, 2]]) {
      const bytes = fixture.slice();
      const view = new DataView(bytes.buffer);
      view.setUint16(INVENTORY + 9 + slot * 4, item);
      view.setUint16(INVENTORY + 11 + slot * 4, quantity);
      expect(() => decodeSnapshot(bytes)).toThrow("inventory stack");
    }
    const omitted = new Uint8Array([...fixture.slice(0, INVENTORY + 8), 0, ...fixture.slice(INVENTORY + 9 + 84 + 16)]);
    expect(decodeSnapshot(omitted)).toMatchObject({ inventoryRevision: 9n, inventory: null, equipment: null, stats: null, quests: null });
  });
  test("validates equipment slots and stat totals beside the bag", () => {
    for (const [slot, item, message] of [[0, 10, "equipment slot"], [0, 11, "equipment slot"], [1, 6, "equipment slot"]]) {
      const bytes = fixture.slice();
      new DataView(bytes.buffer).setUint16(EQUIPMENT + 2 * slot, item);
      expect(() => decodeSnapshot(bytes), `slot ${slot} = item ${item}`).toThrow(message);
    }
    const stale = fixture.slice();
    new DataView(stale.buffer).setUint16(EQUIPMENT, 0);
    expect(() => decodeSnapshot(stale)).toThrow("Stat totals");
    for (let stat = 0; stat < 4; stat += 1) {
      const bytes = fixture.slice();
      bytes[STATS + 2 * stat + 1] ^= 1;
      expect(() => decodeSnapshot(bytes)).toThrow("Stat totals");
    }
    const inverted = fixture.slice();
    new DataView(inverted.buffer).setUint16(DAMAGE, 13);
    expect(() => decodeSnapshot(inverted)).toThrow("Inverted");
  });
  test("rejects invalid progression and the actual legacy v5 fixture", () => {
    const legacy = readFileSync(new URL("../../fixtures/protocol/player-snapshot-v5.hex", import.meta.url), "utf8").trim();
    expect(() => decodeSnapshot(Uint8Array.from(Buffer.from(legacy, "hex")))).toThrow("version");
    for (const [level, experience, threshold] of [[1, 100, 100], [1, 0, 0], [10, 1, 0], [10, 0, 100], [11, 0, 0]]) {
      const bytes = fixture.slice();
      const view = new DataView(bytes.buffer);
      view.setUint8(40, level);
      view.setUint32(41, experience);
      view.setUint32(45, threshold);
      expect(() => decodeSnapshot(bytes)).toThrow("Inconsistent");
    }
  });
  test("decodes the same golden bytes as the Rust encoder", () => {
    expect(fixture.length).toBe(105 + 2 * 3 + 2 * 6 + 84 + 16 + 16 * 14 + 22 + 14 + 6 + 4 * 21);
    expect(decodeSnapshot(fixture)).toEqual({
      zoneId: 42, tick: 99n, contentRevision: 4n, acknowledgedSequence: 81, viewerId: 7,
      viewer: {
        copper: 42, experience: 37, experienceToNextLevel: 400, health: 38, maxHealth: 110, level: 4, dead: false, inCombat: true,
        autoAttacking: true, target: WOLF, classChoice: { classId: "arcanist", sex: "female" },
        resource: { kind: "mana", value: 121, max: 176 }, cast: { ability: 9, elapsed: 20, total: 60, channel: false }, globalCooldown: 25,
        damage: { min: 9, max: 12 },
      },
      cooldowns: [{ ability: 10, remaining: 412 }, { ability: 11, remaining: 700 }],
      auras: [{ ability: 11, kind: "absorb", remaining: 500, amount: 31 }],
      targetOfTarget: VIEWER,
      targetDetail: {
        cast: { ability: 13, elapsed: 10, total: 45, channel: false },
        auras: [{ ability: 10, kind: "root", remaining: 150, amount: 0 }],
      },
      inventoryRevision: 9n,
      inventory: [{ itemId: 1, quantity: 3 }, { itemId: 2, quantity: 1 }, ...Array(13).fill(null), { itemId: 1, quantity: 20 }],
      equipment: [4, null, 6, 7, null, null],
      stats: { stamina: 3, strength: 0, agility: 0, intellect: 6 },
      quests: {
        completed: 1,
        entries: [{ quest: 2, progress: [3, 0, 0] }],
        markers: [{ npc: 1, marker: "available" }, { npc: 2, marker: "in-progress" }],
      },
      loot: null,
      events: [
        { kind: "damage-dealt", source: VIEWER, target: WOLF, amount: 7, critical: true },
        { kind: "damage-taken", source: WOLF, target: VIEWER, amount: 3, critical: false },
        { kind: "miss", source: VIEWER, target: WOLF },
        { kind: "evade", source: VIEWER, target: WOLF },
        { kind: "died", entity: WOLF, killer: VIEWER },
        { kind: "error", code: "out-of-range", target: WOLF },
        { kind: "error", code: "not-enough-resource", target: null },
        { kind: "cast-started", source: WOLF, target: VIEWER, ability: 13, ticks: 45 },
        { kind: "ability-used", source: VIEWER, target: null, ability: 10 },
        { kind: "healed", source: VIEWER, target: VIEWER, ability: 3, amount: 2 },
        { kind: "aura-applied", source: VIEWER, target: WOLF, ability: 10, ticks: 180 },
        { kind: "aura-removed", source: VIEWER, target: VIEWER, ability: 11 },
        { kind: "interrupted", source: null, target: VIEWER, ability: 9 },
        { kind: "absorbed", source: WOLF, target: VIEWER, amount: 5 },
        { kind: "quest-progress", quest: 2, objective: 0, count: 3 },
        { kind: "quest-completed", quest: 1 },
      ],
      chat: [
        { speaker: 7, channel: "say", text: "Hail, Greyhaven!" },
        { speaker: 4_000_000_000, channel: "yell", text: "Grüße!" },
        { speaker: 9, channel: "emote", emote: "cheer" },
      ],
      entities: [
        {
          kind: "player", entityId: 7, appearance: 5, position: [10, 90, -30], velocity: [1, -2, 3], facing: 16_384,
          level: 1, healthPercent: 40, flags: { ...NO_FLAGS, inCombat: true },
        },
        {
          kind: "creature", entityId: 108, appearance: 5, position: [-845, 45, 2_500], velocity: [-13, 0, -128],
          facing: 49_152, level: 2, healthPercent: 43,
          flags: { ...NO_FLAGS, inCombat: true, hostile: true, attackable: true, targetsViewer: true },
        },
        {
          kind: "npc", entityId: 5, appearance: 5, position: [-1_650, 90, 4_550], velocity: [0, 0, 0], facing: 49_152,
          level: 10, healthPercent: 100, flags: NO_FLAGS,
        },
        {
          kind: "creature", entityId: 109, appearance: 1, position: [-900, 45, 2_600], velocity: [0, 0, 0], facing: 0,
          level: 1, healthPercent: 0, flags: { ...NO_FLAGS, dead: true, hostile: true, tappedByOther: true },
        },
      ],
    });
    const offsetBuffer = new Uint8Array(fixture.length + 4);
    offsetBuffer.set(fixture, 2);
    expect(decodeSnapshot(offsetBuffer.subarray(2, -2))).toEqual(decodeSnapshot(fixture));
    expect(findEntity(decodeSnapshot(fixture), WOLF)?.level).toBe(2);
    expect(findEntity(decodeSnapshot(fixture), { kind: "creature", id: 7 })).toBeUndefined();
  });

  test("decodes every error code of the Rust wire in order", () => {
    const codes = [
      "no-target", "out-of-range", "target-dead", "not-attackable", "you-are-dead", "not-dead", "invalid-target",
      "too-many-intents", "invalid-inventory-move", "inventory-full",
      "invalid-loot", "not-loot-owner", "empty-loot", "money-overflow",
      "no-class", "not-learned", "not-ready", "not-enough-resource", "stunned", "already-casting", "invalid-class",
      "not-equippable", "invalid-vendor", "not-enough-money", "chat-throttled", "invalid-quest", "quest-log-full",
      "quest-incomplete",
    ];
    codes.forEach((code, index) => {
      const bytes = fixture.slice();
      bytes[EVENTS + 1 + 6 * 14 + 13] = index + 1;
      expect(decodeSnapshot(bytes).events[6]).toEqual({ kind: "error", code, target: null });
    });
  });

  test("rejects legacy, canonical, truncated, excessive, malformed and trailing payloads", () => {
    for (let length = 0; length < fixture.length; length++) {
      expect(() => decodeSnapshot(fixture.slice(0, length))).toThrow();
    }
    const wolfRecord = FIRST_ENTITY + 21;
    for (const [offset, value, message] of [
      [0, 4, "version"],
      [1, 1, "player-visible"],
      [3, 4, "version"],
      [53, 0b1000, "Reserved"],
      [53, 0b111, "Inconsistent"],
      [54, 9, "kind"],
      [CLASS, 7, "ability state"],
      [CLASS, 1, "resource"],
      [CLASS + 1, 1, "resource"],
      [CLASS + 4, 0xff, "resource"],
      [CLASS + 7, 46, "ability state"],
      [CAST, 1, "cast state"],
      [CAST + 1, 1, "cast state"],
      [CAST + 1, 2, "Reserved"],
      [CAST + 3, 60, "cast state"],
      [COOLDOWNS, 5, "too many cooldowns"],
      [COOLDOWNS + 1, 11, "cooldown record"],
      [COOLDOWNS + 2, 9, "cooldown record"],
      [AURAS, 9, "too many auras"],
      [AURAS + 1, 9, "aura record"],
      [AURAS + 2, 4, "aura record"],
      [AURAS + 3, 9, "aura record"],
      [TARGET_OF_TARGET, 0, "absent"],
      [TARGET_CAST, 0, "cast state"],
      [TARGET_AURAS + 4, 0, "aura record"],
      [EVENTS, 17, "event capacity"],
      [EVENTS + 1, 16, "event kind"],
      [EVENTS + 1 + 14 * 14 + 1, 0, "Quest id"],
      [EVENTS + 1 + 14 * 14 + 12, 3, "Malformed"],
      [EVENTS + 1 + 14 * 14 + 13, 0, "Malformed"],
      [EVENTS + 1 + 15 * 14 + 1, 33, "Quest id"],
      [EVENTS + 1 + 15 * 14 + 2, 1, "Malformed"],
      [QUESTS + 4, 11, "Quest log exceeds"],
      [QUESTS + 5, 1, "Inconsistent quest log"],
      [QUESTS + 9, 9, "too many markers"],
      [QUESTS + 12, 4, "Inconsistent quest marker"],
      [QUESTS + 14, 1, "Inconsistent quest marker"],
      [EVENTS + 1 + 7 * 14 + 1, 15, "Unknown ability"],
      [EVENTS + 1 + 7 * 14 + 1, 0, "Unknown ability"],
      [EVENTS + 1 + 8 * 14 + 13, 1, "Malformed"],
      [EVENTS + 1 + 13 * 14 + 1, 1, "flags"],
      [EVENTS + 2, 2, "flags"],
      [EVENTS + 1 + 2 * 14 + 1, 1, "flags"],
      [EVENTS + 1 + 6 * 14 + 13, 99, "error code"],
      [ENTITY_COUNT, 255, "count"],
      [CHAT, 5, "chat capacity"],
      [CHAT + 5, 3, "chat channel"],
      [EMOTE + 5, 0, "Unknown emote"],
      [EMOTE + 5, 6, "Unknown emote"],
      [CHAT + 7, 0x0a, "chat text"],
      [FIRST_ENTITY, 0, "kind"],
      [FIRST_ENTITY, 4, "kind"],
      [wolfRecord + 19, 101, "Health percent"],
      [wolfRecord + 20, 0x80, "Lootable"],
      [31, 9, "viewer"],
    ]) {
      const invalid = fixture.slice();
      invalid[offset] = value;
      expect(() => decodeSnapshot(invalid), `byte ${offset} = ${value}`).toThrow(message);
    }
    const fewer = fixture.slice();
    new DataView(fewer.buffer).setUint16(ENTITY_COUNT, 3);
    expect(() => decodeSnapshot(fewer)).toThrow("count");
    expect(() => decodeSnapshot(new Uint8Array([...fixture, 0]))).toThrow();
    expect(() => decodeSnapshot(new Uint8Array(1_078))).toThrow("budget");
    for (const version of [9, 13]) {
      const legacy = readFileSync(new URL(`../../fixtures/protocol/player-snapshot-v${version}.hex`, import.meta.url), "utf8").trim();
      expect(() => decodeSnapshot(Uint8Array.from(Buffer.from(legacy, "hex")))).toThrow("version");
    }
  });

  test("requires the viewer to lead the priority-ordered records", () => {
    const swapped = fixture.slice();
    swapped.set(fixture.slice(FIRST_ENTITY + 21, FIRST_ENTITY + 42), FIRST_ENTITY);
    swapped.set(fixture.slice(FIRST_ENTITY, FIRST_ENTITY + 21), FIRST_ENTITY + 21);
    expect(() => decodeSnapshot(swapped)).toThrow("viewer");
    const otherViewer = fixture.slice();
    new DataView(otherViewer.buffer).setUint32(28, 9);
    expect(() => decodeSnapshot(otherViewer)).toThrow("viewer");
  });

  test("preserves 64-bit ticks without floating point rounding", () => {
    const encoded = fixture.slice();
    new DataView(encoded.buffer).setBigUint64(8, 18_446_744_073_709_551_615n);
    expect(decodeSnapshot(encoded).tick).toBe(18_446_744_073_709_551_615n);
  });

  test("rejects duplicate entity identities", () => {
    const encoded = new Uint8Array(fixture.length + 21);
    encoded.set(fixture);
    encoded.set(fixture.slice(FIRST_ENTITY, FIRST_ENTITY + 21), fixture.length);
    new DataView(encoded.buffer).setUint16(ENTITY_COUNT, 5);
    expect(() => decodeSnapshot(encoded)).toThrow("Duplicate");
  });
});

describe("snapshot presentation", () => {
  test("interpolates irregular server ticks and holds during packet loss", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(10));
    buffer.push(snapshot(13));
    expect(buffer.sample(11n, 0.5)[0].position).toEqual([138, 90, 0]);
    expect(buffer.sample(100n)[0].position).toEqual([156, 90, 0]);
    expect(buffer.push(snapshot(12))).toBe(false);
    expect(buffer.push(snapshot(13))).toBe(false);
    expect(buffer.sample(13n)[0].position).toEqual([156, 90, 0]);
  });

  test("interpolates facing along the shorter arc", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(0, [player(1, 0, 65_000)]));
    buffer.push(snapshot(2, [player(1, 0, 500)]));
    expect(buffer.sample(1n)[0].facing).toBe(65_518);
    expect(buffer.sample(0n, 0.5)[0].facing).toBe(65_259);
    buffer.reset();
    buffer.push(snapshot(0, [player(1, 0, 500)]));
    buffer.push(snapshot(2, [player(1, 0, 65_000)]));
    expect(buffer.sample(1n)[0].facing).toBe(65_518);
    expect(buffer.sample(2n)[0].facing).toBe(65_000);
  });

  test("appearance and removal occur at authoritative ticks", () => {
    const buffer = new SnapshotBuffer();
    buffer.push(snapshot(1, [player(1, 0), player(2, 10)]));
    buffer.push(snapshot(2, [player(1, 12), player(3, 20)]));
    expect(buffer.sample(1n, 0.5).map(p => p.entityId)).toEqual([1, 2]);
    expect(buffer.sample(2n).map(p => p.entityId)).toEqual([1, 3]);
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
    expect(buffer.sample(0n)[0].position[0]).toBe(8 * 12);
    buffer.push(snapshot(1000));
    expect(buffer.sample(39n)[0].position[0]).toBe(12_000);
    buffer.reset();
    const tick = 18_446_744_073_709_550_000n;
    buffer.push(snapshot(tick, [player(1, 0)]));
    buffer.push(snapshot(tick + 2n, [player(1, 24)]));
    expect(buffer.sample(tick + 1n)[0].position[0]).toBe(12);
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
