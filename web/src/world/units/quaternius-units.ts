import * as THREE from "three";
import { adaptImportedCharacterBundle } from "@moritzbrantner/three-d-renderer/imported-character";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { EntityRef } from "../../entity-ref";
import type { UnitPlacement } from "../humanoid";
import { ClipPlayback, QUATERNIUS_MODELS, type QuaterniusKind } from "./quaternius";

/**
 * Animated imported units as real three.js skinned meshes drawn through the renderer's
 * imported-roots path. Players use asset-tooling's Ranger humanoid bundle (Atlas 6.3.7), loaded
 * with 3d-lab's `adaptImportedCharacterBundle` and posed by its Rust runtime; the Quaternius CC0
 * animals (Atlas 6.3.6) are GLBs posed by three's `GLTFLoader` and `AnimationMixer`. Each model
 * loads on first use and is instantiated per unit; until it has loaded the caller keeps drawing
 * the procedural model. Presentation only.
 */
const ANIMAL_URLS: Readonly<Record<Exclude<QuaterniusKind, "Ranger">, string>> = {
  Wolf: new URL("../../../../assets/quaternius/Wolf.glb", import.meta.url).href,
  Rat: new URL("../../../../assets/quaternius/Rat.glb", import.meta.url).href,
  Pig: new URL("../../../../assets/quaternius/Pig.glb", import.meta.url).href,
};
/** The unpacked bundle `scripts/fetch-character-bundle.sh` places in `web/public`. */
const RANGER_BUNDLE = `${import.meta.env.BASE_URL}characters/humanoid.ranger.male/`;

/** Tint of a dead unit's materials, as native. */
const DEAD_TINT = 0.6;

/** One drawn model of an asset, posed by clip name and clip time. */
type Posable = { model: THREE.Object3D; pose(clip: string, time: number): void; dispose(): void };

