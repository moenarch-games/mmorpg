import * as THREE from "three";
import type { RendererGeometry, RendererSceneNode, RendererInstanceBatch } from "@moritzbrantner/three-d-renderer";
import type { EntityRef } from "../../entity-ref";

/**
 * Presentation picking over the exact nodes submitted to the renderer. Three.js owns
 * ray/mesh intersection; the result is only an identity for a zone selection intent.
 * Scenery and non-attackable players occlude units, while cosmetic target rings are not bodies.
 */
/** What a click lands on first: a unit, a door leaf (#158), or nothing selectable. */
export type Picked = EntityRef | { kind: "door"; id: number };

const NO_ATTACKABLE_PLAYERS: ReadonlySet<number> = new Set();

/** An invisible, presentation-only box for forgiving clicks on small visible objects. */
type PickProxy = { target: Picked; bounds: THREE.Box3 };
type PickViewport = { width: number; height: number; pointerType?: string };

const MOUSE_PICK_MARGIN_PIXELS = 8;
const TOUCH_PICK_MARGIN_PIXELS = 16;

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
    attackablePlayers: ReadonlySet<number> = NO_ATTACKABLE_PLAYERS,
    viewport?: PickViewport,
  ): Picked | null {
    this.#raycaster.setFromCamera(point, camera);
    this.#raycaster.near = camera.near;
    this.#raycaster.far = camera.far;
    let target: Picked | null = null;
    const proxies = new Map<string, PickProxy>();
    // Test bodies first, so subsequent scenery queries stop at the closest body.
    for (const nodes of [units, scenery]) {
      for (const node of nodes) {
        if (node.visible === false || node.opacity === 0 || node.id.endsWith("-target-ring")) {
          continue;
        }
        this.#material.side = node.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
        this.#place(node);
        const identity = /^unit-(player|creature|npc)-(\d+)-/.exec(node.id);
        const kind = identity?.[1];
        const selected: EntityRef | null = identity && (kind === "creature" || kind === "npc" || (kind === "player" && attackablePlayers.has(Number(identity[2]))))
          ? { kind: kind as EntityRef["kind"], id: Number(identity[2]) }
          : null;
        // Prefer the torso and head; weapons, health bars and target rings must not enlarge a hitbox.
        if (viewport && selected && (node.id.endsWith("-body") || node.id.endsWith("-head"))) {
          const geometry = this.#mesh.geometry;
          if (!geometry.boundingBox) { geometry.computeBoundingBox(); }
          if (geometry.boundingBox) {
            this.#addProxy(proxies, selected, geometry.boundingBox.clone().applyMatrix4(this.#mesh.matrixWorld));
          }
        }
        const intersections: THREE.Intersection[] = [];
        this.#mesh.raycast(this.#raycaster, intersections);
        for (const hit of intersections) {
          if (hit.distance >= this.#raycaster.far) {
            continue;
          }
          this.#raycaster.far = hit.distance;
          target = selected;
        }
      }
    }
    // Imported unit models: three.js raycasts the skinned meshes in their current pose.
    for (const { entity, object } of models) {
      object.traverse((child) => {
        if (child instanceof THREE.SkinnedMesh) { child.computeBoundingSphere(); }
      });
      if (viewport && object.visible && (entity.kind !== "player" || attackablePlayers.has(entity.id))) {
        this.#addProxy(proxies, entity, new THREE.Box3().setFromObject(object));
      }
      for (const hit of this.#raycaster.intersectObject(object, true)) {
        if (hit.distance < this.#raycaster.far) {
          this.#raycaster.far = hit.distance;
          target = entity.kind !== "player" || attackablePlayers.has(entity.id) ? entity : null;
        }
      }
    }
    // Imported buildings occlude like scenery; their door leaf is itself a target.
    for (const { door, object } of buildings) {
      if (viewport && door !== null && object.visible) {
        this.#addProxy(proxies, { kind: "door", id: door }, new THREE.Box3().setFromObject(object));
      }
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
    // Exact geometry always wins. On a miss, assist only with targets in front of
    // the closest actual ray hit, so solid scenery and ineligible units still occlude.
    return target ?? (viewport ? this.#assistedPick(point, camera, viewport, proxies, this.#raycaster.far) : null);
  }

  #addProxy(proxies: Map<string, PickProxy>, target: Picked, bounds: THREE.Box3): void {
    if (bounds.isEmpty()) { return; }
    const key = target.kind + ":" + target.id;
    const previous = proxies.get(key);
    if (previous) { previous.bounds.union(bounds); }
    else { proxies.set(key, { target, bounds }); }
  }

  /** Only near misses on visible projected bounds qualify; never substitute a distant NPC. */
  #assistedPick(
    point: THREE.Vector2,
    camera: THREE.PerspectiveCamera,
    viewport: PickViewport,
    proxies: ReadonlyMap<string, PickProxy>,
    obstructionDistance: number,
  ): Picked | null {
    const { width, height } = viewport;
    if (!(width > 0 && height > 0) || Math.abs(point.x) > 1 || Math.abs(point.y) > 1) { return null; }
    const cursorX = (point.x + 1) * width / 2;
    const cursorY = (1 - point.y) * height / 2;
    const margin = viewport.pointerType === "touch" ? TOUCH_PICK_MARGIN_PIXELS : MOUSE_PICK_MARGIN_PIXELS;
    let best: { target: Picked; miss: number; distance: number; centre: number } | null = null;
    const corner = new THREE.Vector3();
    const centre = new THREE.Vector3();
    for (const { target, bounds } of proxies.values()) {
      bounds.getCenter(centre);
      const distance = centre.distanceTo(camera.position);
      if (distance >= obstructionDistance || distance > camera.far) { continue; }
      centre.project(camera);
      if (centre.z < -1 || centre.z > 1) { continue; }
      const centrePixels = Math.hypot((centre.x + 1) * width / 2 - cursorX, (1 - centre.y) * height / 2 - cursorY);
      let left = Infinity, right = -Infinity, top = Infinity, bottom = -Infinity;
      for (const x of [bounds.min.x, bounds.max.x]) {
        for (const y of [bounds.min.y, bounds.max.y]) {
          for (const z of [bounds.min.z, bounds.max.z]) {
            corner.set(x, y, z).project(camera);
            const screenX = (corner.x + 1) * width / 2;
            const screenY = (1 - corner.y) * height / 2;
            left = Math.min(left, screenX);
            right = Math.max(right, screenX);
            top = Math.min(top, screenY);
            bottom = Math.max(bottom, screenY);
          }
        }
      }
      const miss = Math.hypot(Math.max(left - cursorX, 0, cursorX - right), Math.max(top - cursorY, 0, cursorY - bottom));
      if (miss > margin) { continue; }
      if (!best || miss < best.miss - 0.01
        || (Math.abs(miss - best.miss) <= 0.01 && (distance < best.distance - 0.01
          || (Math.abs(distance - best.distance) <= 0.01 && centrePixels < best.centre)))) {
        best = { target, miss, distance, centre: centrePixels };
      }
    }
    return best?.target ?? null;
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
