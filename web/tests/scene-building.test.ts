import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import type { IndexedMeshGeometry, RendererInstance, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { ENVIRONMENT } from "../src/world/environment";
import { CULL_DISTANCE_METRES, SceneBatcher, batchVisible } from "../src/world/mesh-batching";
import { BOX, MeshBuilder, compose, scale, translate } from "../src/world/mesh-builder";
import { PROP_KINDS, type Prop } from "../src/world/scenery";
import { SceneryFrame, buildSceneryScene, type SceneryScene } from "../src/world/scenery-nodes";
import { groundBiomes, terrainSurfaceY, toneVariant } from "../src/world/terrain-mesh";
import { bandFootprint, expectValidMesh, triangleNormals } from "./support/geometry";
import { fixtureScenery } from "./support/scenery-fixture";

/** Reconstruct the exact submitted geometry in world space for placement assertions. */
function transformedMesh(geometry: IndexedMeshGeometry, placement: RendererSceneNode | RendererInstance) {
  const matrix = new THREE.Matrix4();
  if (placement.modelMatrix) {
    matrix.fromArray(placement.modelMatrix);
  } else {
    matrix.compose(new THREE.Vector3(...placement.transform.translation),
      new THREE.Quaternion(...(placement.transform.rotationQuaternion ?? [0, 0, 0, 1])),
      new THREE.Vector3(...(placement.transform.scale ?? [1, 1, 1])));
  }
  return {
    positions: geometry.positions.map((point): [number, number, number] => {
      const result = new THREE.Vector3(...point).applyMatrix4(matrix);
      return [result.x, result.y, result.z];
    }),
    indices: geometry.indices,
  };
}

function propMeshes(scene: SceneryScene) {
  const ordinary = scene.batches.filter((batch) => !batch.node.id.startsWith("static-all"))
    .map(({ node }) => transformedMesh(node.geometry, node));
  const instanced = scene.instanceBatches.flatMap(({ batch }) => {
    if (batch.geometry.kind !== "mesh") {
      throw new Error(`Unexpected non-mesh prop geometry: ${batch.id}`);
    }
    const geometry = batch.geometry;
    return batch.instances.map((instance) => transformedMesh(geometry, instance));
  });
  return [...ordinary, ...instanced];
}

describe("static batching", () => {
  test("groups parts by chunk, colour and cull class under stable IDs and resource keys", () => {
    const batcher = new SceneBatcher("scenery:test:7");
    batcher.builder("#112233", 1, 1, "near").add(BOX, translate(1, 0, 1));
    batcher.builder("#112233", 30, 5, "near").add(BOX, translate(30, 0, 5));
    batcher.builder("#112233", 45, 5, "near").add(BOX, translate(45, 0, 5));
    batcher.builder("#445566", 1, 1, "near").add(BOX, translate(1, 0, 1));
    batcher.builder("#112233", 1, 1, "always").add(BOX, translate(1, 0, 1));
    batcher.builder("#778899", 1, 1, "mid");
    const batches = batcher.batches();
    expect(batches.map((batch) => batch.node.id)).toEqual([
      "static-120m0_0:always:112233",
      "static-40m0_0:near:112233",
      "static-40m0_0:near:445566",
      "static-40m1_0:near:112233",
    ]);
    expect(batches[1]!.node.geometry.positions).toHaveLength(48);
    expect(batches[1]!.node.geometry.resourceKey).toBe("scenery:test:7:40m0_0:near:112233");
    expect(batches[1]!.node.color).toBe("#112233");
    for (const batch of batches) {
      expectValidMesh(batch.node.geometry, batch.node.id);
    }
  });

  test("hides a class beyond its distance, measured to the batch's bounding circle", () => {
    const batcher = new SceneBatcher("test");
    batcher.builder("#000000", 0, 0, "near").add(BOX, scale(2, 1, 2));
    const [batch] = batcher.batches();
    expect(batch!.center).toEqual([0, 0]);
    expect(batch!.radius).toBeCloseTo(Math.SQRT2 * 2, 9);
    const reach = CULL_DISTANCE_METRES.near + batch!.radius;
    expect(batchVisible(batch!, reach - 0.01, 0)).toBe(true);
    expect(batchVisible(batch!, reach + 0.01, 0)).toBe(false);
  });
});

describe("terrain", () => {
  const scenery = fixtureScenery();
  const metres = (units: number) => units / scenery.unitsPerMetre;

  test("the rendered surface passes through every sample and interpolates between them", () => {
    const { terrain } = scenery;
    for (let row = 0; row < terrain.rows; row += 1) {
      for (let column = 0; column < terrain.columns; column += 1) {
        const x = metres(terrain.originXz[0] + column * terrain.step);
        const z = metres(terrain.originXz[1] + row * terrain.step);
        expect(terrainSurfaceY(terrain, scenery.unitsPerMetre, x, z)).toBeCloseTo(metres(terrain.heights[row * terrain.columns + column]!), 9);
      }
    }
    const between = terrainSurfaceY(terrain, scenery.unitsPerMetre, -15, -15);
    expect(Number.isFinite(between)).toBe(true);
  });

  test("roads, water, the plaza and the field give way to the surrounding ground", () => {
    const names = new Map(scenery.biomes.map((biome) => [biome.id, biome.name]));
    const ground = groundBiomes(scenery.terrain, names);
    const overlay = new Set(["road", "lake bed", "shore", "plaza", "farmland"]);
    expect(ground.every((biome) => !overlay.has(names.get(biome)!))).toBe(true);
    expect(ground.length).toBe(scenery.terrain.biomes.length);
    expect(toneVariant(12.5, -40)).toBe(toneVariant(12.5, -40));
    expect([0, 1, 2]).toContain(toneVariant(-3, 7));
  });
});

describe("the static scene", () => {
  const scenery = fixtureScenery();
  const build = (props: readonly Prop[] = scenery.props): SceneryScene => buildSceneryScene({ ...scenery, props }, ENVIRONMENT);

  test("every batch is valid indexed geometry and every prop kind has a model", () => {
    const scene = build();
    expect(Object.keys(scene.stats.props).sort()).toEqual([...PROP_KINDS].sort());
    for (const batch of scene.batches) {
      expectValidMesh(batch.node.geometry, batch.node.id);
    }
    for (const { batch } of scene.instanceBatches) {
      expect(batch.geometry.kind).toBe("mesh");
      if (batch.geometry.kind === "mesh") {
        expectValidMesh(batch.geometry, batch.id);
      }
      expect(batch.instances.length).toBeGreaterThan(0);
    }
    for (const kind of PROP_KINDS) {
      const prop = scenery.props.find((candidate) => candidate.kind === kind)!;
      // Animated props are independently checked below; every remaining kind must draw geometry.
      const single = build([prop]);
      expect(propMeshes(single).length + single.animated.sails.length + single.animated.flames.length + single.animated.reeds.length, kind)
        .toBeGreaterThan(0);
    }
    for (const node of scene.water) {
      expect(node.opacity).toBeLessThan(1);
    }
    const { sails, flames, reeds } = scene.animated;
    expect([sails.length, flames.length, reeds.length > 0]).toEqual([1, 1, true]);
    expect(scene.stats.staticNodes).toBe(scene.batches.length);
  });

  test("presentation edits receive new resource keys without changing gameplay revision", () => {
    const first = build();
    const second = buildSceneryScene({ ...scenery, presentationFingerprint: "fedcba9876543210" }, ENVIRONMENT);
    expect(second.batches.map((batch) => batch.node.id)).toEqual(first.batches.map((batch) => batch.node.id));
    const keys = new Set(first.batches.map((batch) => batch.node.geometry.resourceKey));
    expect(second.batches.every((batch) => !keys.has(batch.node.geometry.resourceKey))).toBe(true);
    expect(second.water.every((node) => node.geometry.resourceKey?.includes("fedcba9876543210"))).toBe(true);
    const instanceKeys = new Set(first.instanceBatches.map(({ batch }) => batch.geometry.kind === "mesh" ? batch.geometry.resourceKey : null));
    expect(second.instanceBatches.every(({ batch }) => batch.geometry.kind === "mesh" && !instanceKeys.has(batch.geometry.resourceKey))).toBe(true);
  });

  test("terrain faces point up and the same scenery always builds the same scene", () => {
    const first = build();
    const second = build();
    expect(second.batches.map((batch) => batch.node)).toEqual(first.batches.map((batch) => batch.node));
    expect(second.instanceBatches).toEqual(first.instanceBatches);
    const terrain = first.batches.filter((batch) => batch.node.id.startsWith("static-all-terrain"));
    expect(terrain.length).toBeGreaterThan(0);
    const colors = terrain.flatMap((batch) => batch.node.geometry.colors ?? []);
    expect(new Set(colors.map((color) => color.join(","))).size).toBeGreaterThan(1);
    for (const batch of terrain) {
      expect(batch.node.geometry.colors!.length).toBe(batch.node.geometry.positions.length);
      expect(triangleNormals(batch.node.geometry).every((normal) => normal[1] > 0)).toBe(true);
    }
  });

  test("props stand on the drawn terrain whatever feet height the export gives", () => {
    // The exported feet follow the full-resolution relief, which the 4 m mesh only approximates.
    for (const kind of ["tree-oak", "grass-tuft", "flowers", "house"] as const) {
      const prop = scenery.props.find((candidate) => candidate.kind === kind)!;
      const { unitsPerMetre } = scenery;
      const surface = terrainSurfaceY(scenery.terrain, unitsPerMetre, prop.position[0] / unitsPerMetre, prop.position[2] / unitsPerMetre);
      const positions = (feetMetres: number) => propMeshes(build([{
        ...prop,
        position: [prop.position[0], Math.round(feetMetres * unitsPerMetre), prop.position[2]],
      }])).flatMap((mesh) => mesh.positions);
      const floating = positions(surface + 0.4);
      expect(floating.length, kind).toBeGreaterThan(0);
      expect(positions(surface - 0.4), kind).toEqual(floating);
      const lowest = Math.min(...floating.map(([, y]) => y));
      // Trunks and blades reach a little into the ground; nothing hovers above it.
      expect(lowest, kind).toBeLessThanOrEqual(surface + 1e-9);
      expect(lowest, kind).toBeGreaterThan(surface - 0.35);
    }
  });

  test("the body-height footprint sees faces that span the band without a vertex in it", () => {
    // A 4 m tall wall box: its only vertices are at y = 0 and y = 4.
    const wall = new MeshBuilder();
    wall.add(BOX, compose(translate(0, 2, 0), scale(3, 2, 0.5)));
    expect(wall.positions.some(([, y]) => y > 0.3 && y < 1.8)).toBe(false);
    const footprint = bandFootprint(wall, 0.3, 1.8);
    expect(Math.max(...footprint.map(([x]) => Math.abs(x)))).toBeCloseTo(3, 9);
    expect(Math.max(...footprint.map(([, z]) => Math.abs(z)))).toBeCloseTo(0.5, 9);
  });

  test("walls at body height stay on their collider", () => {
    // Loose dressing (the barn's hay bales, the inn's barrels) may stand outside; walls may not.
    for (const kind of ["house", "keep", "windmill", "palisade", "gate-post", "waystone", "well", "cliff"] as const) {
      const prop = scenery.props.find((candidate) => candidate.kind === kind)!;
      const collider = prop.collider!;
      const scene = build([prop]);
      const margin = 0.35;
      const [cx, , cz] = collider.center.map((value) => value / 100);
      const [hx, , hz] = collider.halfExtents.map((value) => value / 100);
      const footprint = propMeshes(scene).flatMap((mesh) => bandFootprint(mesh, 0.3, 1.8));
      expect(footprint.length, kind).toBeGreaterThan(0);
      for (const [x, z] of footprint) {
        expect(Math.abs(x - cx!), `${kind} x`).toBeLessThanOrEqual(hx! + margin);
        expect(Math.abs(z - cz!), `${kind} z`).toBeLessThanOrEqual(hz! + margin);
      }
    }
  });

  test("repeated props add transforms rather than duplicate geometry or ordinary prop copies", () => {
    for (const kind of ["tree-oak", "tree-pine", "grass-tuft", "rock-small"] as const) {
      const prop = scenery.props.find((candidate) => candidate.kind === kind)!;
      const single = build([{ ...prop, position: [0, 0, 0] }]);
      // Identical copies intentionally share a location: varying shape seeds are not part of this resource test.
      const repeated = build(Array.from({ length: 5 }, () => ({ ...prop, position: [0, 0, 0] as const })));
      expect(single.instanceBatches.length, kind).toBeGreaterThan(0);
      expect(repeated.instanceBatches.length, kind).toBe(single.instanceBatches.length);
      const ordinaryProps = (scene: SceneryScene) => scene.batches.filter((batch) => !batch.node.id.startsWith("static-all"));
      expect(ordinaryProps(single), kind).toHaveLength(0);
      expect(ordinaryProps(repeated), kind).toHaveLength(0);
      for (const { batch } of repeated.instanceBatches) {
        const original = single.instanceBatches.find((candidate) => candidate.batch.id === batch.id)!.batch;
        expect(batch.geometry, kind).toEqual(original.geometry);
        expect(batch.instances.length, kind).toBe(original.instances.length * 5);
      }
      const vertices = (scene: SceneryScene) => propMeshes(scene).reduce((sum, mesh) => sum + mesh.positions.length, 0);
      expect(vertices(repeated), kind).toBe(vertices(single) * 5);
    }
  });

  test("animated parts keep their node IDs and only move with time", () => {
    const scene = build();
    const frame = new SceneryFrame(scene, "test");
    const still = frame.nodes([0, 5, 0], { seconds: 3, animate: false });
    const again = frame.nodes([0, 5, 0], { seconds: 9, animate: false });
    expect(again.nodes).toEqual(still.nodes);
    const moving = frame.nodes([0, 5, 0], { seconds: 9, animate: true });
    expect(moving.nodes.map((node) => node.id)).toEqual(still.nodes.map((node) => node.id));
    expect(moving.nodes).not.toEqual(still.nodes);
    expect(new Set(still.nodes.map((node) => node.id)).size).toBe(still.nodes.length);
    const far = frame.nodes([5_000, 5, 5_000], { seconds: 0, animate: false });
    expect(far.visibleBatches).toBeLessThan(still.visibleBatches);
  });
});
