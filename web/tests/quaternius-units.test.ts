import { describe, expect, test } from "bun:test";
import type { ContentCatalog } from "../src/world/catalog";
import { ClipPlayback, QUATERNIUS_MODELS, classFromAppearance, quaterniusKind } from "../src/world/units/quaternius";

const catalog = {
  creatureTemplates: new Map([
    [1, { family: "wolf" }], [2, { family: "vermin" }], [3, { family: "boar" }],
    [4, { family: "marauder" }], [5, { family: "mirefin" }], [6, { family: "redbrand" }],
  ]),
  npcs: new Map([[10, { role: "guard" }], [11, { role: "vendor" }], [12, { role: "spirit_healer" }], [13, { role: "quest_giver" }]]),
} as unknown as ContentCatalog;
const viewer = { id: 1, classChoice: null };

describe("Quaternius unit mapping (mirrors native imported_asset)", () => {
  test("players by class and sex; sex code 0 is female", () => {
    expect(classFromAppearance(1)).toEqual({ classId: "warden", sex: "female" });
    const kinds = [1, 2, 3, 4, 5, 6, 0].map((appearance) => quaterniusKind({ kind: "player", entityId: 2, appearance }, catalog, viewer));
    expect(kinds).toEqual(["KnightFemale", "KnightMale", "NinjaFemale", "Elf", "Witch", "Wizard", "KnightMale"]);
    expect(quaterniusKind({ kind: "player", entityId: 1, appearance: 2 }, catalog, { id: 1, classChoice: { classId: "arcanist", sex: "female" } })).toBe("Witch");
  });

  test("creature families and NPC roles, female for odd IDs", () => {
    const creature = (appearance: number, entityId: number) => quaterniusKind({ kind: "creature", entityId, appearance }, catalog, viewer);
    expect([1, 2, 3].map((a) => creature(a, 7))).toEqual(["Wolf", "Rat", "Pig"]);
    expect([4, 5, 6].map((a) => creature(a, 7))).toEqual(["VikingFemale", "GoblinFemale", "PirateFemale"]);
    expect([4, 5, 6].map((a) => creature(a, 8))).toEqual(["VikingMale", "GoblinMale", "PirateMale"]);
    expect(creature(99, 8)).toBeNull();
    const npc = (entityId: number) => quaterniusKind({ kind: "npc", entityId, appearance: 0 }, catalog, viewer);
    expect([10, 11, 12, 13, 14].map(npc)).toEqual(["SoldierMale", "WorkerFemale", "Wizard", "CasualFemale", null]);
  });

  test("clip choice follows speed, attack and death; reduced motion freezes the clock", () => {
    const clips = QUATERNIUS_MODELS.KnightMale.clips;
    const duration = () => 1;
    const playback = new ClipPlayback();
    expect(playback.advance(clips, duration, 0, false, 0, 0.1, false).clip).toBe("Idle");
    expect(playback.advance(clips, duration, 1, false, 0, 0.1, false).clip).toBe("Walk");
    expect(playback.advance(clips, duration, 4, false, 0, 0.1, false).clip).toBe("Run");
    playback.attack(1, 0);
    expect(playback.advance(clips, duration, 4, false, 0.1, 0.1, false).clip).toBe("SwordSlash");
    playback.advance(clips, duration, 0, true, 1, 0.1, false);
    expect(playback.advance(clips, duration, 0, true, 5, 5, false)).toEqual({ clip: "Death", time: 1 });
    const frozen = new ClipPlayback();
    frozen.advance(clips, duration, 0, false, 0, 0.1, true);
    expect(frozen.advance(clips, duration, 0, false, 1, 0.5, true).time).toBe(0);
  });
});
