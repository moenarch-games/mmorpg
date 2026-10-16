import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { decodeSnapshot } from "../src/replication";

const bytes = Uint8Array.from(Buffer.from(readFileSync(new URL("../../fixtures/protocol/player-loot-v14.hex", import.meta.url), "utf8").trim(), "hex"));

describe("Rust/browser corpse loot contract", () => {
  test("reads the complete fenced sheet and repeated balance from Rust's golden bytes", () => {
    const projection = decodeSnapshot(bytes);
    expect(projection.viewer.copper).toBe(42);
    expect(projection.loot).toEqual({
      creatureId: 108, diedAt: 98n, money: 2, item: { itemId: 1, quantity: 2 }, questItem: { itemId: 10, quantity: 1 },
    });
    expect(projection.entities[1]?.flags).toMatchObject({ dead: true, lootable: true, tappedByOther: false });
    const offset = new Uint8Array(bytes.length + 4);
    offset.set(bytes, 2);
    expect(decodeSnapshot(offset.subarray(2, -2))).toEqual(projection);
  });

  test("rejects malformed presence, items, death fences and corpse flags", () => {
    for (let length = 0; length < bytes.length; length += 1) {
      expect(() => decodeSnapshot(bytes.subarray(0, length))).toThrow();
    }
    // Header and self (59), self abilities with the damage range, two cooldowns and one aura (32),
    // target section without detail auras (12), sheet revision, presence and sheet (93) with its
    // 16-byte quest part.
    const presence = 59 + 32 + 12 + 93 + 16;
    for (const [offset, value] of [
      [presence, 2], [presence + 4, 109], [presence + 12, 100], [presence + 17, 2], [presence + 19, 9], [presence + 21, 0],
      [presence + 22, 2], [presence + 24, 11], [presence + 26, 0], [presence + 26, 2],
    ]) {
      if (offset === undefined || value === undefined) {
        throw new Error("Missing malformed fixture field");
      }
      const invalid = bytes.slice();
      invalid[offset] = value;
      expect(() => decodeSnapshot(invalid)).toThrow();
    }
    // The 26-byte corpse sheet, 16 events, two chat lines and an emote, the entity count and the viewer.
    const corpse = presence + 1 + 26 + 1 + 16 * 14 + 1 + 22 + 14 + 6 + 2 + 21;
    for (const flags of [0x05, 0x84, 0x95, 0x8d]) {
      const invalid = bytes.slice();
      invalid[corpse + 20] = flags;
      expect(() => decodeSnapshot(invalid)).toThrow();
    }
    const legacy = Uint8Array.from(Buffer.from(readFileSync(new URL("../../fixtures/protocol/player-snapshot-v7.hex", import.meta.url), "utf8").trim(), "hex"));
    expect(() => decodeSnapshot(legacy)).toThrow("version");
    const v8 = Uint8Array.from(Buffer.from(readFileSync(new URL("../../fixtures/protocol/player-loot-v8.hex", import.meta.url), "utf8").trim(), "hex"));
    expect(() => decodeSnapshot(v8)).toThrow("version");
    for (const version of [9, 13]) {
      const legacyLoot = Uint8Array.from(Buffer.from(readFileSync(new URL(`../../fixtures/protocol/player-loot-v${version}.hex`, import.meta.url), "utf8").trim(), "hex"));
      expect(() => decodeSnapshot(legacyLoot)).toThrow("version");
    }
  });
});
