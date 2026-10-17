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
    picker.reset();
  });
});
