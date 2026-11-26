import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { GroupBuffers, GroupStreamSource, StaticBatch } from "../src/world/scenery-nodes";
import { BATCH_CELL, BatchedRoots, BatchedScenery, CellCulledBatch, SHADOW_ONLY } from "../src/world/static-batches";
import { staticBatchView } from "./support/work/static-batch-views";

/** `.performance/contract.json`'s `static-batch-views` budgets. */
const STATIC_BATCH_VIEW_SUB_DRAWS = 1536;
const STATIC_BATCH_VIEW_CALLS = 13;

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
    // The opaque draws' batch, and their shared shadow-only caster batch (#671).
    const drawn = batches.filter((batch) => batch.material === opaque);
    expect(drawn).toHaveLength(1);
    expect(drawn[0]!.instanceCount).toBe(3);
    const casters = batches.filter((batch) => batch.material === SHADOW_ONLY.single);
    expect(casters).toHaveLength(1);
    expect(casters[0]!.instanceCount).toBe(3);
    expect(batches).toHaveLength(2);
    expect(batched.root.children.filter((child) => !(child instanceof THREE.BatchedMesh))).toHaveLength(1);
  });

  test("a synced root moves, hides and shows its instances and copies", () => {
    const leaf = new THREE.Mesh(box, opaque);
    leaf.position.set(0, 1, 0);
    const pane = new THREE.Mesh(box, glass);
    const door = root("door", leaf, pane);
    const batched = new BatchedRoots("inn", [door]);
    const material = (child: THREE.Object3D) => (child as THREE.Mesh).material;
    // A lone opaque draw stays its own mesh (#834) and still casts through the shadow batch (#671).
    const own = batched.root.children.find((child) => !(child instanceof THREE.BatchedMesh) && material(child) === opaque)!;
    const caster = batched.root.children.find((child) => child instanceof THREE.BatchedMesh && material(child) === SHADOW_ONLY.single) as THREE.BatchedMesh;
    const copy = batched.root.children.find((child) => material(child) === glass)!;
    const at = new THREE.Matrix4();
    expect(new THREE.Vector3().setFromMatrixPosition(own.matrix).toArray()).toEqual([0, 1, 0]);
    expect(new THREE.Vector3().setFromMatrixPosition(caster.getMatrixAt(0, at)).toArray()).toEqual([0, 1, 0]);

    door.matrix.makeTranslation(5, 0, 2);
    batched.sync(0);
    expect(new THREE.Vector3().setFromMatrixPosition(own.matrix).toArray()).toEqual([5, 1, 2]);
    expect(new THREE.Vector3().setFromMatrixPosition(caster.getMatrixAt(0, at)).toArray()).toEqual([5, 1, 2]);
    expect(new THREE.Vector3().setFromMatrixPosition(copy.matrix).toArray()).toEqual([5, 0, 2]);

    door.visible = false;
    batched.sync(0);
    expect(own.visible).toBe(false);
    expect(caster.getVisibleAt(0)).toBe(false);
    expect(copy.visible).toBe(false);
    door.visible = true;
    leaf.visible = false;
    batched.sync(0);
    expect(own.visible).toBe(false);
    expect(caster.getVisibleAt(0)).toBe(false);
    expect(copy.visible).toBe(true);
  });

  test("a drawn batch hides the cells outside the main view; shadow batches split by cell and grow as draws move", () => {
    const near = root("near", new THREE.Mesh(box, opaque), new THREE.Mesh(box, opaque));
    const far = root("far", new THREE.Mesh(box, opaque), new THREE.Mesh(box, opaque));
    far.matrix.makeTranslation(BATCH_CELL * 3, 0, 0);
    const batched = new BatchedRoots("kit", [near, far]);
    const batches = batched.root.children.filter((child) => child instanceof THREE.BatchedMesh) as THREE.BatchedMesh[];
    // One drawn batch for the material, culling two cells; one caster batch per cell, culled whole.
    const drawn = batches.filter((batch) => batch instanceof CellCulledBatch) as CellCulledBatch[];
    expect(drawn).toHaveLength(1);
    expect(drawn[0]!.cellCount).toBe(2);
    expect(drawn[0]!.frustumCulled).toBe(false);
    const casters = batches.filter((batch) => batch.material === SHADOW_ONLY.single);
    expect(casters).toHaveLength(2);
    for (const caster of casters) {
      expect(caster.frustumCulled).toBe(true);
      expect(caster.instanceCount).toBe(2);
    }

    // A camera at the origin looking along -z sees the near cell only.
    const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 50);
    camera.position.set(0, 1, 5);
    camera.updateMatrixWorld(true);
    const shownCount = () => [0, 1, 2, 3].filter((instance) => drawn[0]!.getVisibleAt(instance)).length;
    drawn[0]!.cull(camera);
    expect(shownCount()).toBe(2);
    // A hidden draw stays hidden when its cell comes back into view, and a shown one in a cell out of view shows on return.
    far.visible = false;
    batched.sync(1);
    camera.position.set(BATCH_CELL * 3, 1, 5);
    camera.updateMatrixWorld(true);
    drawn[0]!.cull(camera);
    expect(shownCount()).toBe(0);
    far.visible = true;
    batched.sync(1);
    expect(shownCount()).toBe(2);

    const nearCaster = casters.find((caster) => caster.boundingSphere!.center.x < BATCH_CELL)!;
    expect(nearCaster.boundingSphere!.containsPoint(new THREE.Vector3(BATCH_CELL * 3, 0, 0))).toBe(false);
    near.matrix.makeTranslation(BATCH_CELL * 3, 0, 0);
    batched.sync(0);
    expect(nearCaster.boundingSphere!.containsPoint(new THREE.Vector3(BATCH_CELL * 3, 0, 0))).toBe(true);
    // The moved draws' cell grew with them: the drawn batch shows them from there.
    drawn[0]!.cull(camera);
    expect(shownCount()).toBe(4);
  });
});

// The `static-batch-views` budget (#1113, `.performance/contract.json`): a fixed view of placed kit modules issues the
// sub-draws of the ground cells it sees, however far the placements reach. `support/work/static-batch-views.ts` measures
// it; `web/scripts/work-evidence.ts` emits it.
describe("static-batch-views", () => {
  test("a view's sub-draws stay bounded as the placed region grows", () => {
    const small = staticBatchView(256);
    const large = staticBatchView(1024);
    expect(large.placed).toBe(small.placed * 16);
    expect(large.subDraws).toBe(small.subDraws);
    expect(large.subDraws).toBeLessThanOrEqual(STATIC_BATCH_VIEW_SUB_DRAWS);
    expect(large.calls).toBeLessThanOrEqual(STATIC_BATCH_VIEW_CALLS);
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
  const kits = { outpost: false, settlementProps: false, nature: false, limestone: false, climbing: false, crag: false, dressed: new Uint32Array() };
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
