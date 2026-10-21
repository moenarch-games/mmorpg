import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import type { RendererInstanceBatch, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import * as environment from "../src/world/environment";
import { WorldPicking } from "../src/world/units/world-picking";

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
    const player = { ...body, id: "unit-player-2-body" };
    expect(picker.pick(point, camera, [], [player])).toBeNull();
    expect(picker.pick(point, camera, [], [player], [], [], [], new Set([2]))).toEqual({ kind: "player", id: 2 });
    picker.reset();
  });

  test("assisted selection catches a near miss but never selects through opaque geometry", () => {
    const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
    camera.position.set(0, 1, 10);
    camera.lookAt(0, 1, 0);
    camera.updateMatrixWorld(true);
    const body: RendererSceneNode = { id: "unit-creature-108-body", geometry: { kind: "box", size: [1, 2, 1] },
      color: "#ffffff", transform: { translation: [0, 1, 0] } };
    const wall: RendererSceneNode = { id: "wall", geometry: { kind: "box", size: [2, 3, 1] },
      color: "#777777", transform: { translation: [0, 1, 5] } };
    const picker = new WorldPicking();
    const nearMiss = new THREE.Vector2(0.11, 0);
    const viewport = { width: 800, height: 800 };
    const pick = (scenery: RendererSceneNode[], units: RendererSceneNode[] = [body]) =>
      picker.pick(nearMiss, camera, scenery, units, [], [], [], new Set(), viewport);
    expect(picker.pick(nearMiss, camera, [], [body])).toBeNull(); // Exact geometry misses.
    expect(pick([])).toEqual({ kind: "creature", id: 108 });
    expect(pick([wall])).toBeNull();
    expect(pick([], [{ ...wall, id: "unit-player-2-body" }, body])).toBeNull();
    expect(picker.pick(new THREE.Vector2(0.5, 0), camera, [], [body], [], [], [], new Set(), viewport)).toBeNull();
    picker.reset();
  });

  test("direct hits beat nearby proxies; imported models and doors also have assisted hitboxes", () => {
    const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
    camera.position.set(0, 1, 10);
    camera.lookAt(0, 1, 0);
    camera.updateMatrixWorld(true);
    const visible: RendererSceneNode = { id: "unit-creature-108-body", geometry: { kind: "box", size: [1, 2, 1] },
      color: "#ffffff", transform: { translation: [0, 1, 0] } };
    const nearby: RendererSceneNode = { ...visible, id: "unit-npc-2-body", transform: { translation: [0.55, 1, 5] } };
    const viewport = { width: 800, height: 800 };
    const picker = new WorldPicking();
    expect(picker.pick(new THREE.Vector2(0, 0), camera, [], [visible, nearby], [], [], [], new Set(), viewport))
      .toEqual({ kind: "creature", id: 108 });

    const model = new THREE.Mesh(new THREE.BoxGeometry(0.2, 2, 0.2), new THREE.MeshBasicMaterial());
    model.position.set(0, 1, 0);
    model.updateMatrixWorld(true);
    const entity = { kind: "npc" as const, id: 7 };
    const models = [{ entity, object: model }];
    expect(picker.pick(new THREE.Vector2(0.035, 0), camera, [], [], [], models)).toBeNull();
    expect(picker.pick(new THREE.Vector2(0.035, 0), camera, [], [], [], models, [], new Set(), viewport))
      .toEqual(entity);
    expect(picker.pick(new THREE.Vector2(0.035, 0), camera, [{ ...nearby, id: "wall" }], [], [], models, [], new Set(), viewport))
      .toBeNull();

    const door = { door: 1, object: model };
    expect(picker.pick(new THREE.Vector2(0.035, 0), camera, [], [], [], [], [door], new Set(), viewport))
      .toEqual({ kind: "door", id: 1 });
    picker.reset();
  });
});
