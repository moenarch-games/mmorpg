import { describe, expect, test } from "bun:test";
import { panelViews } from "../src/world/units/panel-views";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

// What the panel offers is `mmorpg-client-model`'s `panels::waystone_offer` (#1280) over the hosted content, read with
// the projection's panel views; the outpost stone stands near (-550, 3050) units.

function snapshot(position: [number, number, number], waystones: number[], dead = false) {
  return testSnapshot({
    zoneId: 1, tick: 1n, contentRevision: 1n, acknowledgedSequence: 0, viewerId: 1,
    entities: [playerEntity(1, position)],
    viewer: { ...HEALTHY_VIEWER, waystones, dead, health: dead ? 0 : HEALTHY_VIEWER.health },
  });
}

describe("waystone panel (#145)", () => {
  test("offers the unlocked stones other than the one in reach", () => {
    const offer = panelViews(snapshot([-550, 90, 2_800], [1, 3])).waystone;
    expect(offer?.id).toBe(1);
    expect(offer?.destinations.map((site) => site.id)).toEqual([3]);
    expect(offer?.status).toMatch(/^\d+ undiscovered$/);
  });

  test("stays closed away from a stone or while dead", () => {
    expect(panelViews(snapshot([2_000, 90, 0], [1, 2])).waystone).toBeNull();
    expect(panelViews(snapshot([-550, 90, 2_800], [1, 2], true)).waystone).toBeNull();
  });
});
