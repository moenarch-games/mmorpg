import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import type { RendererInstanceBatch, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import * as environment from "../src/world/environment";
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

  test("repeated vegetation and rocks submit real instance batches, preserving exported placement", () => {
    const fixture = fixtureScenery();
    for (const kind of ["tree-oak", "tree-pine", "grass-tuft", "rock-small"] as const) {
      const original = fixture.props.find((prop) => prop.kind === kind)!;
      expect(original).toBeDefined();
      const props = [0, 1, 2].map((index) => ({ ...original, position: [index * 300, 0, 0] as const, yaw: 0, scale: 1 }));
      const scenery = { ...fixture, props };
      const before = structuredClone(scenery);
      const scene = buildSceneryScene(scenery, ENVIRONMENT);
      const frame = new SceneryFrame(scene, "acceptance").nodes([0, 5, 8], { seconds: 0, animate: false });
      const repeated = frame.instanceBatches.filter((batch) => batch.instances.length >= 3);
      expect(repeated.length, kind).toBeGreaterThan(0);
      expect(repeated.every((batch) => batch.geometry.kind === "mesh" && batch.geometry.positions.length > 0)).toBe(true);
      for (const batch of repeated) {
        const xs = batch.instances.map((instance) => instance.modelMatrix?.[12] ?? instance.transform.translation[0]).sort((a, b) => a - b);
        expect(xs[1]! - xs[0]!).toBeCloseTo(3, 6);
        expect(xs[2]! - xs[1]!).toBeCloseTo(3, 6);
      }
      const next = new SceneryFrame(scene, "acceptance").nodes([1, 5, 8], { seconds: 1, animate: true });
      for (const batch of repeated) {
        expect(next.instanceBatches.find((other) => other.id === batch.id)!.geometry).toBe(batch.geometry);
      }
      expect(scenery).toEqual(before);
      expect(new SceneryFrame(scene, "acceptance").nodes([10000, 5, 10000], { seconds: 0, animate: false })
        .instanceBatches.filter((batch) => batch.visible !== false).length).toBeLessThan(frame.instanceBatches.length);
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
