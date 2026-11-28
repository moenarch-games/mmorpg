import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { onceAcrossPasses } from "../src/world/lighting";
import { SKINS, animal, pose, sceneMatrixFrames } from "./support/work/scene-matrix-frame";

// The `scene-matrix-frame` budget (#1257, `.performance/contract.json`): one walk of the scene's world matrices a frame
// for all of its passes, and an ambient animal's matrices and bone texture recomputed only in a frame it was posed.
// `support/work/scene-matrix-frame.ts` measures it; `scripts/work-evidence.ts` emits the same measurement.
describe("scene-matrix-frame", () => {
  test("the work of a warm frame follows the posed animals, not the placed ones", () => {
    for (const placed of [16, 64, 256]) {
      const work = sceneMatrixFrames(placed, 4, 32);
      expect({ recomputed: work.recomputed, skeletonUpdates: work.skeletonUpdates, walks: work.walks, skeletonsPerAnimal: work.skeletonsPerAnimal })
        .toEqual({ recomputed: 4, skeletonUpdates: 4, walks: 1, skeletonsPerAnimal: 1 });
    }
    expect([1, 2, 4].map((posed) => sceneMatrixFrames(64, posed, 32).recomputed)).toEqual([1, 2, 4]);
  });

  test("a posed animal draws its new pose and a held one keeps its last", () => {
    const scene = new THREE.Scene();
    scene.matrixAutoUpdate = false;
    const posed = animal(0);
    const held = animal(4);
    scene.add(posed.root, held.root);
    const frame = () => onceAcrossPasses(scene, () => { for (const { skins } of [posed, held]) { for (const skin of skins) { skin.skeleton.update(); } } });
    frame();
    const before = [posed, held].map(({ skins }) => Float32Array.from(skins[0]!.skeleton.boneMatrices!));
    pose(posed, 0.5);
    frame();
    // Exactly what three computes from the joints, unshared and unheld.
    const reference = new THREE.Skeleton(posed.bones, posed.skins[0]!.skeleton.boneInverses);
    reference.update();
    expect(Array.from(posed.skins[0]!.skeleton.boneMatrices!)).toEqual(Array.from(reference.boneMatrices!));
    expect(Array.from(posed.skins[0]!.skeleton.boneMatrices!)).not.toEqual(Array.from(before[0]!));
    expect(Array.from(held.skins[0]!.skeleton.boneMatrices!)).toEqual(Array.from(before[1]!));
    // Every skin of an animal draws from its one skeleton.
    expect(new Set(posed.skins.map((skin) => skin.skeleton)).size).toBe(1);
    expect(posed.skins.length).toBe(SKINS);
  });

  test("a moved animal is placed even without a pose", () => {
    const scene = new THREE.Scene();
    scene.matrixAutoUpdate = false;
    const moved = animal(0);
    scene.add(moved.root);
    onceAcrossPasses(scene, () => {});
    moved.root.matrix.makeTranslation(10, 0, 0);
    moved.root.matrixWorldNeedsUpdate = true;
    onceAcrossPasses(scene, () => {});
    // The joint's world matrix as the frame's walk left it.
    expect(moved.bones[0]!.matrixWorld.elements[12]).toBeCloseTo(10);
  });
});
