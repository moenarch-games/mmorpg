import * as THREE from "three";
import { HeldMatrices, shareSkeletons } from "../../../src/world/held-matrices";
import { onceAcrossPasses, takeSkeletonUpdates } from "../../../src/world/lighting";

// The `scene-matrix-frame` measurement (#1257): `web/tests/held-matrices.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. A frame walks the scene's world matrices once for all of its
// render passes, an ambient animal's subtree is recomputed only in a frame it was posed or moved, and its meshes share
// one skeleton, whose bone texture uploads only after a pose.

/** Joints and skinned meshes of one stand-in animal (a body, eyes, fur, ...: five skins over one skeleton). */
export const BONES = 24;
export const SKINS = 5;
/** Render passes of a frame: two sun cascades and the main pass, each asking for the world matrices as three does. */
export const PASSES = 3;

export type Animal = { root: THREE.Group; held: HeldMatrices; bones: THREE.Bone[]; skins: THREE.SkinnedMesh[] };

/**
 * One animal as `SkeletonUtils.clone` leaves a glTF one: a chain of joints and `SKINS` skinned meshes, each with its own
 * skeleton over the same joints and inverses, then shared and held as `AmbientAnimals` does.
 */
export function animal(x: number): Animal {
  const root = new THREE.Group();
  root.matrixAutoUpdate = false;
  root.matrix.makeTranslation(x, 0, 0);
  const bones = Array.from({ length: BONES }, () => new THREE.Bone());
  bones.forEach((bone, index) => {
    bone.position.set(0, 0.1, 0);
    (index === 0 ? root : bones[index - 1]!).add(bone);
  });
  root.updateMatrixWorld(true);
  const inverses = bones.map((bone) => bone.matrixWorld.clone().invert());
  const skins = Array.from({ length: SKINS }, () => {
    const mesh = new THREE.SkinnedMesh(new THREE.BufferGeometry(), new THREE.MeshBasicMaterial());
    root.add(mesh);
    mesh.bind(new THREE.Skeleton(bones, inverses.map((inverse) => inverse.clone())), new THREE.Matrix4());
    return mesh;
  });
  shareSkeletons(root);
  return { root, held: new HeldMatrices(root), bones, skins };
}

/** Poses `animal` as a sampled clip does: its joints' local transforms change, and it is touched. */
export function pose(animal: Animal, angle: number): void {
  for (const bone of animal.bones) { bone.rotation.z = angle; }
  animal.held.touch();
}

/**
 * `placed` animals in one scene, `posed` of them (in turn) posed each frame, over `frames` warm frames after a first:
 * the most subtrees recomputed, skeletons updated (each a bone texture upload) and scene matrix walks of any warm frame,
 * and the distinct skeletons per animal.
 */
export function sceneMatrixFrames(placed: number, posed: number, frames: number) {
  const scene = new THREE.Scene();
  // As the renderer's scene root (three-d-renderer): it never moves, so it forces no descendant's matrices.
  scene.matrixAutoUpdate = false;
  const animals = Array.from({ length: placed }, (_, index) => animal(index * 4));
  for (const { root } of animals) { scene.add(root); }
  const walk = THREE.Object3D.prototype.updateMatrixWorld;
  let walks = 0;
  THREE.Object3D.prototype.updateMatrixWorld = function updateMatrixWorld(this: THREE.Object3D, force?: boolean): void {
    if (this === scene) { walks += 1; }
    walk.call(this, force);
  };
  const largest = { recomputed: 0, skeletonUpdates: 0, walks: 0 };
  try {
    for (let frame = 0; frame <= frames; frame += 1) {
      HeldMatrices.takeRecomputed();
      takeSkeletonUpdates();
      walks = 0;
      if (frame > 0) {
        for (let index = 0; index < posed; index += 1) { pose(animals[(frame * posed + index) % placed]!, frame / 10); }
      }
      onceAcrossPasses(scene, () => {
        for (let pass = 0; pass < PASSES; pass += 1) {
          // Each pass asks for the matrices (render-engine's frame, three's `render`), and draws every skin.
          scene.updateMatrixWorld();
          for (const { skins } of animals) { for (const skin of skins) { skin.skeleton.update(); } }
        }
      });
      const recomputed = HeldMatrices.takeRecomputed();
      const skeletonUpdates = takeSkeletonUpdates();
      if (frame === 0) { continue; }
      largest.recomputed = Math.max(largest.recomputed, recomputed);
      largest.skeletonUpdates = Math.max(largest.skeletonUpdates, skeletonUpdates);
      largest.walks = Math.max(largest.walks, walks);
    }
  } finally {
    THREE.Object3D.prototype.updateMatrixWorld = walk;
  }
  const skeletonsPerAnimal = Math.max(...animals.map(({ skins }) => new Set(skins.map((skin) => skin.skeleton)).size));
  return { ...largest, skeletonsPerAnimal, animals, scene };
}
