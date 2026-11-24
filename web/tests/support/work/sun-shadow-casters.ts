import * as THREE from "three";
import { shadowGeometry, StaggeredCascades } from "@moritzbrantner/three-lighting";
import { CASCADE_UPDATE_INTERVALS, SUN_SHADOWS } from "../../../src/world/lighting";
import { triangles } from "../../../src/world/shadow-lod";
import { mergeUnitMeshes } from "../../../src/world/units/merged-unit";
import { wasm } from "../../../src/wasm";

// The `sun-shadow-casters` measurement (#1083, #1120): `web/tests/shadow-lod.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. A camera walking and turning through Greyhaven redraws at
// most two of the three sun cascades a frame, and a skinned unit's shadow proxy draws a fraction of its body's
// triangles in each cascade. Counted without a GPU; callers await `shadowLodReady` first.

/** The game camera's view (55° high, 16:10) at `x, z` looking along `yaw`, its world matrix's elements. */
function view(x: number, z: number, yaw: number): number[] {
  const camera = new THREE.PerspectiveCamera(55, 1.6, 0.1, 1000);
  camera.position.set(x, 6, z);
  camera.rotation.set(-0.35, yaw, 0, "YXZ");
  camera.updateMatrixWorld();
  return camera.matrixWorld.elements;
}

/** The most cascades any frame after the first redraws over `frames` frames of a walk at run speed, turning. */
export function largestFrameCascadeRedraws(frames: number): number {
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
export function body(): { model: THREE.Group; triangles: number } {
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

/**
 * Per cascade (near, middle, far), the share of the body's triangles its shadow proxies draw, in percent, once their
 * simplified levels arrived (#1122: in a worker).
 */
export async function unitShadowTrianglePercent(): Promise<number[]> {
  const { model, triangles: full } = body();
  const merged = mergeUnitMeshes(model, new Set());
  await merged.shadowLevels;
  return [0, 1, 2].map((cascade) => Math.round((100 * merged.shadows.reduce((sum, proxy) => sum + triangles(shadowGeometry(proxy, cascade)), 0)) / full));
}

/** A stand-in nature descriptor: every entry a tree 6 m across and 14 m tall, one level (no bundle needed). */
function standInNatureDescriptor(entries: readonly string[]): string {
  return JSON.stringify({
    lodSelection: { metric: "projected-bounding-sphere-height-fraction" },
    assets: entries.map((key) => ({
      key, bounds: { min: [-3, 0, -3], max: [3, 14, 3] },
      lods: { boundingSphere: { center: [0, 7, 0], radius: 7.6 }, levels: [{ variant: "default", minScreenSize: 0 }] },
    })),
  });
}

/**
 * Off-screen casters the sun keeps (#1038): over `frames` frames of the walk through Greyhaven's woods (from the
 * `woods` review pose, 6 m/s, a quarter turn every four seconds), the largest share by which the sun's shadow sweep
 * grows the nature kit's drawn placements past the camera's own selection, in percent, for a sun `elevation` degrees
 * high. Every placement stands in as a 14 m tree, so bushes and rocks count as long casters as trees.
 */
export function largestFrameKitShadowCasterExtraPercent(frames: number, elevation: number): number {
  const entries = wasm.kitEntries("nature");
  const descriptor = standInNatureDescriptor(entries);
  const plain = new wasm.KitLods("nature", descriptor);
  const lit = new wasm.KitLods("nature", descriptor);
  const up = THREE.MathUtils.degToRad(elevation);
  // From the south-west, like the cascade walk's sun.
  lit.setSun(new Float64Array([-Math.cos(up) * 0.66, -Math.sin(up), -Math.cos(up) * 0.75]));
  const camera = new THREE.PerspectiveCamera(55, 1.6, 0.1, 1000);
  const clip = new THREE.Matrix4();
  const fov = THREE.MathUtils.degToRad(55);
  let largest = 0;
  let own = 0;
  let kept = 0;
  try {
    for (let f = 0; f <= frames; f++) {
      camera.position.set(-55 + 0.1 * f, 20, 8 - 0.03 * f);
      camera.rotation.set(-0.35, Math.PI / 2 + f * (Math.PI / 2 / 240), 0, "YXZ");
      camera.updateMatrixWorld();
      clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
      const clipFloats = new Float32Array(clip.elements);
      const { x, y, z } = camera.position;
      // Undefined: unchanged since the previous frame.
      own = plain.update(x, y, z, fov, clipFloats, 1 / 60, 1, false)?.length ?? own;
      kept = lit.update(x, y, z, fov, clipFloats, 1 / 60, 1, false)?.length ?? kept;
      if (f > 0 && own > 0) { largest = Math.max(largest, Math.round((100 * (kept - own)) / own)); }
    }
  } finally {
    plain.free();
    lit.free();
  }
  return largest;
}
