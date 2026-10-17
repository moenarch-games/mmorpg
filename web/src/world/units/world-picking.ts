import * as THREE from "three";
import type { RendererGeometry, RendererSceneNode, RendererInstanceBatch } from "@moritzbrantner/three-d-renderer";
import type { EntityRef } from "../../entity-ref";

/**
 * Presentation picking over the exact nodes submitted to the renderer. Three.js owns
 * ray/mesh intersection; the result is only an identity for a zone selection intent.
 * Scenery and other players occlude units, while cosmetic target rings are not bodies.
 */
/** What a click lands on first: a unit, a door leaf (#158), or nothing selectable. */
export type Picked = EntityRef | { kind: "door"; id: number };

export class WorldPicking {
  readonly #raycaster = new THREE.Raycaster();
  readonly #material = new THREE.MeshBasicMaterial();
  readonly #mesh = new THREE.Mesh<THREE.BufferGeometry>(new THREE.BoxGeometry(1, 1, 1), this.#material);
  readonly #box = this.#mesh.geometry;
  readonly #sphere = new THREE.SphereGeometry(1, 20, 12);
  readonly #cylinder = new THREE.CylinderGeometry(1, 1, 1, 20);
  readonly #meshes = new Map<string, THREE.BufferGeometry>();
  readonly #translation = new THREE.Vector3();
  readonly #rotation = new THREE.Quaternion();
  readonly #scale = new THREE.Vector3();

  /** Immutable scenery resource keys have the same lifetime as the loaded world. */
  reset(): void {
    for (const geometry of this.#meshes.values()) {
      geometry.dispose();
    }
    this.#meshes.clear();
  }

  pick(
    point: THREE.Vector2,
    camera: THREE.PerspectiveCamera,
    scenery: readonly RendererSceneNode[],
    units: readonly RendererSceneNode[],
    instanceBatches: readonly RendererInstanceBatch[] = [],
    models: readonly { entity: EntityRef; object: THREE.Object3D }[] = [],
    buildings: readonly { door: number | null; object: THREE.Object3D }[] = [],
  ): Picked | null {
    this.#raycaster.setFromCamera(point, camera);
    this.#raycaster.near = camera.near;
    this.#raycaster.far = camera.far;
    let target: Picked | null = null;
    // Test bodies first, so subsequent scenery queries stop at the closest body.
    for (const nodes of [units, scenery]) {
      for (const node of nodes) {
        if (node.visible === false || node.opacity === 0 || node.id.endsWith("-target-ring")) {
          continue;
        }
        this.#material.side = node.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
        this.#place(node);
        const intersections: THREE.Intersection[] = [];
        this.#mesh.raycast(this.#raycaster, intersections);
        for (const hit of intersections) {
          if (hit.distance >= this.#raycaster.far) {
            continue;
          }
          this.#raycaster.far = hit.distance;
          const identity = /^unit-(creature|npc)-(\d+)-/.exec(node.id);
          const kind = identity?.[1];
          target = identity && (kind === "creature" || kind === "npc")
            ? { kind, id: Number(identity[2]) }
            : null;
        }
      }
    }
    // Imported unit models: three.js raycasts the skinned meshes in their current pose.
    for (const { entity, object } of models) {
      object.traverse((child) => {
        if (child instanceof THREE.SkinnedMesh) { child.computeBoundingSphere(); }
      });
      for (const hit of this.#raycaster.intersectObject(object, true)) {
        if (hit.distance < this.#raycaster.far) {
          this.#raycaster.far = hit.distance;
          target = entity.kind === "creature" || entity.kind === "npc" ? entity : null;
        }
      }
    }
    // Imported buildings occlude like scenery; their door leaf is itself a target.
    for (const { door, object } of buildings) {
      for (const hit of this.#raycaster.intersectObject(object, true)) {
        if (hit.distance < this.#raycaster.far) {
          this.#raycaster.far = hit.distance;
          target = door === null ? null : { kind: "door", id: door };
        }
      }
    }
    for (const batch of instanceBatches) {
      if (batch.visible === false || batch.opacity === 0) {
        continue;
      }
      for (const instance of batch.instances) {
        this.#material.side = batch.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
        this.#place({ id: batch.id, geometry: batch.geometry, color: batch.color, ...instance });
        const hits: THREE.Intersection[] = [];
        this.#mesh.raycast(this.#raycaster, hits);
        for (const hit of hits) {
          if (hit.distance < this.#raycaster.far) {
            this.#raycaster.far = hit.distance;
            target = null;
          }
        }
      }
    }
    return target;
  }

  #place(node: RendererSceneNode): void {
    const mesh = this.#mesh;
    this.#scale.set(1, 1, 1);
    mesh.geometry = this.#geometry(node.geometry);
    if (node.modelMatrix) {
      mesh.matrixWorld.fromArray(node.modelMatrix);
    } else {
      const transform = node.transform;
      this.#translation.fromArray(transform.translation);
      this.#rotation.fromArray(transform.rotationQuaternion ?? [0, 0, 0, 1]);
      this.#scale.fromArray(transform.scale ?? [1, 1, 1]);
      mesh.matrixWorld.compose(this.#translation, this.#rotation, this.#scale);
    }
    // The primitives share normalized geometry; scale it to the renderer's exact dimensions.
    switch (node.geometry.kind) {
      case "box":
        mesh.matrixWorld.scale(this.#scale.fromArray(node.geometry.size));
        break;
      case "sphere":
        mesh.matrixWorld.scale(this.#scale.setScalar(node.geometry.radius));
        break;
      case "cylinder":
        mesh.matrixWorld.scale(this.#scale.set(node.geometry.radius, node.geometry.height, node.geometry.radius));
        break;
      case "mesh":
        break;
    }
  }

  #geometry(geometry: RendererGeometry): THREE.BufferGeometry {
    switch (geometry.kind) {
      case "box": return this.#box;
      case "sphere": return this.#sphere;
      case "cylinder": return this.#cylinder;
      case "mesh": {
        const previous = this.#meshes.get(geometry.resourceKey);
        if (previous) {
          return previous;
        }
        const mesh = new THREE.BufferGeometry();
        mesh.setAttribute("position", new THREE.Float32BufferAttribute(geometry.positions.flat(), 3));
        mesh.setIndex([...geometry.indices]);
        mesh.computeBoundingSphere();
        this.#meshes.set(geometry.resourceKey, mesh);
        return mesh;
      }
    }
  }
}
