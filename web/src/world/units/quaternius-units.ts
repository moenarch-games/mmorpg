import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { EntityRef } from "../../entity-ref";
import type { UnitPlacement } from "../humanoid";
import { ClipPlayback, QUATERNIUS_MODELS, type QuaterniusKind } from "./quaternius";

/**
 * Animated Quaternius units (Atlas 6.3.6) as real three.js skinned meshes, posed by three's own
 * `GLTFLoader`, `SkeletonUtils.clone` and `AnimationMixer` and drawn through the renderer's
 * imported-roots path. Each GLB is fetched on first use and cloned per unit; until it has loaded
 * the caller keeps drawing the procedural model. Presentation only.
 */
const URLS: Readonly<Record<QuaterniusKind, string>> = {
  KnightMale: new URL("../../../../assets/quaternius/Knight_Male.glb", import.meta.url).href,
  KnightFemale: new URL("../../../../assets/quaternius/Knight_Golden_Female.glb", import.meta.url).href,
  Elf: new URL("../../../../assets/quaternius/Elf.glb", import.meta.url).href,
  NinjaFemale: new URL("../../../../assets/quaternius/Ninja_Female.glb", import.meta.url).href,
  Wizard: new URL("../../../../assets/quaternius/Wizard.glb", import.meta.url).href,
  Witch: new URL("../../../../assets/quaternius/Witch.glb", import.meta.url).href,
  VikingMale: new URL("../../../../assets/quaternius/Viking_Male.glb", import.meta.url).href,
  VikingFemale: new URL("../../../../assets/quaternius/Viking_Female.glb", import.meta.url).href,
  GoblinMale: new URL("../../../../assets/quaternius/Goblin_Male.glb", import.meta.url).href,
  GoblinFemale: new URL("../../../../assets/quaternius/Goblin_Female.glb", import.meta.url).href,
  PirateMale: new URL("../../../../assets/quaternius/Pirate_Male.glb", import.meta.url).href,
  PirateFemale: new URL("../../../../assets/quaternius/Pirate_Female.glb", import.meta.url).href,
  SoldierMale: new URL("../../../../assets/quaternius/Soldier_Male.glb", import.meta.url).href,
  SoldierFemale: new URL("../../../../assets/quaternius/Soldier_Female.glb", import.meta.url).href,
  WorkerMale: new URL("../../../../assets/quaternius/Worker_Male.glb", import.meta.url).href,
  WorkerFemale: new URL("../../../../assets/quaternius/Worker_Female.glb", import.meta.url).href,
  CasualMale: new URL("../../../../assets/quaternius/Casual_Male.glb", import.meta.url).href,
  CasualFemale: new URL("../../../../assets/quaternius/Casual_Female.glb", import.meta.url).href,
  Wolf: new URL("../../../../assets/quaternius/Wolf.glb", import.meta.url).href,
  Rat: new URL("../../../../assets/quaternius/Rat.glb", import.meta.url).href,
  Pig: new URL("../../../../assets/quaternius/Pig.glb", import.meta.url).href,
};

/** Tint of a dead unit's materials, as native. */
const DEAD_TINT = 0.6;

type Asset = {
  scene: THREE.Object3D;
  clips: Map<string, THREE.AnimationClip>;
  /** Rest (idle at 0) bounds: lowest point, height and horizontal centre, model space. */
  floor: number;
  height: number;
  centre: [number, number];
  /** The `Head` joint's rest head centre in the joint's frame and the head radius, model space. */
  head: { offset: THREE.Vector3; radius: number } | null;
};

type Instance = {
  kind: QuaterniusKind;
  root: THREE.Group;
  mixer: THREE.AnimationMixer;
  playback: ClipPlayback;
  action: THREE.AnimationAction | null;
  clip: string;
  materials: { material: THREE.MeshStandardMaterial; base: THREE.Color }[];
  dead: boolean | null;
  head: THREE.Object3D | null;
  entity: EntityRef;
  /** Whether the last frame's action cue was already counted as an attack. */
  cue: { progress: number } | null;
};

/** One unit to present this frame. */
export type QuaterniusUnit = {
  id: string;
  entity: EntityRef;
  kind: QuaterniusKind;
  placement: UnitPlacement;
  /** Collision height in metres; the model is scaled to it. */
  heightMetres: number;
  /** Planar speed in m/s. */
  speed: number;
  dead: boolean;
  /** The unit's current #27 action cue; a new non-held cue starts the attack clip. */
  action: { progress: number; hold: boolean } | null;
};

