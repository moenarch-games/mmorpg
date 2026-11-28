import * as THREE from "three";

/**
 * Makes `root`'s skinned meshes that are bound to the same bones with the same inverse bind matrices share one
 * `Skeleton`, returning how many skeletons it dropped (#1257). three's glTF loader and `SkeletonUtils.clone` give each
 * skinned mesh its own skeleton even where an animal's body, eyes and fur share one skin, and every skeleton recomputes
 * its bone matrices and re-uploads its bone texture each frame it is drawn: an animal's five meshes did the same work
 * five times. Bone matrices depend only on the bones and their inverses, so the drawn pose is unchanged.
 */
export function shareSkeletons(root: THREE.Object3D): number {
  const shared: THREE.Skeleton[] = [];
  let dropped = 0;
  root.traverse((object) => {
    const mesh = object as THREE.SkinnedMesh;
    if (!mesh.isSkinnedMesh) { return; }
    const same = shared.find((skeleton) => sameSkin(skeleton, mesh.skeleton));
    if (!same) {
      shared.push(mesh.skeleton);
    } else if (same !== mesh.skeleton) {
      mesh.skeleton.dispose();
      mesh.skeleton = same;
      dropped += 1;
    }
  });
  return dropped;
}

function sameSkin(a: THREE.Skeleton, b: THREE.Skeleton): boolean {
  if (a.bones.length !== b.bones.length) { return false; }
  for (let index = 0; index < a.bones.length; index += 1) {
    if (a.bones[index] !== b.bones[index] || !a.boneInverses[index]!.equals(b.boneInverses[index]!)) { return false; }
  }
  return true;
}

/** Per held skeleton, the hold of the root it belongs to. */
const holds = new WeakMap<THREE.Skeleton, HeldMatrices>();

/**
 * A subtree whose world matrices are recomputed only in a frame it was posed or moved (#1257). three's
 * `scene.updateMatrixWorld` visits every node of the scene each frame, composing every auto-updated node's matrix and
 * multiplying it into its world matrix whether it moved or not: a skinned animal is a hundred bones, and most animals
 * are hidden or hold their pose on most frames. A held root skips its whole subtree until [`touch`] (or its own
 * `matrixWorldNeedsUpdate`) asks again, and its skeletons keep the bone textures they uploaded ([`HeldMatrices.poseOf`]).
 * Presentation only: ambient creatures, never anything picked or collided.
 */
export class HeldMatrices {
  #posed = true;
  /** How many times the subtree's matrices were recomputed: its skeletons' pose revision. */
  #poses = 0;
  /** Subtrees recomputed since the last [`takeRecomputed`] (the `ambient-clips` work counter). */
  static #recomputed = 0;

  constructor(root: THREE.Object3D) {
    const update = Object.getPrototypeOf(root).updateMatrixWorld as THREE.Object3D["updateMatrixWorld"];
    const hold = this;
    root.updateMatrixWorld = function updateMatrixWorld(this: THREE.Object3D, force?: boolean): void {
      if (!hold.#posed && !force && !this.matrixWorldNeedsUpdate) { return; }
      hold.#posed = false;
      hold.#poses += 1;
      HeldMatrices.#recomputed += 1;
      update.call(this, force);
    };
    root.traverse((object) => {
      if ((object as THREE.SkinnedMesh).isSkinnedMesh) { holds.set((object as THREE.SkinnedMesh).skeleton, this); }
    });
  }

  /** The subtree was posed (its local matrices changed): the next `updateMatrixWorld` recomputes it. */
  touch(): void {
    this.#posed = true;
  }

  /** The pose revision of a held `skeleton`'s root, or undefined for a skeleton no hold owns. */
  static poseOf(skeleton: THREE.Skeleton): number | undefined {
    const hold = holds.get(skeleton);
    return hold === undefined ? undefined : hold.#poses;
  }

  /** Held subtrees whose matrices were recomputed since the last call. */
  static takeRecomputed(): number {
    const recomputed = HeldMatrices.#recomputed;
    HeldMatrices.#recomputed = 0;
    return recomputed;
  }
}
