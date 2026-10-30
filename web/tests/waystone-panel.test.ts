import { describe, expect, test } from "bun:test";
import { waystoneOffer } from "../src/world/units/waystone-panel";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

const SITES = [{ id: 1, name: "Greyhaven Outpost" }, { id: 2, name: "Seam Test West" }, { id: 3, name: "Seam Test East" }];
const atOutpost = (x: number, z: number) => (Math.hypot(x + 550, z - 3_050) <= 800 ? 1 : 0);

function snapshot(position: [number, number, number], waystones: number[], dead = false) {
  return testSnapshot({
    zoneId: 1, tick: 1n, contentRevision: 1n, acknowledgedSequence: 0, viewerId: 1,
    entities: [playerEntity(1, position)],
    viewer: { ...HEALTHY_VIEWER, waystones, dead, health: dead ? 0 : HEALTHY_VIEWER.health },
  });
}

describe("waystone panel (#145)", () => {
  test("offers the unlocked stones other than the one in reach", () => {
    const offer = waystoneOffer(snapshot([-550, 90, 2_800], [1, 3]), atOutpost, SITES);
    expect(offer?.here.name).toBe("Greyhaven Outpost");
    expect(offer?.destinations.map((site) => site.id)).toEqual([3]);
    expect(offer?.undiscovered).toBe(1);
  });

  test("stays closed away from a stone or while dead", () => {
    expect(waystoneOffer(snapshot([2_000, 90, 0], [1, 2]), atOutpost, SITES)).toBeNull();
    expect(waystoneOffer(snapshot([-550, 90, 2_800], [1, 2], true), atOutpost, SITES)).toBeNull();
  });
});
