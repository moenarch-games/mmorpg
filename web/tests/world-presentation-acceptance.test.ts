import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import type { RendererInstanceBatch, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import * as environment from "../src/world/environment";
import { CULL_DISTANCE_METRES } from "../src/world/mesh-batching";
import { buildSceneryScene, SceneryFrame } from "../src/world/scenery-nodes";
import { WorldPicking } from "../src/world/units/world-picking";
import { fixtureScenery } from "./support/scenery-fixture";

const { ENVIRONMENT } = environment;

describe("shared renderer adoption", () => {
  test("the actual renderer environment cycles through night and dawn, with fog and finite lighting", () => {
    const noon = environment.environmentAt(ENVIRONMENT, 0);
    const night = environment.environmentAt(ENVIRONMENT, 0.5);
    expect(environment.environmentAt(ENVIRONMENT, 1)).toEqual(noon);
    expect(environment.environmentAt(ENVIRONMENT, 1.5)).toEqual(night);
    expect(night.sun!.intensity).toBeLessThan(noon.sun!.intensity);
    expect(night.sky!.intensity).toBeLessThan(noon.sky!.intensity);
    expect(night.sun!.direction).not.toEqual(noon.sun!.direction);
    expect(night.background).not.toEqual(noon.background);
    for (const fraction of [0, 0.25, 0.5, 0.75, -0.25, Number.NaN]) {
      const frame = environment.environmentAt(ENVIRONMENT, fraction);
      expect(frame.fog!.near).toBeGreaterThanOrEqual(0);
      expect(frame.fog!.far).toBeGreaterThan(frame.fog!.near);
      expect(frame.sun!.direction.every(Number.isFinite)).toBe(true);
      expect(Math.hypot(...frame.sun!.direction)).toBeGreaterThan(0);
      expect(Number.isFinite(frame.sun!.intensity)).toBe(true);
      expect(frame.sun!.intensity).toBeGreaterThanOrEqual(0);
      expect(frame.sky!.intensity).toBeGreaterThan(0);
    }
  });

  test("repeated seeded vegetation and rocks share variant geometry without changing placed shapes", () => {
    const fixture = fixtureScenery();
    const vertices = (scene: ReturnType<typeof buildSceneryScene>) => {
      const result: string[] = [];
      const add = (geometry: RendererInstanceBatch["geometry"], placement: RendererInstanceBatch["instances"][number]) => {
        if (geometry.kind !== "mesh") { throw new Error("Expected procedural indexed mesh"); }
        const matrix = new THREE.Matrix4();
        if (placement.modelMatrix) { matrix.fromArray(placement.modelMatrix); }
        else { matrix.compose(new THREE.Vector3(...placement.transform.translation),
          new THREE.Quaternion(...(placement.transform.rotationQuaternion ?? [0, 0, 0, 1])),
          new THREE.Vector3(...(placement.transform.scale ?? [1, 1, 1]))); }
        for (const point of geometry.positions) {
          const transformed = new THREE.Vector3(...point).applyMatrix4(matrix);
          result.push([transformed.x, transformed.y, transformed.z].map((value) => value.toFixed(6)).join(","));
        }
      };
      for (const { node } of scene.batches) { if (!node.id.startsWith("static-all")) { add(node.geometry, node); } }
      for (const { batch } of scene.instanceBatches) { for (const instance of batch.instances) { add(batch.geometry, instance); } }
      return result.sort();
    };
    for (const kind of ["tree-oak", "tree-pine", "grass-tuft", "rock-small"] as const) {
      const original = fixture.props.find((prop) => prop.kind === kind)!;
      expect(original).toBeDefined();
      // Enough actual locations for bounded seeded variants to recur; do not require all props to look identical.
      const props = Array.from({ length: 64 }, (_, index) => ({ ...original,
        position: [(index % 8) * 300, 0, Math.floor(index / 8) * 300] as const,
        yaw: original.yaw, scale: original.scale }));
      const scenery = { ...fixture, props };
      const before = structuredClone(scenery);
      const scene = buildSceneryScene(scenery, ENVIRONMENT);
      const frame = new SceneryFrame(scene, "acceptance").nodes([0, 5, 8], { seconds: 0, animate: false });
      const repeated = frame.instanceBatches.filter((batch) => batch.instances.length >= 3);
      expect(repeated.length, kind).toBeGreaterThan(0);
      expect(repeated.every((batch) => batch.geometry.kind === "mesh" && batch.geometry.positions.length > 0)).toBe(true);
      const resources = new Set(frame.instanceBatches.map((batch) => batch.geometry.kind === "mesh" ? batch.geometry.resourceKey : null));
      expect(resources.size, kind).toBeLessThan(props.length);
      if (kind === "rock-small" || kind === "grass-tuft") {
        expect(resources.size).toBeGreaterThan(1);
        expect(resources.size).toBeLessThanOrEqual(kind === "rock-small" ? 7 : 4);
      }
      // Simple independent assembly is the oracle for the optimized multi-prop batching path:
      // every seeded shape at its actual position appears exactly once, with all local transforms intact.
      const reference = props.flatMap((prop) => vertices(buildSceneryScene({ ...fixture, props: [prop] }, ENVIRONMENT))).sort();
      expect(vertices(scene), kind).toEqual(reference);
      const next = new SceneryFrame(scene, "acceptance").nodes([1, 5, 8], { seconds: 1, animate: true });
      for (const batch of repeated) { expect(next.instanceBatches.find((other) => other.id === batch.id)!.geometry).toBe(batch.geometry); }
      expect(scenery).toEqual(before);
      expect(new SceneryFrame(scene, "acceptance").nodes([10000, 5, 10000], { seconds: 0, animate: false })
        .instanceBatches.filter((batch) => batch.visible !== false).length).toBe(scene.instanceBatches.filter((batch) => batch.cull === "always").length);
    }
  });

  test("original always/far/mid/near scenery classes retain distance policy at their exact bounds", () => {
    const fixture = fixtureScenery();
    const cases = [
      { kind: "tree-oak", collidable: false, cull: "always" },
      { kind: "tree-pine", collidable: true, cull: "far" },
      { kind: "rock-small", collidable: false, cull: "mid" },
      { kind: "rock-large", collidable: true, cull: "always" },
      { kind: "grass-tuft", collidable: false, cull: "near" },
    ] as const;
    for (const item of cases) {
      const original = fixture.props.find((prop) => prop.kind === item.kind)!;
      const prop = { ...original, collider: item.collidable
        ? { id: 1, center: original.position, halfExtents: original.halfExtents } : null };
      const scene = buildSceneryScene({ ...fixture, props: [prop] }, ENVIRONMENT);
      expect(scene.instanceBatches.length, item.kind).toBeGreaterThan(0);
      const frames = new SceneryFrame(scene, "acceptance");
      for (const group of scene.instanceBatches) {
        expect(group.cull, item.kind).toBe(item.cull);
        const geometry = group.batch.geometry;
        if (geometry.kind !== "mesh") { throw new Error("Expected procedural mesh"); }
        const points = group.batch.instances.flatMap((instance) => {
          const matrix = new THREE.Matrix4();
          if (instance.modelMatrix) { matrix.fromArray(instance.modelMatrix); }
          else {
            matrix.compose(new THREE.Vector3(...instance.transform.translation),
              new THREE.Quaternion(...(instance.transform.rotationQuaternion ?? [0, 0, 0, 1])),
              new THREE.Vector3(...(instance.transform.scale ?? [1, 1, 1])));
          }
          return geometry.positions.map((point) => new THREE.Vector3(...point).applyMatrix4(matrix));
        });
        const minX = Math.min(...points.map((point) => point.x));
        const maxX = Math.max(...points.map((point) => point.x));
        const minZ = Math.min(...points.map((point) => point.z));
        const maxZ = Math.max(...points.map((point) => point.z));
        expect(group.center[0]).toBeCloseTo((minX + maxX) / 2, 8);
        expect(group.center[1]).toBeCloseTo((minZ + maxZ) / 2, 8);
        expect(group.radius).toBeCloseTo(Math.hypot(maxX - minX, maxZ - minZ) / 2, 8);
        const visibleAt = (x: number) => frames.nodes([x, 5, group.center[1]], { seconds: 0, animate: false })
          .instanceBatches.find((batch) => batch.id === group.batch.id)!.visible !== false;
        if (item.cull === "always") {
          expect(visibleAt(group.center[0] + 1000000), item.kind).toBe(true);
        } else {
          const edge = group.center[0] + group.radius + CULL_DISTANCE_METRES[item.cull];
          expect(visibleAt(edge - 0.01), item.kind).toBe(true);
          expect(visibleAt(edge + 0.01), item.kind).toBe(false);
        }
      }
    }
  });

  test("terrain carries real vertex colours and lights are emissive while water remains translucent", () => {
    const scene = buildSceneryScene(fixtureScenery(), ENVIRONMENT);
    const frame = new SceneryFrame(scene, "acceptance").nodes([0, 5, 8], { seconds: 0, animate: false });
    const coloured = frame.nodes.filter((node) => node.geometry.kind === "mesh" && node.geometry.colors?.length);
    expect(coloured.length).toBeGreaterThan(0);
    for (const node of coloured) {
      if (node.geometry.kind === "mesh") {
        expect(node.geometry.colors!.length).toBe(node.geometry.positions.length);
        expect(node.geometry.colors!.every((color) => color.every((channel) => Number.isFinite(channel) && channel >= 0 && channel <= 1))).toBe(true);
      }
    }
    expect(frame.nodes.some((node) => node.emissive !== undefined)).toBe(true);
    expect(scene.water.length).toBeGreaterThan(0);
    expect(scene.water.every((node) => node.opacity !== undefined && node.opacity < 1)).toBe(true);
  });

  test("instanced scenery occludes picks at its rendered transform and hidden instances do not", () => {
    const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
    camera.position.set(0, 1, 10);
    camera.lookAt(0, 1, 0);
    camera.updateMatrixWorld(true);
    const body: RendererSceneNode = { id: "unit-creature-108-body", geometry: { kind: "box", size: [1, 2, 1] },
      color: "#ffffff", transform: { translation: [0, 1, 0] } };
    const blocker: RendererInstanceBatch = { id: "rocks", geometry: { kind: "box", size: [2, 3, 2] },
      color: "#777777", instances: [{ transform: { translation: [0, 1, 5] } }] };
    const picker = new WorldPicking();
    const point = new THREE.Vector2(0, 0);
    expect(picker.pick(point, camera, [], [body], [])).toEqual({ kind: "creature", id: 108 });
    expect(picker.pick(point, camera, [], [body], [blocker])).toBeNull();
    expect(picker.pick(point, camera, [], [body], [{ ...blocker, instances: [{ transform: { translation: [8, 1, 5] } }] }])).toEqual({ kind: "creature", id: 108 });
    expect(picker.pick(point, camera, [], [body], [{ ...blocker, visible: false }])).toEqual({ kind: "creature", id: 108 });
    picker.reset();
  });
});