type Asset = {
  /** Clip durations in seconds by name. */
  clips: Map<string, number>;
  instantiate(): Posable;
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
  posable: Posable;
  playback: ClipPlayback;
  clip: string;
  time: number;
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

/** Rest bounds and head of a model posed at the start of its idle clip. */
function measure(posable: Posable, idle: string): Pick<Asset, "floor" | "height" | "centre" | "head"> {
  posable.pose(idle, 0);
  const probe = posable.model;
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

async function loadAnimal(loader: GLTFLoader, kind: Exclude<QuaterniusKind, "Ranger">): Promise<Asset> {
  const gltf = await loader.loadAsync(ANIMAL_URLS[kind]);
  const sources = new Map<string, THREE.AnimationClip>();
  for (const clip of gltf.animations) {
    // Blender prefixes clips with their armature ("AnimalArmature|Walk").
    sources.set(clip.name.split("|").pop() ?? clip.name, clip);
  }
  const instantiate = (): Posable => {
    const model = cloneSkinned(gltf.scene);
    const mixer = new THREE.AnimationMixer(model);
    let current: { clip: string; action: THREE.AnimationAction } | null = null;
    return {
      model,
      pose(clip, time) {
        if (current?.clip !== clip) {
          mixer.stopAllAction();
          const source = sources.get(clip);
          current = source ? { clip, action: mixer.clipAction(source).play() } : null;
        }
        if (current) {
          // Hold a one-shot clip on its last frame rather than letting the loop wrap it to 0.
          current.action.time = Math.min(time, Math.max(0, current.action.getClip().duration - 1e-4));
          mixer.update(0);
        }
      },
      dispose: () => mixer.stopAllAction(),
    };
  };
  const clips = new Map([...sources].map(([name, clip]) => [name, clip.duration]));
  const probe = instantiate();
  return { clips, instantiate, ...measure(probe, QUATERNIUS_MODELS[kind].clips.idle) };
}

async function loadRanger(): Promise<Asset> {
  const read = async (path: string) => {
    const response = await fetch(new URL(path, new URL(RANGER_BUNDLE, window.location.href)));
    if (!response.ok) {
      throw new Error(`Ranger bundle file ${path} is missing (${response.status}); run scripts/fetch-character-bundle.sh.`);
    }
    return response.arrayBuffer();
  };
  const manifest: unknown = JSON.parse(new TextDecoder().decode(await read("manifest.json")));
  const character = await adaptImportedCharacterBundle({ manifest, key: "humanoid.ranger.male", readResource: read });
  const instantiate = (): Posable => {
    const instance = character.createInstance();
    // The runtime's clip clock holds a one-shot clip at its end like the animals.
    return { model: instance.root, pose: (clip, time) => instance.seek(clip, time), dispose: () => instance.dispose() };
  };
  const clips = new Map(character.clips.map((clip) => [clip.name, clip.duration]));
  const probe = instantiate();
  const measured = measure(probe, QUATERNIUS_MODELS.Ranger.clips.idle);
  probe.dispose();
  return { clips, instantiate, ...measured };
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
      (kind === "Ranger" ? loadRanger() : loadAnimal(this.#loader, kind)).then(
        (asset) => this.#assets.set(kind, asset),
        (error: unknown) => {
          console.warn(`Unit model ${kind} failed to load; keeping the procedural model.`, error);
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
    const posable = asset.instantiate();
    const { model } = posable;
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
      kind: unit.kind, root, posable, playback: new ClipPlayback(),
      clip: "", time: 0, materials, dead: null, head: model.getObjectByName("Head") ?? null, entity: unit.entity, cue: null,
    };
    this.#instances.set(unit.id, instance);
    this.#rootsChanged = true;
    return instance;
  }

  #pose(instance: Instance, asset: Asset, unit: QuaterniusUnit, seconds: number, deltaSeconds: number, animate: boolean): void {
    const clips = QUATERNIUS_MODELS[unit.kind].clips;
    const duration = (name: string) => asset.clips.get(name) ?? 0;
    const cue = animate && !unit.dead && unit.action && !unit.action.hold ? unit.action : null;
    if (cue && (!instance.cue || cue.progress < instance.cue.progress)) {
      instance.playback.attack(duration(clips.attack), seconds);
    }
    instance.cue = cue ? { progress: cue.progress } : null;
    const { clip, time } = instance.playback.advance(clips, duration, unit.speed, unit.dead, seconds, deltaSeconds, !animate);
    instance.clip = clip;
    instance.time = Math.min(time, Math.max(0, duration(clip) - 1e-4));
    instance.posable.pose(clip, instance.time);
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
    instance.posable.dispose();
    for (const { material } of instance.materials) { material.dispose(); }
    this.#instances.delete(id);
    this.#rootsChanged = true;
  }

  /** Whether `id` drew an imported model this frame. */
  has(id: string): boolean {
    return this.#instances.has(id);
  }

  /**
   * Debug read of a drawn unit's actual pose: its clip and clip time, dead tint, root transform, the
   * world-space extent of its posed skeleton and its head joint (metres, rounded to 0.1 mm); null
   * when no model is drawn.
   */
  pose(id: string): { clip: string; time: number; dead: boolean; root: number[]; skeleton: number[]; head: number[] | null } | null {
    const instance = this.#instances.get(id);
    if (!instance) { return null; }
    const round = (value: number) => Math.round(value * 1e4) / 1e4;
    instance.root.updateMatrixWorld(true);
    const extent = new THREE.Box3();
    instance.root.traverse((object) => {
      if (object instanceof THREE.Bone) { extent.expandByPoint(object.getWorldPosition(point)); }
    });
    const head = instance.head ? instance.head.getWorldPosition(point).toArray().map(round) : null;
    const { position, rotation, scale } = instance.root;
    return {
      clip: instance.clip, time: round(instance.time), dead: instance.dead === true,
      root: [position.x, position.y, position.z, rotation.y, scale.x].map(round),
      skeleton: extent.isEmpty() ? [] : [...extent.min.toArray(), ...extent.max.toArray()].map(round), head,
    };
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