const point = new THREE.Vector3();

function measure(scene: THREE.Object3D, idle: THREE.AnimationClip | undefined): Pick<Asset, "floor" | "height" | "centre" | "head"> {
  const probe = cloneSkinned(scene);
  if (idle) {
    const mixer = new THREE.AnimationMixer(probe);
    mixer.clipAction(idle).play();
    mixer.update(0);
  }
  probe.updateMatrixWorld(true);
  const box = new THREE.Box3();
  const headBox = new THREE.Box3();
  const headJoint = probe.getObjectByName("Head") ?? null;
  probe.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) { return; }
    const position = object.geometry.getAttribute("position");
    const skinIndex = object instanceof THREE.SkinnedMesh ? object.geometry.getAttribute("skinIndex") : undefined;
    const skinWeight = object instanceof THREE.SkinnedMesh ? object.geometry.getAttribute("skinWeight") : undefined;
    for (let i = 0; i < position.count; i++) {
      object.getVertexPosition(i, point);
      point.applyMatrix4(object.matrixWorld);
      box.expandByPoint(point);
      if (headJoint && object instanceof THREE.SkinnedMesh && skinIndex && skinWeight) {
        let best = 0;
        let bone = -1;
        for (let k = 0; k < 4; k++) {
          const weight = skinWeight.getComponent(i, k);
          if (weight > best) { best = weight; bone = skinIndex.getComponent(i, k); }
        }
        if (object.skeleton.bones[bone] === headJoint) { headBox.expandByPoint(point); }
      }
    }
  });
  if (box.isEmpty()) {
    return { floor: 0, height: 1, centre: [0, 0], head: null };
  }
  let head: Asset["head"] = null;
  if (headJoint && !headBox.isEmpty()) {
    const centre = headBox.getCenter(new THREE.Vector3());
    const size = headBox.getSize(new THREE.Vector3());
    head = { offset: headJoint.worldToLocal(centre), radius: Math.max(size.x, size.y, size.z) / 2 };
  }
  return { floor: box.min.y, height: box.max.y - box.min.y, centre: [(box.min.x + box.max.x) / 2, (box.min.z + box.max.z) / 2], head };
}

async function loadAsset(loader: GLTFLoader, kind: QuaterniusKind): Promise<Asset> {
  const gltf = await loader.loadAsync(URLS[kind]);
  const clips = new Map<string, THREE.AnimationClip>();
  for (const clip of gltf.animations) {
    // Blender prefixes clips with their armature ("AnimalArmature|Walk").
    clips.set(clip.name.split("|").pop() ?? clip.name, clip);
  }
  return { scene: gltf.scene, clips, ...measure(gltf.scene, clips.get(QUATERNIUS_MODELS[kind].clips.idle)) };
}

export class QuaterniusUnits {
  readonly #loader = new GLTFLoader();
  readonly #assets = new Map<QuaterniusKind, Asset | "loading" | "failed">();
  readonly #instances = new Map<string, Instance>();
  #rootsChanged = false;

  /** Starts loading `kind`; true once it can be drawn. */
  ready(kind: QuaterniusKind): boolean {
    const state = this.#assets.get(kind);
    if (state === undefined) {
      this.#assets.set(kind, "loading");
      loadAsset(this.#loader, kind).then(
        (asset) => this.#assets.set(kind, asset),
        (error: unknown) => {
          console.warn(`Quaternius model ${kind} failed to load; keeping the procedural model.`, error);
          this.#assets.set(kind, "failed");
        },
      );
      return false;
    }
    return typeof state === "object";
  }

