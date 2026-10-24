import { expect, test } from "bun:test";
import { ChordState, CHORD_TIMEOUT_MS, mergeHeld, stickIntent, touchContext } from "../src/input/touch-layout";
import { IDLE_INTENT } from "../src/world/orbit-camera";
import { HEALTHY_VIEWER, NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

test("single taps fire slots 1–4 and modifier chords reach slots 5–8", () => {
  const chord = new ChordState();
  expect(chord.press(0, 0)).toBe("ability.slot1");
  expect(chord.press(3, 0)).toBe("ability.slot4");
  chord.toggleModifier(10);
  expect(chord.page(10)).toBe(1);
  // Modifier, then face 2, fires ability 6; the chord returns to the first page.
  expect(chord.press(1, 20)).toBe("ability.slot6");
  expect(chord.press(1, 30)).toBe("ability.slot2");
  chord.toggleModifier(40);
  chord.toggleModifier(50);
  expect(chord.press(2, 60)).toBe("ability.slot3");
  chord.toggleModifier(100);
  expect(chord.press(0, 100 + CHORD_TIMEOUT_MS + 1)).toBe("ability.slot1");
});

test("the thumbstick quantizes into the wire's eight movement sectors", () => {
  expect(stickIntent(0, 0.1, false)).toEqual({ forward: 0, strafe: 0, steering: false, rise: 0 });
  expect(stickIntent(0, -1, false)).toMatchObject({ forward: 1, strafe: 0, steering: true });
  expect(stickIntent(0.7, 0.7, false)).toMatchObject({ forward: -1, strafe: 1 });
  expect(stickIntent(-0.9, 0.1, true)).toMatchObject({ forward: 0, strafe: -1, rise: 1 });
  const keys = { forward: 1 as const, strafe: 0 as const, steering: true, rise: 0 as const };
  expect(mergeHeld(keys, stickIntent(1, 0, false))).toEqual({ forward: 1, strafe: 1, steering: true, rise: 0 });
  expect(mergeHeld(IDLE_INTENT, IDLE_INTENT).steering).toBe(false);
});

test("context buttons follow nearby loot, NPCs, doors and opponents", () => {
  const base = { zoneId: 1, contentRevision: 5n, tick: 1n, acknowledgedSequence: 0, viewerId: 1 };
  const inputs = { isVendor: (id: number) => id === 7, doorInReach: false, repairInReach: false };
  const npc = (id: number, x: number) => ({ ...playerEntity(id, [x, 90, 0]), kind: "npc" as const });
  const wolf = { ...playerEntity(9, [1_000, 90, 0]), kind: "creature" as const, flags: { ...NO_FLAGS, attackable: true, hostile: true } };
  const corpse = { ...playerEntity(10, [200, 90, 0]), kind: "creature" as const, healthPercent: 0,
    flags: { ...NO_FLAGS, dead: true, lootable: true } };

  const idle = touchContext(testSnapshot({ ...base, entities: [playerEntity(1, [0, 90, 0])] }), { ...inputs, doorInReach: true });
  expect(idle).toEqual({ combat: false, dead: false, interact: "open", npc: null });
  const vendor = touchContext(testSnapshot({ ...base, entities: [playerEntity(1, [0, 90, 0]), npc(8, 450), npc(7, 300)] }), inputs);
  expect(vendor).toMatchObject({ interact: "vendor", npc: { kind: "npc", id: 7 } });
  const fight = touchContext(testSnapshot({ ...base, entities: [playerEntity(1, [0, 90, 0]), wolf, corpse, npc(8, 100)] }), inputs);
  expect(fight).toMatchObject({ combat: true, interact: "loot" });
  const dead = touchContext({ ...testSnapshot({ ...base, entities: [playerEntity(1, [0, 90, 0])] }),
    viewer: { ...HEALTHY_VIEWER, health: 0, dead: true } }, inputs);
  expect(dead).toEqual({ combat: false, dead: true, interact: null, npc: null });
});
