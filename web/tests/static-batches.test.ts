import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { GroupBuffers, GroupStreamSource, StaticBatch } from "../src/world/scenery-nodes";
import { BatchedRoots, BatchedScenery } from "../src/world/static-batches";

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

// #526: static groups stream through growing batches; only residents exist, in one batch per material.
describe("BatchedScenery", () => {
  const buffers = (shift: number): GroupBuffers => ({
    positions: Float32Array.of(shift, 0, 0, shift + 1, 0, 0, shift, 1, 0),
    normals: Float32Array.of(0, 0, 1, 0, 0, 1, 0, 0, 1),
    colors: Float32Array.of(1, 1, 1, 1, 1, 1, 1, 1, 1),
    indices: Uint32Array.of(0, 1, 2),
  });
  const group = (index: number) => ({
    cull: "near" as const, center: [index * 10, 0] as const, radius: 1, group: index,
    node: { id: `g${index}`, color: "#ff0000", modelMatrix: new THREE.Matrix4().makeTranslation(index, 0, 0).toArray() } as unknown as StaticBatch["node"],
    buffers: () => buffers(index),
  }) satisfies StaticBatch;
  const updates: number[][] = [];
  const stream: GroupStreamSource = { update: () => Uint32Array.from(updates.shift() ?? [0, 0]) };
  const kits = { outpost: false, settlementProps: false, nature: false, climbing: false, crag: false, dressed: new Uint32Array() };
  const scenery = () => new BatchedScenery({ batches: [group(0), group(1), group(2)], instanceBatches: [], water: [], animated: [] }, stream);

  test("a group draws only once resident and an evicted one frees its instance", () => {
    const batched = scenery();
    const draws = batched.root.children.filter((child) => child instanceof THREE.BatchedMesh && !child.name.includes(":shadow")) as THREE.BatchedMesh[];
    expect(batched.observe()).toEqual({ groups: 3, resident: 0, residentBatches: 0 });
    expect(draws[0]!.instanceCount).toBe(0);
    updates.push([2, 0, 2, 0]);
    batched.stream([0, 0, 0], kits);
    expect(batched.resident(0)).toBe(true);
    expect(batched.resident(1)).toBe(false);
    expect(batched.observe().residentBatches).toBe(2);
    expect(draws[0]!.instanceCount).toBe(2);
    // Built hidden; the frame's nodes show it (a resident group that is culled stays hidden).
    expect(draws[0]!.getVisibleAt(0)).toBe(false);
    batched.show([{ visible: true }, {}, {}] as unknown as RendererSceneNode[]);
    expect(draws[0]!.getVisibleAt(0)).toBe(true);
    updates.push([0, 1, 0]);
    batched.stream([100, 0, 0], kits);
    expect(batched.resident(0)).toBe(false);
    expect(batched.observe().residentBatches).toBe(1);
    expect(draws[0]!.instanceCount).toBe(1);
    batched.dispose();
  });

  test("growing past a batch's first size keeps the resident draws", () => {
    const batched = scenery();
    const draws = batched.root.children.filter((child) => child instanceof THREE.BatchedMesh && !child.name.includes(":shadow")) as THREE.BatchedMesh[];
    updates.push([3, 0, 1, 2, 0]);
    batched.stream([0, 0, 0], kits);
    expect(draws[0]!.instanceCount).toBe(3);
    batched.dispose();
  });
});
