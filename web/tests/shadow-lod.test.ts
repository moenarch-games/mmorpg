import { beforeAll, describe, expect, test } from "bun:test";
import * as THREE from "three";
import { shadowGeometry } from "@moritzbrantner/three-lighting";
import { shadowBakeWork, useShadowBaker } from "../src/world/shadow-bake";
import { closedSurface, shadowLevels, shadowLodReady, triangles, UNIT_SHADOW_LEVELS } from "../src/world/shadow-lod";
import { indexLevels } from "../src/world/shadow-lod-indices";
import { mergeUnitMeshes, releaseUnit } from "../src/world/units/merged-unit";
import { body, largestFrameCascadeRedraws, unitShadowTrianglePercent } from "./support/work/sun-shadow-casters";

// The `sun-shadow-casters` budget (#1083, `.performance/contract.json`): a camera walking and turning through Greyhaven
// redraws at most two of the three sun cascades a frame, and a skinned unit's shadow proxy draws a fraction of its
// body's triangles in each cascade. Counted without a GPU by `support/work/sun-shadow-casters.ts`, which
// `scripts/work-evidence.ts` emits as evidence too (#1120).

beforeAll(() => shadowLodReady);

describe("sun-shadow-casters", () => {
  test("a walking, turning camera redraws at most two cascades a frame", () => {
    for (const frames of [16, 64, 256]) { expect(largestFrameCascadeRedraws(frames)).toBeLessThanOrEqual(2); }
  });

  test("a unit's shadow proxy draws at most a third of its triangles near and an eighth far", async () => {
    const [near, middle, far] = await unitShadowTrianglePercent();
    expect(near).toBeLessThanOrEqual(33);
    expect(middle).toBeLessThanOrEqual(12);
    expect(far).toBeLessThanOrEqual(12);
  });

  test("a closed double-sided body casts through one single-sided proxy", () => {
    const { model } = body();
    const merged = mergeUnitMeshes(model, new Set());
    expect(merged.shadows.map((proxy) => proxy.name)).toEqual(["shadow:single"]);
  });
});

describe("shadow levels", () => {
  test("each level shares the vertices and only shortens the index", () => {
    const sphere = new THREE.SphereGeometry(1, 64, 32);
    const levels = shadowLevels(sphere, UNIT_SHADOW_LEVELS);
    expect(levels).toHaveLength(2);
    for (const level of levels) {
      expect(level.getAttribute("position")).toBe(sphere.getAttribute("position"));
      expect(triangles(level)).toBeLessThan(triangles(sphere));
    }
    expect(triangles(levels[1]!)).toBeLessThan(triangles(levels[0]!));
  });

  test("closedSurface welds seams and tells a closed box from an open plane", () => {
    expect(closedSurface(new THREE.BoxGeometry(1, 1, 1, 2, 2, 2))).toBe(true);
    expect(closedSurface(new THREE.SphereGeometry(1, 16, 8))).toBe(true);
    expect(closedSurface(new THREE.PlaneGeometry(1, 1))).toBe(false);
    expect(closedSurface(new THREE.CylinderGeometry(1, 1, 1, 8, 1, true))).toBe(false);
  });
});

// #1122: simplifying units' proxies cost ~230 ms of the first update after joining; it now runs off the page's thread,
// once per look.
describe("shadow proxies off the entry path", () => {
  test("merging a unit simplifies nothing: its proxy casts its full shape until the levels arrive", async () => {
    const { model, triangles: full } = body();
    const merged = mergeUnitMeshes(model, new Set());
    const [proxy] = merged.shadows;
    expect(proxy!.userData.shadowGeometry).toBeUndefined();
    expect(triangles(shadowGeometry(proxy!, 0))).toBe(full);
    await merged.shadowLevels;
    expect(triangles(shadowGeometry(proxy!, 0))).toBeLessThan(full);
  });

  test("units of one look share one simplification", async () => {
    const calls: number[] = [];
    useShadowBaker(async (positions, indices, levels) => {
      calls.push(indices.length);
      return indexLevels(indices, positions, levels);
    });
    try {
      const first = mergeUnitMeshes(body().model, new Set());
      const second = mergeUnitMeshes(body().model, new Set());
      await Promise.all([first.shadowLevels, second.shadowLevels]);
      expect(calls).toHaveLength(1);
      expect(shadowBakeWork()).toEqual({ requests: 2, simplified: 1 });
      expect(triangles(shadowGeometry(second.shadows[0]!, 2))).toBe(triangles(shadowGeometry(first.shadows[0]!, 2)));
    } finally {
      useShadowBaker(null);
    }
  });

  test("a unit dropped before its levels arrive never takes them", async () => {
    const { model } = body();
    const merged = mergeUnitMeshes(model, new Set());
    releaseUnit(model, merged);
    await merged.shadowLevels;
    expect(merged.shadows[0]!.userData.shadowGeometry).toBeUndefined();
  });
});
