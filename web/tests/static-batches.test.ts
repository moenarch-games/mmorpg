import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import { BatchedRoots } from "../src/world/static-batches";

// Walk-in buildings and kit modules draw through BatchedMeshes (#615): the batches follow their source roots.
describe("BatchedRoots", () => {
  const opaque = new LitMaterial();
  const glass = new LitMaterial({ alphaMode: "blend", alpha: 0.4 });
  const box = new THREE.BoxGeometry(1, 1, 1);
  const root = (name: string, ...meshes: THREE.Mesh[]): THREE.Object3D => {
    const group = new THREE.Group();
    group.name = name;
    group.matrixAutoUpdate = false;
    for (const mesh of meshes) { group.add(mesh); }
    return group;
  };

  test("one batch per material holds every opaque mesh; a blended one stays a plain copy", () => {
    const shell = root("shell", new THREE.Mesh(box, opaque), new THREE.Mesh(new THREE.SphereGeometry(1), opaque));
    const door = root("door", new THREE.Mesh(box, opaque), new THREE.Mesh(box, glass));
    const batched = new BatchedRoots("inn", [shell, door]);
    const batches = batched.root.children.filter((child) => child instanceof THREE.BatchedMesh) as THREE.BatchedMesh[];
    expect(batches).toHaveLength(1);
    expect(batches[0]!.instanceCount).toBe(3);
    expect(batched.root.children.filter((child) => !(child instanceof THREE.BatchedMesh))).toHaveLength(1);
  });

  test("a synced root moves, hides and shows its instances and copies", () => {
    const leaf = new THREE.Mesh(box, opaque);
    leaf.position.set(0, 1, 0);
    const pane = new THREE.Mesh(box, glass);
    const door = root("door", leaf, pane);
    const batched = new BatchedRoots("inn", [door]);
    const batch = batched.root.children.find((child) => child instanceof THREE.BatchedMesh) as THREE.BatchedMesh;
    const copy = batched.root.children.find((child) => !(child instanceof THREE.BatchedMesh))!;
    const at = new THREE.Matrix4();
    expect(new THREE.Vector3().setFromMatrixPosition(batch.getMatrixAt(0, at)).toArray()).toEqual([0, 1, 0]);

    door.matrix.makeTranslation(5, 0, 2);
    batched.sync(0);
    expect(new THREE.Vector3().setFromMatrixPosition(batch.getMatrixAt(0, at)).toArray()).toEqual([5, 1, 2]);
    expect(new THREE.Vector3().setFromMatrixPosition(copy.matrix).toArray()).toEqual([5, 0, 2]);

    door.visible = false;
    batched.sync(0);
    expect(batch.getVisibleAt(0)).toBe(false);
    expect(copy.visible).toBe(false);
    door.visible = true;
    leaf.visible = false;
    batched.sync(0);
    expect(batch.getVisibleAt(0)).toBe(false);
    expect(copy.visible).toBe(true);
  });
});
