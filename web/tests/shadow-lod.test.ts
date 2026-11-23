import { beforeAll, describe, expect, test } from "bun:test";
import * as THREE from "three";
import { shadowGeometry, StaggeredCascades } from "@moritzbrantner/three-lighting";
import { CASCADE_UPDATE_INTERVALS, SUN_SHADOWS } from "../src/world/lighting";
import { closedSurface, shadowLevels, shadowLodReady, triangles, UNIT_SHADOW_LEVELS } from "../src/world/shadow-lod";
import { mergeUnitMeshes } from "../src/world/units/merged-unit";

// The `sun-shadow-casters` budget (#1083, `.performance/contract.json`): a camera walking and turning through Greyhaven
// redraws at most two of the three sun cascades a frame, and a skinned unit's shadow proxy draws a fraction of its
// body's triangles in each cascade. Counted without a GPU.

beforeAll(() => shadowLodReady);

/** The game camera's view (55° high, 16:10) at `x, z` looking along `yaw`, its world matrix's elements. */
function view(x: number, z: number, yaw: number): number[] {
  const camera = new THREE.PerspectiveCamera(55, 1.6, 0.1, 1000);
  camera.position.set(x, 6, z);
  camera.rotation.set(-0.35, yaw, 0, "YXZ");
  camera.updateMatrixWorld();
  return camera.matrixWorld.elements;
}

/** The most cascades any frame after the first redraws over `frames` frames of a walk at run speed, turning. */
function largestFrameCascadeRedraws(frames: number): number {
  const cascades = new StaggeredCascades();
  const frame = (f: number) => cascades.update({
    shadows: SUN_SHADOWS, mapSize: 2048, sunDirection: [-0.35, -0.85, -0.4], fovY: THREE.MathUtils.degToRad(55), aspect: 1.6,
    // 6 m/s at 60 frames a second, the camera turning a quarter turn every four seconds.
    worldFromView: view(0.1 * f, -0.03 * f, f * (Math.PI / 2 / 240)), intervals: CASCADE_UPDATE_INTERVALS,
  }).filter((cascade) => cascade.redraw).length;
  frame(0);
  let largest = 0;
  for (let f = 1; f <= frames; f++) { largest = Math.max(largest, frame(f)); }
  return largest;
}

/** A skinned body of about 20 000 triangles on two joints: a capsule, closed, drawn from both sides. */
function body(): { model: THREE.Group; triangles: number } {
  const model = new THREE.Group();
  const root = new THREE.Bone();
  const knee = new THREE.Bone();
  knee.position.set(0, 0.9, 0);
  root.add(knee);
  model.add(root);
  model.updateMatrixWorld(true);
  const geometry = new THREE.CapsuleGeometry(0.3, 1.2, 48, 96).translate(0, 0.9, 0);
  const position = geometry.getAttribute("position");
  const joints: number[] = [];
  const weights: number[] = [];
  for (let i = 0; i < position.count; i++) {
    const w = THREE.MathUtils.clamp((position.getY(i) - 0.6) / 0.6, 0, 1);
    joints.push(0, 1, 0, 0);
    weights.push(1 - w, w, 0, 0);
  }
  geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute(joints, 4));
  geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute(weights, 4));
  const mesh = new THREE.SkinnedMesh(geometry, new THREE.MeshStandardMaterial({ side: THREE.DoubleSide }));
  model.add(mesh);
  mesh.bind(new THREE.Skeleton([root, knee]), mesh.matrixWorld);
  return { model, triangles: triangles(geometry) };
}

/** Per cascade, the share of the body's triangles its shadow proxies draw, in percent. */
function unitShadowTrianglePercent(): number[] {
  const { model, triangles: full } = body();
  const merged = mergeUnitMeshes(model, new Set());
  return [0, 1, 2].map((cascade) => Math.round((100 * merged.shadows.reduce((sum, proxy) => sum + triangles(shadowGeometry(proxy, cascade)), 0)) / full));
}

describe("sun-shadow-casters", () => {
  test("a walking, turning camera redraws at most two cascades a frame", () => {
    for (const frames of [16, 64, 256]) { expect(largestFrameCascadeRedraws(frames)).toBeLessThanOrEqual(2); }
  });

  test("a unit's shadow proxy draws at most a third of its triangles near and an eighth far", () => {
    const [near, middle, far] = unitShadowTrianglePercent();
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
