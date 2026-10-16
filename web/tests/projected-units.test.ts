import { describe, expect, test } from "bun:test";
import { nearestAnimal, type ProjectedUnit } from "../src/world/units/projected-units";

const unit = (id: number, kind: string, family: ProjectedUnit["family"], x: number, z: number): ProjectedUnit => ({ kind, id, family, x, z });

describe("nearestAnimal", () => {
  test("picks the closest wolf, boar or vermin and skips other units", () => {
    const units = [
      unit(1, "player", null, 0, 0),
      unit(2, "npc", null, 1, 1),
      unit(3, "creature", "marauder", 2, 2),
      unit(4, "creature", "wolf", -40, 0),
      unit(5, "creature", "boar", 10, 0),
      unit(6, "creature", "vermin", -20, 0),
    ];
    expect(nearestAnimal(units, 0, 0)?.id).toBe(5);
    expect(nearestAnimal(units, -35, 0)?.id).toBe(4);
  });

  test("returns null when no animal is projected", () => {
    expect(nearestAnimal([unit(1, "player", null, 0, 0), unit(3, "creature", "redbrand", 1, 1)], 0, 0)).toBeNull();
    expect(nearestAnimal([], 0, 0)).toBeNull();
  });

  test("breaks ties by the lower id", () => {
    expect(nearestAnimal([unit(9, "creature", "wolf", 5, 0), unit(8, "creature", "wolf", -5, 0)], 0, 0)?.id).toBe(8);
  });
});
