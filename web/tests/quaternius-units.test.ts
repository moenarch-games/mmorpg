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

describe("imported unit mapping (mirrors native imported_asset)", () => {
  test("every player is the Ranger humanoid while it is the only character bundle", () => {
    expect(classFromAppearance(1)).toEqual({ classId: "warden", sex: "female" });
    const kinds = [1, 2, 3, 4, 5, 6, 0].map((appearance) => quaterniusKind({ kind: "player", entityId: 2, appearance }, catalog));
    expect(kinds).toEqual(Array(7).fill("Ranger"));
  });

  test("animal families use Quaternius animals; humanoid creatures and NPCs stay procedural", () => {
    const creature = (appearance: number) => quaterniusKind({ kind: "creature", entityId: 7, appearance }, catalog);
    expect([1, 2, 3].map(creature)).toEqual(["Wolf", "Rat", "Pig"]);
    expect([4, 5, 6, 99].map(creature)).toEqual([null, null, null, null]);
    const npc = (entityId: number) => quaterniusKind({ kind: "npc", entityId, appearance: 0 }, catalog);
    expect([10, 11, 12, 13, 14].map(npc)).toEqual([null, null, null, null, null]);
  });

  test("clip choice follows speed, attack and death; reduced motion freezes the clock", () => {
    const clips = QUATERNIUS_MODELS.Ranger.clips;
    const duration = () => 1;
    const playback = new ClipPlayback();
    expect(playback.advance(clips, duration, 0, false, 0, 0.1, false).clip).toBe("Idle_Loop");
    expect(playback.advance(clips, duration, 1, false, 0, 0.1, false).clip).toBe("Walk_Loop");
    expect(playback.advance(clips, duration, 4, false, 0, 0.1, false).clip).toBe("Jog_Fwd_Loop");
    playback.attack(1, 0);
    expect(playback.advance(clips, duration, 4, false, 0.1, 0.1, false).clip).toBe("Sword_Attack");
    playback.advance(clips, duration, 0, true, 1, 0.1, false);
    expect(playback.advance(clips, duration, 0, true, 5, 5, false)).toEqual({ clip: "Death01", time: 1 });
    const frozen = new ClipPlayback();
    frozen.advance(clips, duration, 0, false, 0, 0.1, true);
    expect(frozen.advance(clips, duration, 0, false, 1, 0.5, true).time).toBe(0);
  });
});
