import { describe, expect, mock, test } from "bun:test";
import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import * as staticBundle from "../src/world/static-bundle";
import { InstancedLevels } from "../src/world/instanced-levels";
import { ZoneLifecycle } from "../src/world/zone-lifecycle";

// Zone transitions (#1005): a replaced zone releases what it owns exactly once, a revisit reuses the parsed bundle,
// and a load the zone abandoned attaches nothing. The bundle is a fake: parsing counts calls instead of fetching.
const TEST_BUNDLE = "test://zone-lifecycle/";
const parses: string[] = [];
let gate: Promise<void> = Promise.resolve();
const shared = { geometry: new THREE.BoxGeometry(1, 1, 1), material: new LitMaterial() };
mock.module("../src/world/static-bundle", () => ({
  ...staticBundle,
  loadStaticBundle: async (base: string, keys: readonly string[], ktx2?: unknown) => {
    if (!base.startsWith(TEST_BUNDLE)) { return staticBundle.loadStaticBundle(base, keys, ktx2 as never); }
    parses.push(base);
    await gate;
    return keys.map((key) => {
      const root = new THREE.Group();
      root.name = key;
      root.matrixAutoUpdate = false;
      root.add(new THREE.Mesh(shared.geometry, shared.material));
      return root;
    });
  },
}));
const { KitModules } = await import("../src/world/kit-modules");

const placements = [0, 4, 8].map((x) => ({ key: "mill", matrix: new THREE.Matrix4().makeTranslation(x, 0, 0).toArray(new Float32Array(16)) }));

/** Counts each `dispose` of `targets`. */
function disposals(targets: readonly THREE.EventDispatcher<{ dispose: object }>[]): Map<unknown, number> {
  const counts = new Map<unknown, number>(targets.map((target) => [target, 0]));
  for (const target of targets) { target.addEventListener("dispose", () => counts.set(target, counts.get(target)! + 1)); }
  return counts;
}

describe("zone presentation lifecycle", () => {
  test("replacing a zone disposes its owned resources exactly once; a revisit reuses the parsed bundle", async () => {
    const greyhaven = new ZoneLifecycle();
    const first = greyhaven.own(new KitModules(`${TEST_BUNDLE}a/`, "test kit", ["mill"], placements));
    expect(await first.load()).toBe(true);
    expect(first.drawn).toBe(true);
    const owned = first.roots().roots[0]!.children.filter((child) => child instanceof THREE.BatchedMesh) as THREE.BatchedMesh[];
    expect(owned.length).toBeGreaterThan(0);
    const ownedCounts = disposals(owned);
    const sharedCounts = disposals([shared.geometry, shared.material]);

    // Travel to the Depths: Greyhaven's presentation retires, twice to prove it releases once.
    greyhaven.retire();
    greyhaven.retire();
    expect([...ownedCounts.values()]).toEqual(owned.map(() => 1));
    expect(first.drawn).toBe(false);
    expect(first.roots().roots).toEqual([]);
    // The parsed bundle is retained: its geometry and material were never released.
    expect([...sharedCounts.values()]).toEqual([0, 0]);
    // A late request for the retired zone's art does nothing.
    expect(await first.load()).toBe(false);

    // Back to Greyhaven: the new zone draws from the same parse.
    const revisit = new ZoneLifecycle();
    const second = revisit.own(new KitModules(`${TEST_BUNDLE}a/`, "test kit", ["mill"], placements));
    expect(await second.load()).toBe(true);
    expect(second.drawn).toBe(true);
    expect(parses.filter((base) => base === `${TEST_BUNDLE}a/`)).toHaveLength(1);
    expect([...sharedCounts.values()]).toEqual([0, 0]);
  });

  test("a load still in flight when its zone retires attaches nothing", async () => {
    let release!: () => void;
    gate = new Promise((resolve) => { release = resolve; });
    const zone = new ZoneLifecycle();
    const kit = zone.own(new KitModules(`${TEST_BUNDLE}b/`, "test kit", ["mill"], placements));
    const loading = kit.load();
    zone.retire();
    release();
    await loading;
    expect(kit.drawn).toBe(false);
    expect(kit.roots().roots).toEqual([]);
    gate = Promise.resolve();
  });

  test("a component handed to a retired zone is disposed at once", () => {
    const zone = new ZoneLifecycle();
    zone.retire();
    let disposed = 0;
    zone.own({ dispose: () => { disposed += 1; } });
    expect(disposed).toBe(1);
  });

  test("placed kit levels release their geometry copies, instances and fade materials, never the bundle's", () => {
    const scene = new THREE.Group();
    scene.add(new THREE.Mesh(shared.geometry, shared.material));
    const levels = new InstancedLevels("test", ["rock"], [[scene]], placements.map(({ matrix }) => ({ key: "rock", matrix })));
    const meshes = levels.root.children as THREE.InstancedMesh[];
    const fade = meshes.find((mesh) => mesh.name.endsWith(".fading"))!.material as THREE.Material;
    const owned = disposals([...meshes, ...meshes.map((mesh) => mesh.geometry), fade]);
    const sharedCounts = disposals([shared.geometry, shared.material]);
    levels.dispose();
    levels.dispose();
    expect([...owned.values()]).toEqual([...owned.keys()].map(() => 1));
    expect([...sharedCounts.values()]).toEqual([0, 0]);
    expect(levels.root.children).toHaveLength(0);
  });
});
