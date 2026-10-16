import { expect, test } from "bun:test";
import type { Vector3 } from "../src/replication";
import { lootTarget } from "../src/world/units/loot-panel";
import { HEALTHY_VIEWER, NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

function corpse(id: number, position: Vector3, owned = true) {
  return { ...playerEntity(id, position), kind: "creature" as const, appearance: 1, healthPercent: 0,
    flags: { ...NO_FLAGS, dead: true, lootable: owned, tappedByOther: !owned } };
}

function snapshot() {
  return testSnapshot({ zoneId: 1, contentRevision: 5n, tick: 1n, acknowledgedSequence: 0, viewerId: 1,
    entities: [playerEntity(1, [0, 90, 0]), corpse(108, [500, 90, 0]), corpse(109, [-500, 90, 0]), corpse(110, [10, 90, 0], false)] });
}

test("Loot selects the nearest received owned corpse with stable ID tie-breaking", () => {
  expect(lootTarget(snapshot())).toEqual({ kind: "creature", id: 108 });
  // Selection is allowed outside claim reach; only core can expose an eligible sheet.
  expect(snapshot().loot).toBeNull();
});

test("an already selected owned corpse retains priority", () => {
  expect(lootTarget({ ...snapshot(), viewer: { ...HEALTHY_VIEWER, target: { kind: "creature", id: 109 } } }))
    .toEqual({ kind: "creature", id: 109 });
});

test("living units, unowned corpses, missing viewer and a dead viewer cannot become loot selections", () => {
  expect(lootTarget({ ...snapshot(), entities: [playerEntity(1, [0, 90, 0]), playerEntity(2, [10, 90, 0]), corpse(110, [10, 90, 0], false)] })).toBeNull();
  expect(lootTarget({ ...snapshot(), entities: [] })).toBeNull();
  expect(lootTarget({ ...snapshot(), viewer: { ...HEALTHY_VIEWER, health: 0, dead: true } })).toBeNull();
});

test("presentation selection orders full 3D centre distance without deciding claim reach", () => {
  expect(lootTarget({ ...snapshot(), entities: [...snapshot().entities, corpse(107, [0, 1000, 0])] }))
    .toEqual({ kind: "creature", id: 108 });
});
