import * as THREE from "three";
import {
  adaptImportedCharacterBundle,
  adaptImportedItemBundle,
  readBundleAttachment,
  type ImportedItemAsset,
} from "@moritzbrantner/three-d-renderer/imported-character";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { EntityRef } from "../../entity-ref";
import type { ImportedModelKind, ImportedRequest } from "../unit-presenter";

type QuaterniusKind = ImportedModelKind;
/** What a player model carries: a Warden's sword and shield, or nothing (#156). */
type Loadout = "sword-and-shield" | null;
/** The rest (idle) clip a model is measured in, from the shared presenter's clip table. */
export type RestClip = (kind: QuaterniusKind) => string;

/**
 * Animated imported units as real three.js skinned meshes drawn through the renderer's
 * imported-roots path. Players use asset-tooling's Ranger humanoid bundle (Atlas 6.3.7), loaded
 * with 3d-lab's `adaptImportedCharacterBundle` and posed by its Rust runtime; the Quaternius CC0
 * animals (Atlas 6.3.6) are GLBs posed by three's `GLTFLoader` and `AnimationMixer`. Each model
 * loads on first use and is instantiated per unit; until it has loaded the shared presenter keeps
 * drawing the procedural model. Clip choice and clock come from the shared presenter (#184).
 * Presentation only.
 */
const ANIMAL_URLS: Readonly<Record<Exclude<QuaterniusKind, "Ranger">, string>> = {
  Wolf: new URL("../../../../assets/quaternius/Wolf.glb", import.meta.url).href,
  Rat: new URL("../../../../assets/quaternius/Rat.glb", import.meta.url).href,
  Pig: new URL("../../../../assets/quaternius/Pig.glb", import.meta.url).href,
};
/** The unpacked bundle `scripts/fetch-character-bundle.sh` places in `web/public`. */
const RANGER_BUNDLE = `${import.meta.env.BASE_URL}characters/humanoid.ranger.male/`;

/** One drawn model of an asset, posed by clip name and clip time. */
type Posable = { model: THREE.Object3D; pose(clip: string, time: number): void; dispose(): void };

type Asset = {
  /** Clip durations in seconds by name. */
  clips: Map<string, number>;
  instantiate(loadout: Loadout): Posable;
  /** Rest (idle at 0) bounds: lowest point, height and horizontal centre, model space. */
  floor: number;
  height: number;
  centre: [number, number];
  /** The `Head` joint's rest head centre in the joint's frame and the head radius, model space. */
  head: { offset: THREE.Vector3; radius: number } | null;
};

