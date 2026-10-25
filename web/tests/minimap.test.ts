import { describe, expect, test } from "bun:test";
import { ENVIRONMENT } from "../src/world/environment";
import { buildMinimapLayers, clampToRim, dispositionOf, minimapOffset, minimapRotation } from "../src/world/minimap";
import type { EntityState } from "../src/replication";
import { fixtureScenery } from "./support/scenery-fixture";
import { NO_FLAGS, playerEntity } from "./support/snapshots";

describe("minimap projection", () => {
  test("north-up maps north (−Z) up and east (+X) right", () => {
    expect(minimapOffset(0, -10, 1.2, false, 2)).toEqual([0, -20]);
    expect(minimapOffset(5, 0, 1.2, false, 2)).toEqual([10, 0]);
    expect(minimapRotation(1.2, false)).toBe(0);
  });

  test("heading-up puts the camera heading up and the view's right on the right", () => {
    for (const heading of [0, 0.7, Math.PI / 2, 2.5, Math.PI, 5.9]) {
      const [ux, uy] = minimapOffset(Math.sin(heading), Math.cos(heading), heading, true, 1);
      expect(ux).toBeCloseTo(0, 9);
      expect(uy).toBeCloseTo(-1, 9);
      // The view's right is heading − 90°: (−cos, sin).
      const [rx, ry] = minimapOffset(-Math.cos(heading), Math.sin(heading), heading, true, 1);
      expect(rx).toBeCloseTo(1, 9);
      expect(ry).toBeCloseTo(0, 9);
    }
    // The canvas rotation applies the same mapping.
    const heading = 0.9;
    const angle = minimapRotation(heading, true);
    const [x, y] = [3, -4];
    const rotated = [x * Math.cos(angle) - y * Math.sin(angle), x * Math.sin(angle) + y * Math.cos(angle)];
    const direct = minimapOffset(x, y, heading, true, 1);
    expect(rotated[0]).toBeCloseTo(direct[0], 9);
    expect(rotated[1]).toBeCloseTo(direct[1], 9);
  });

  test("dots beyond the rim move onto it, others stay", () => {
    expect(clampToRim([3, 4], 10)).toEqual([3, 4]);
    const [x, y] = clampToRim([30, 40], 10);
    expect(x).toBeCloseTo(6, 9);
    expect(y).toBeCloseTo(8, 9);
  });

  test("dots take the unit's disposition: players, hostile or neutral creatures, friendly NPCs", () => {
    const creature = (hostile: boolean): EntityState => ({ ...playerEntity(9, [0, 45, 0]), kind: "creature", appearance: 1, flags: { ...NO_FLAGS, hostile } });
    expect(dispositionOf(playerEntity(2, [0, 90, 0]))).toBe("player");
    expect(dispositionOf(creature(true))).toBe("hostile");
    expect(dispositionOf(creature(false))).toBe("neutral");
    expect(dispositionOf({ ...playerEntity(6, [0, 90, 0]), kind: "npc", appearance: 6 })).toBe("friendly");
  });
});

describe("minimap layers", () => {
  test("are built once from scenery, deterministically, as opaque pixels plus vectors", () => {
    const scenery = fixtureScenery();
    const layers = buildMinimapLayers(scenery, ENVIRONMENT);
    // The Vale and the farmland south of it (#424), 2 px per metre.
    expect([layers.width, layers.height]).toEqual([848, 1_712]);
    expect(layers.terrain.length).toBe(layers.width * layers.height * 4);
    for (let offset = 3; offset < layers.terrain.length; offset += 4 * 97) {
      expect(layers.terrain[offset]).toBe(255);
    }
    expect(layers.roads).toHaveLength(1);
    expect(layers.roads[0]!.width).toBeCloseTo(3, 9);
    expect(layers.water[0]!.center).toEqual([6, -12]);
    expect(layers.trees.length).toBe(3);
    expect(layers.buildings.length).toBeGreaterThan(5);
    expect(buildMinimapLayers(scenery, ENVIRONMENT)).toEqual(layers);
  });
});
