import { describe, expect, test } from "bun:test";
import { ArtPrefetch } from "../src/world/art-prefetch";
import { isOutpostArea, kitNearArea, kitNearPoint, shelteredOutpost } from "../src/world/crater-streaming";
import type { PlacedKitSource } from "../src/world/placed-kit";

const outpost = { id: 1, name: "Greyhaven Outpost", minXz: [-3500, -1300], maxXz: [3500, 5300] } as const;
const farm = { id: 3, name: "Millbrook Farm", minXz: [4000, 1300], maxXz: [10500, 9000] } as const;

test("the Outpost screens adjacent region art until a player approaches its edge", () => {
  expect(isOutpostArea(outpost)).toBe(true);
  expect(shelteredOutpost([0, 2000], [outpost, farm])?.id).toBe(1);
  expect(shelteredOutpost([2600, 2000], [outpost, farm])).toBeNull();
  expect(shelteredOutpost([4000, 2000], [outpost, farm])).toBeNull();
  expect(shelteredOutpost([0, 2000], [farm])).toBeNull();
});

test("placed kit downloads include near-rim art, not unrelated distant placements", () => {
  const placement = (x: number, z: number) => {
    const matrix = new Float32Array(16);
    matrix[12] = x;
    matrix[14] = z;
    return { key: `${x}:${z}`, matrix };
  };
  const source = (x: number, z: number): PlacedKitSource => ({
    kit: "nature", entries: [], placements: [placement(x, z)],
    lods: () => { throw new Error("not used in proximity tests"); },
  });
  expect(kitNearArea(source(45, 20), outpost, 100, 5000)).toBe(true);
  expect(kitNearArea(source(170, 20), outpost, 100, 5000)).toBe(false);
  expect(kitNearPoint(source(45, 20), [0, 2000], 100, 6000)).toBe(true);
  expect(kitNearPoint(source(170, 20), [0, 2000], 100, 6000)).toBe(false);
  expect(kitNearPoint(source(170, 20), [12000, 2000], 100, 6000)).toBe(true);
});

/** A prefetch whose loads settle only when the test resolves them. */
function harness() {
  const started: string[] = [];
  const pending = new Map<string, () => void>();
  let idle = 0;
  const prefetch = new ArtPrefetch((key) => {
    started.push(key);
    return new Promise<void>((resolve) => pending.set(key, resolve));
  }, (isIdle) => { if (isIdle) { idle += 1; } });
  const settle = async (key: string) => {
    pending.get(key)!();
    pending.delete(key);
    for (let i = 0; i < 5; i += 1) { await Promise.resolve(); }
  };
  const flush = async () => { for (let i = 0; i < 5; i += 1) { await Promise.resolve(); } };
  return { prefetch, started, settle, flush, idle: () => idle };
}

describe("ArtPrefetch (#412)", () => {
  test("foreground starts at once; background waits for it, then runs one at a time", async () => {
    const { prefetch, started, settle, flush } = harness();
    prefetch.plan({ foreground: ["Ranger", "inn"], background: ["Wolf", "Rat"] });
    await flush();
    expect(started).toEqual(["Ranger", "inn"]);
    await settle("Ranger");
    expect(started).toEqual(["Ranger", "inn"]);
    await settle("inn");
    expect(started).toEqual(["Ranger", "inn", "Wolf"]);
    await settle("Wolf");
    expect(started).toEqual(["Ranger", "inn", "Wolf", "Rat"]);
  });

  test("a new plan drops stale queued work and never starts a key twice", async () => {
    const { prefetch, started, settle, flush, idle } = harness();
    prefetch.plan({ foreground: ["Ranger"], background: ["Wolf", "Rat"] });
    prefetch.plan({ foreground: ["Ranger", "Pig"], background: ["Ranger", "Rat"] });
    await flush();
    expect(started).toEqual(["Ranger", "Pig"]);
    expect(prefetch.queued).toEqual(["Rat"]);
    await settle("Ranger");
    await settle("Pig");
    expect(started).toEqual(["Ranger", "Pig", "Rat"]);
    prefetch.request(["Rat", "Pig"]);
    await settle("Rat");
    expect(started).toEqual(["Ranger", "Pig", "Rat"]);
    expect(prefetch.settled("Rat")).toBe(true);
    expect(idle()).toBe(1);
  });

  test("a failed piece settles and lets the queue continue", async () => {
    const started: string[] = [];
    const prefetch = new ArtPrefetch(async (key) => { started.push(key); if (key === "inn") { throw new Error("missing"); } });
    prefetch.plan({ foreground: ["inn"], background: ["Wolf"] });
    for (let i = 0; i < 10; i += 1) { await Promise.resolve(); }
    expect(prefetch.settled("inn")).toBe(true);
    expect(started).toEqual(["inn", "Wolf"]);
  });
});
