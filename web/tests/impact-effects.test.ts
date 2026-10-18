import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { impactCues, impactFrameAt, parseImpactAtlas } from "../src/world/impact-effects";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";
import type { ZoneEvent } from "../src/replication";

const atlas = (name: string) => parseImpactAtlas(JSON.parse(readFileSync(new URL(`../public/effects/${name}/atlas.json`, import.meta.url), "utf8")));

describe("authored impact effects", () => {
  test("samples the shipped puff sequence by fixed age and expires it", () => {
    const puff = atlas("puff.strong");
    expect(puff.durationMs).toBe(600);
    expect(impactFrameAt(puff, -1)).toBe(-1);
    expect(impactFrameAt(puff, 0)).toBe(0);
    expect(impactFrameAt(puff, 59)).toBe(0);
    expect(impactFrameAt(puff, 60)).toBe(1);
    expect(impactFrameAt(puff, 250)).toBe(3);
    expect(impactFrameAt(puff, 599)).toBe(5);
    expect(impactFrameAt(puff, 600)).toBe(-1);
    for (const name of ["puff.subtle", "ring.strong", "ring.subtle"]) { expect(atlas(name).frames.length).toBe(6); }
  });

  test("starts only from confirmed hit and block events, on the struck unit", () => {
    const wolf = { kind: "creature", id: 9 } as const;
    const me = { kind: "player", id: 7 } as const;
    const events: ZoneEvent[] = [
      { kind: "damage-dealt", source: me, target: wolf, amount: 5, critical: false },
      { kind: "miss", source: me, target: wolf },
      { kind: "blocked", source: wolf, target: me, lane: "high", amount: 0 },
    ];
    const snapshot = testSnapshot({
      zoneId: 1, tick: 5n, contentRevision: 3n, acknowledgedSequence: 1, viewerId: 7,
      entities: [playerEntity(7, [0, 90, 0])], viewer: HEALTHY_VIEWER, events,
    });
    expect(impactCues(snapshot)).toEqual([{ family: "puff", target: wolf }, { family: "ring", target: me }]);
  });
});