  /**
   * Poses every given unit (whose model is `ready`) for this frame and drops the instances of
   * units not given, so interest exits clean up. `seconds` is the presentation clock.
   */
  update(units: readonly QuaterniusUnit[], seconds: number, deltaSeconds: number, animate: boolean): void {
    const seen = new Set<string>();
    for (const unit of units) {
      const asset = this.#assets.get(unit.kind);
      if (typeof asset !== "object") { continue; }
      seen.add(unit.id);
      let instance = this.#instances.get(unit.id);
      if (instance && instance.kind !== unit.kind) {
        this.#drop(unit.id, instance);
        instance = undefined;
      }
      instance ??= this.#create(unit, asset);
      this.#pose(instance, asset, unit, seconds, deltaSeconds, animate);
    }
    for (const [id, instance] of this.#instances) {
      if (!seen.has(id)) { this.#drop(id, instance); }
    }
  }

  #create(unit: QuaterniusUnit, asset: Asset): Instance {
    const model = cloneSkinned(asset.scene);
    const materials: Instance["materials"] = [];
    model.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) { return; }
      object.castShadow = true;
      // Animated bounds are not tracked; a handful of units are cheaper than stale culling.
      object.frustumCulled = false;
      const own = (material: THREE.Material): THREE.Material => {
        const copy = material.clone();
        if (copy instanceof THREE.MeshStandardMaterial) { materials.push({ material: copy, base: copy.color.clone() }); }
        return copy;
      };
      object.material = Array.isArray(object.material) ? object.material.map(own) : own(object.material);
    });
    model.position.set(-asset.centre[0], -asset.floor, -asset.centre[1]);
    const root = new THREE.Group();
    root.name = `${unit.id}-model`;
    root.add(model);
    const instance: Instance = {
      kind: unit.kind, root, mixer: new THREE.AnimationMixer(model), playback: new ClipPlayback(),
      action: null, clip: "", materials, dead: null, head: model.getObjectByName("Head") ?? null, entity: unit.entity, cue: null,
    };
    this.#instances.set(unit.id, instance);
    this.#rootsChanged = true;
    return instance;
  }

  #pose(instance: Instance, asset: Asset, unit: QuaterniusUnit, seconds: number, deltaSeconds: number, animate: boolean): void {
    const clips = QUATERNIUS_MODELS[unit.kind].clips;
    const duration = (name: string) => asset.clips.get(name)?.duration ?? 0;
    const cue = animate && !unit.dead && unit.action && !unit.action.hold ? unit.action : null;
    if (cue && (!instance.cue || cue.progress < instance.cue.progress)) {
      instance.playback.attack(duration(clips.attack), seconds);
    }
    instance.cue = cue ? { progress: cue.progress } : null;
    const { clip, time } = instance.playback.advance(clips, duration, unit.speed, unit.dead, seconds, deltaSeconds, !animate);
    if (clip !== instance.clip) {
      instance.clip = clip;
      instance.mixer.stopAllAction();
      const source = asset.clips.get(clip);
      instance.action = source ? instance.mixer.clipAction(source).play() : null;
    }
    if (instance.action) {
      // Hold a one-shot clip on its last frame rather than letting the loop wrap it to 0.
      instance.action.time = Math.min(time, Math.max(0, instance.action.getClip().duration - 1e-4));
      instance.mixer.update(0);
    }
    if (instance.dead !== unit.dead) {
      instance.dead = unit.dead;
      for (const { material, base } of instance.materials) {
        material.color.copy(base).multiplyScalar(unit.dead ? DEAD_TINT : 1);
      }
    }
    const { root } = instance;
    root.position.set(unit.placement.x, unit.placement.feetY, unit.placement.z);
    root.rotation.set(0, unit.placement.yawRadians, 0);
    root.scale.setScalar(unit.heightMetres / asset.height);
  }

  #drop(id: string, instance: Instance): void {
    instance.mixer.stopAllAction();
    for (const { material } of instance.materials) { material.dispose(); }
    this.#instances.delete(id);
    this.#rootsChanged = true;
  }

  /** Whether `id` drew an imported model this frame. */
  has(id: string): boolean {
    return this.#instances.has(id);
  }

  /** The posed head centre (metres, world) and radius of a drawn unit, for explicit headwear. */
  head(id: string): { centre: [number, number, number]; radius: number } | null {
    const instance = this.#instances.get(id);
    const asset = instance ? this.#assets.get(instance.kind) : undefined;
    if (!instance?.head || typeof asset !== "object" || !asset.head) { return null; }
    instance.root.updateMatrixWorld(true);
    const centre = instance.head.localToWorld(point.copy(asset.head.offset));
    return { centre: [centre.x, centre.y, centre.z], radius: asset.head.radius * instance.root.scale.x };
  }

  /** The roots to draw, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#rootsChanged;
    this.#rootsChanged = false;
    return { roots: [...this.#instances.values()].map((instance) => instance.root), changed };
  }

  /** The drawn models, for picking against their skinned meshes. */
  pickables(): { entity: EntityRef; object: THREE.Object3D }[] {
    return [...this.#instances.values()].map((instance) => ({ entity: instance.entity, object: instance.root }));
  }

  clear(): void {
    for (const [id, instance] of this.#instances) { this.#drop(id, instance); }
  }
}
