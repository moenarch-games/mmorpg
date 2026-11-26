import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import { BatchedRoots, CellCulledBatch } from "../../../src/world/static-batches";

// The `static-batch-views` measurement (#1113): `web/tests/static-batches.test.ts` asserts its bounds and
// `web/scripts/work-evidence.ts` emits it as work evidence. Kit modules (BatchedRoots) placed every `SPACING` metres over a
// square region are drawn from one view of fixed size, as a sun cascade sees them: the sub-draws the view issues follow
// the ground cells it sees, not how far the placements reach, and the calls stay one drawn batch per material plus one
// shadow batch per cell seen.

const SPACING = 8;
/** A sun cascade's half extent, in metres, looking straight down at the region's middle. */
const VIEW_HALF = 50;

/**
 * Builds the batches of placements over a square of side `regionMetres` and returns the sub-draws (instances of the
 * batches three keeps) and three draw calls (batches and plain meshes kept) of one view of fixed size at its middle.
 */
export function staticBatchView(regionMetres: number): { subDraws: number; calls: number; placed: number } {
  const material = new LitMaterial();
  const box = new THREE.BoxGeometry(2, 3, 2);
  const roots: THREE.Object3D[] = [];
  for (let x = SPACING / 2; x < regionMetres; x += SPACING) {
    for (let z = SPACING / 2; z < regionMetres; z += SPACING) {
      const root = new THREE.Group();
      root.matrixAutoUpdate = false;
      root.matrix.makeTranslation(x - regionMetres / 2, 1.5, z - regionMetres / 2);
      root.add(new THREE.Mesh(box, material));
      roots.push(root);
    }
  }
  const batched = new BatchedRoots("kit", roots);
  batched.root.updateMatrixWorld(true);
  const camera = new THREE.OrthographicCamera(-VIEW_HALF, VIEW_HALF, VIEW_HALF, -VIEW_HALF, 0.1, 500);
  camera.position.set(0, 200, 0);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
  let subDraws = 0;
  let calls = 0;
  for (const child of batched.root.children) {
    const mesh = child as THREE.Mesh;
    // As three's projection keeps an object: culled ones by their sphere, the rest always.
    if (mesh.frustumCulled && !frustum.intersectsObject(mesh)) { continue; }
    calls += 1;
    if (child instanceof CellCulledBatch) {
      // A drawn batch hides the cells outside the view before it draws.
      child.cull(camera);
      for (let instance = 0; instance < child.instanceCount; instance += 1) { subDraws += child.getVisibleAt(instance) ? 1 : 0; }
    } else {
      subDraws += child instanceof THREE.BatchedMesh ? child.instanceCount : 1;
    }
  }
  batched.dispose();
  return { subDraws, calls, placed: roots.length };
}