type Instance = {
  kind: QuaterniusKind;
  loadout: Loadout;
  root: THREE.Group;
  posable: Posable;
  clip: string;
  time: number;
  materials: { material: THREE.MeshStandardMaterial; base: THREE.Color }[];
  dead: boolean | null;
  head: THREE.Object3D | null;
  entity: EntityRef;
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

async function loadAnimal(loader: GLTFLoader, kind: Exclude<QuaterniusKind, "Ranger">, rest: string): Promise<Asset> {
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
  return { clips, instantiate, ...measure(probe, rest) };
}

/** A humanoid-sockets descriptor socket as a column-major matrix in its joint's frame. */
type Socket = { joint: string; matrix: number[] };

function socketsOf(descriptor: unknown): Map<string, Socket> {
  const sockets = new Map<string, Socket>();
  const list = (descriptor as { sockets?: unknown[] } | null)?.sockets;
  if (!Array.isArray(list)) {
    throw new Error("The Ranger bundle's humanoid-sockets descriptor has no sockets.");
  }
  for (const entry of list as { id: string; joint: string; translation: number[]; rotation: number[] }[]) {
    const [x = 0, y = 0, z = 0] = entry.translation;
    const [qx = 0, qy = 0, qz = 0, qw = 1] = entry.rotation;
    const matrix = new THREE.Matrix4().compose(new THREE.Vector3(x, y, z), new THREE.Quaternion(qx, qy, qz, qw), new THREE.Vector3(1, 1, 1));
    sockets.set(entry.id, { joint: entry.joint, matrix: matrix.toArray() });
  }
  return sockets;
}

/** Which bundle item goes to which socket, per loadout: consumer data for 3d-lab's attach. */
const LOADOUT_ITEMS = {
  "sword-and-shield": [
    { key: "equipment.sword.arming", socket: "hand_r.weapon" },
    { key: "equipment.shield.heater", socket: "hand_l.shield" },
  ],
} as const;

async function loadRanger(rest: string): Promise<Asset> {
  const read = async (path: string) => {
    const response = await fetch(new URL(path, new URL(RANGER_BUNDLE, window.location.href)));
    if (!response.ok) {
      throw new Error(`Ranger bundle file ${path} is missing (${response.status}); run scripts/fetch-character-bundle.sh.`);
    }
    return response.arrayBuffer();
  };
  const manifest: unknown = JSON.parse(new TextDecoder().decode(await read("manifest.json")));
  const character = await adaptImportedCharacterBundle({ manifest, key: "humanoid.ranger.male", readResource: read });
  const sockets = socketsOf(await readBundleAttachment({ manifest, key: "humanoid.ranger.male", name: "humanoid-sockets", readResource: read }));
  const items = new Map<string, ImportedItemAsset>();
  for (const { key } of LOADOUT_ITEMS["sword-and-shield"]) {
    items.set(key, await adaptImportedItemBundle({ manifest, key, readResource: read }));
  }
  const instantiate = (loadout: Loadout): Posable => {
    const instance = character.createInstance();
    const carried: { dispose(): void }[] = [];
    for (const { key, socket } of loadout ? LOADOUT_ITEMS[loadout] : []) {
      const item = items.get(key);
      const place = sockets.get(socket);
      if (!item || !place) {
        throw new Error(`The Ranger bundle has no ${key} for socket ${socket}.`);
      }
      const held = item.createInstance();
      instance.attach(held, { joint: place.joint, socket: place.matrix });
      carried.push(held);
    }
    // The runtime's clip clock holds a one-shot clip at its end like the animals.
    return {
      model: instance.root,
      pose: (clip, time) => instance.seek(clip, time),
      dispose: () => {
        instance.dispose();
        for (const held of carried) { held.dispose(); }
      },
    };
  };
  const clips = new Map(character.clips.map((clip) => [clip.name, clip.duration]));
  const probe = instantiate(null);
  const measured = measure(probe, rest);
  probe.dispose();
  return { clips, instantiate, ...measured };
}

export class QuaterniusUnits {
  readonly #loader = new GLTFLoader();
  readonly #rest: RestClip;
  readonly #assets = new Map<QuaterniusKind, Asset | "loading" | "failed">();
  readonly #instances = new Map<string, Instance>();
  readonly #loaded: [QuaterniusKind, [string, number][]][] = [];
  #rootsChanged = false;

  constructor(rest: RestClip) {
    this.#rest = rest;
  }

  /** Starts loading `kind`; true once it can be drawn. */
  ready(kind: QuaterniusKind): boolean {
    const state = this.#assets.get(kind);
    if (state === undefined) {
      this.#assets.set(kind, "loading");
      (kind === "Ranger" ? loadRanger(this.#rest(kind)) : loadAnimal(this.#loader, kind, this.#rest(kind))).then(
        (asset) => {
          this.#assets.set(kind, asset);
          this.#loaded.push([kind, [...asset.clips]]);
        },
        (error: unknown) => {
          console.warn(`Unit model ${kind} failed to load; keeping the procedural model.`, error);
          this.#assets.set(kind, "failed");
        },
      );
      return false;
    }
    return typeof state === "object";
  }

  /** Models that finished loading since the last call, with their clip lengths. */
  drainLoaded(): [QuaterniusKind, [string, number][]][] {
    return this.#loaded.splice(0);
  }

  /** Poses every requested unit and drops the instances of units not requested. */
  update(units: readonly ImportedRequest[]): void {
    const seen = new Set<string>();
    for (const unit of units) {
      const asset = this.#assets.get(unit.kind);
      if (typeof asset !== "object") { continue; }
      seen.add(unit.id);
      const loadout: Loadout = unit.swordAndShield ? "sword-and-shield" : null;
      let instance = this.#instances.get(unit.id);
      if (instance && (instance.kind !== unit.kind || instance.loadout !== loadout)) {
        this.#drop(unit.id, instance);
        instance = undefined;
      }
      instance ??= this.#create(unit, loadout, asset);
      this.#pose(instance, asset, unit);
    }
    for (const [id, instance] of this.#instances) {
      if (!seen.has(id)) { this.#drop(id, instance); }
    }
  }

  #create(unit: ImportedRequest, loadout: Loadout, asset: Asset): Instance {
    const posable = asset.instantiate(loadout);
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
      kind: unit.kind, loadout, root, posable,
      clip: "", time: 0, materials, dead: null, head: model.getObjectByName("Head") ?? null, entity: unit.entity,
    };
    this.#instances.set(unit.id, instance);
    this.#rootsChanged = true;
    return instance;
  }

  #pose(instance: Instance, asset: Asset, unit: ImportedRequest): void {
    const { clip } = unit;
    instance.clip = clip;
    instance.time = Math.min(unit.time, Math.max(0, (asset.clips.get(clip) ?? 0) - 1e-4));
    instance.posable.pose(clip, instance.time);
    const dead = unit.tint !== null;
    if (instance.dead !== dead) {
      instance.dead = dead;
      for (const { material, base } of instance.materials) {
        material.color.copy(base).multiplyScalar(unit.tint ?? 1);
      }
    }
    const { root } = instance;
    root.position.set(unit.feet[0], unit.feet[1], unit.feet[2]);
    root.rotation.set(0, unit.yaw, 0);
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
