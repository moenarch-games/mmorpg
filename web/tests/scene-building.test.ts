import { describe, expect, test } from "bun:test";
import type { IndexedMeshGeometry } from "@moritzbrantner/three-d-renderer";
import { createLocalWorld } from "../src/world/local-world";
import { CULL_DISTANCE_METRES, SceneryFrame, batchVisible, buildSceneryScene, type StaticSceneContent } from "../src/world/scenery-nodes";
import { expectValidMesh } from "./support/geometry";
import { testCharacters } from "./support/local-characters";
import { localZoneModule } from "./support/local-zone-module";

// The shared Rust static world (#184) through the real WASM export; generator
// behaviour itself is tested in crates/mmorpg-presentation.
const wasm = await localZoneModule();
const world = createLocalWorld(wasm, testCharacters()).scenery;
const scene = buildSceneryScene(world.scenery, world.staticScene);

describe("the shared static scene in the browser", () => {
  test("decodes valid indexed geometry with one payload per resource key", () => {
    const geometries = new Map<string, IndexedMeshGeometry>();
    const meshes = [
      ...scene.batches.map(({ node }) => node.geometry),
      ...scene.instanceBatches.map(({ batch }) => batch.geometry as IndexedMeshGeometry),
      ...scene.water.map((node) => node.geometry as IndexedMeshGeometry),
      ...scene.animated.flat().map((node) => node.geometry as IndexedMeshGeometry),
    ];
    for (const geometry of meshes) {
      expectValidMesh(geometry, geometry.resourceKey);
      const previous = geometries.get(geometry.resourceKey);
      if (previous) { expect(geometry).toBe(previous); }
      geometries.set(geometry.resourceKey, geometry);
    }
    expect(scene.instanceBatches.every(({ batch }) => batch.instances.length > 0)).toBe(true);
    expect(scene.stats.staticNodes).toBe(scene.batches.length + scene.ground.length + scene.instanceBatches.length);
    const modelled = Object.values(scene.stats.props).reduce((sum, count) => sum + (count ?? 0), 0);
    expect(modelled).toBe(world.scenery.props.length);
  });

  test("terrain carries vertex colours with upward faces, lights glow and water stays translucent", () => {
    // The textured ground layers (#525): the opaque grass base and weighted overlays, one draw each.
    expect(scene.ground.find((draw) => draw.layer === "grass" && !draw.overlay)).toBeDefined();
    expect(scene.ground.length).toBeLessThanOrEqual(7);
    for (const draw of scene.ground) {
      const count = draw.positions.length / 3;
      expect([draw.normals.length, draw.colors.length, draw.uvs.length, draw.tangents.length]).toEqual([count * 3, count * 3, count * 2, count * 4]);
      expect(draw.alpha?.length ?? 0).toBe(draw.overlay ? count : 0);
      expect(draw.colors.every((channel) => channel >= 0 && channel <= 1)).toBe(true);
      expect(draw.indices.every((index) => index < count)).toBe(true);
    }
    expect(scene.batches.some((batch) => batch.node.emissive !== undefined)).toBe(true);
    expect(scene.water.length).toBeGreaterThan(0);
    expect(scene.water.every((node) => node.opacity !== undefined && node.opacity < 1)).toBe(true);
    expect(scene.surfaceY(0, 0)).toBe(wasm.surfaceY(0, 0));
  });

  test("frames keep node IDs, move animated parts only with time and hide far groups", () => {
    const frame = new SceneryFrame(scene);
    const still = frame.nodes([0, 5, 0], { seconds: 3, animate: false });
    const again = frame.nodes([0, 5, 0], { seconds: 9, animate: false });
    expect(again.nodes).toEqual(still.nodes);
    const moving = frame.nodes([0, 5, 0], { seconds: 9, animate: true });
    expect(moving.nodes.map((node) => node.id)).toEqual(still.nodes.map((node) => node.id));
    expect(moving.nodes).not.toEqual(still.nodes);
    expect(new Set(still.nodes.map((node) => node.id)).size).toBe(still.nodes.length);
    const far = frame.nodes([5_000, 5, 5_000], { seconds: 0, animate: false });
    expect(far.visibleBatches).toBeLessThan(still.visibleBatches);
    const [batch] = scene.batches.filter((candidate) => candidate.cull === "near");
    const reach = CULL_DISTANCE_METRES.near + batch!.radius;
    expect(batchVisible(batch!, batch!.center[0] + reach - 0.01, batch!.center[1])).toBe(true);
    expect(batchVisible(batch!, batch!.center[0] + reach + 0.01, batch!.center[1])).toBe(false);
  });

  test("an export of another format or content revision fails closed", () => {
    const content = (layout: object): StaticSceneContent => ({
      ...world.staticScene,
      staticScene: () => JSON.stringify(layout),
    });
    const real = JSON.parse(world.staticScene.staticScene());
    expect(() => buildSceneryScene(world.scenery, content({ ...real, format: "other" }))).toThrow("Invalid static scene");
    expect(() => buildSceneryScene(world.scenery, content({ ...real, prefix: "scenery:1:0" }))).toThrow("Invalid static scene");
  });
});
