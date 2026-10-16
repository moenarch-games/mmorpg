import * as THREE from "three";
import type { IndexedMeshGeometry, RendererInstance, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { SceneryScene } from "../../src/world/scenery-nodes";

/** Reconstruct the exact submitted geometry in world space for placement assertions. */
function transformedMesh(geometry: IndexedMeshGeometry, placement: RendererSceneNode | RendererInstance) {
  const matrix = new THREE.Matrix4();
  if (placement.modelMatrix) {
    matrix.fromArray(placement.modelMatrix);
  } else {
    matrix.compose(new THREE.Vector3(...placement.transform.translation),
      new THREE.Quaternion(...(placement.transform.rotationQuaternion ?? [0, 0, 0, 1])),
      new THREE.Vector3(...(placement.transform.scale ?? [1, 1, 1])));
  }
  return {
    positions: geometry.positions.map((point): [number, number, number] => {
      const result = new THREE.Vector3(...point).applyMatrix4(matrix);
      return [result.x, result.y, result.z];
    }),
    indices: geometry.indices,
  };
}

export function propMeshes(scene: SceneryScene) {
  const ordinary = scene.batches.filter((batch) => !batch.node.id.startsWith("static-all"))
    .map(({ node }) => transformedMesh(node.geometry, node));
  const instanced = scene.instanceBatches.flatMap(({ batch }) => {
    if (batch.geometry.kind !== "mesh") {
      throw new Error(`Unexpected non-mesh prop geometry: ${batch.id}`);
    }
    const geometry = batch.geometry;
    return batch.instances.map((instance) => transformedMesh(geometry, instance));
  });
  return [...ordinary, ...instanced];
}

