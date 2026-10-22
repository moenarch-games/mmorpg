import * as THREE from "three";
import { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { MeshoptDecoder } from "three/examples/jsm/libs/meshopt_decoder.module.js";
import type { ThreeSceneRenderer } from "@moritzbrantner/three-d-renderer";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { EntityRef } from "../../entity-ref";
import type { ImportedModelKind, ImportedRequest } from "../unit-presenter";

type QuaterniusKind = ImportedModelKind;
/** What a player model carries: a Warden's sword and shield (#156), a Ranger's bow and quiver (#244), or nothing. */
type Loadout = "sword-and-shield" | "bow-and-quiver" | null;
/**
 * The viewer's headwear (#374): an authored item on the `head.armor` socket, the bundle's own hood,
 * or none. Units without a viewer style keep the bundle's default look.
 */
export type Headwear = "wayfarer-hood" | "ranger-cap" | "ironcrest-helm" | "none";
const HEADWEAR_ITEMS: Readonly<Record<Headwear, readonly { key: string; socket: string }[]>> = {
  "wayfarer-hood": [],
  "ranger-cap": [{ key: "equipment.cap.ranger", socket: "head.armor" }],
  "ironcrest-helm": [{ key: "equipment.helm.ironcrest", socket: "head.armor" }],
  none: [],
};
/** The viewer's hair (#376): a bundle hairstyle (or bald) in one of its tints. */
export type Hair = { style: "buzzed" | "parted" | "long" | "bald"; color: "black" | "brown" | "blond" };
const HAIR_ENTRIES: Readonly<Record<Exclude<Hair["style"], "bald">, string>> = {
  buzzed: "hair.buzzed", parted: "hair.simple-parted", long: "hair.long",
};
/** The tintable texture variant every hairstyle is fetched in. */
const HAIR_VARIANT = "hair-1";
/**
 * How the viewer's Ranger is dressed: carried items, headwear and hair. Units without a viewer
 * look (`headwear` and `hair` null) keep the bundle's default: the hood up, no hair.
 */
type Dress = { loadout: Loadout; headwear: Headwear | null; hair: Hair | null };
const dressKey = (dress: Dress) => JSON.stringify(dress);
/** The rest (idle) clip a model is measured in, from the shared presenter's clip table. */
export type RestClip = (kind: QuaterniusKind) => string;

/**
 * Animated imported units as real three.js skinned meshes drawn through the renderer's
 * imported-roots path, all loaded with plain three.js (asset-tooling Atlas 7.1.1, #244): `GLTFLoader`
 * for the Ranger humanoid bundle's glTF entries (Atlas 6.3.7) and the Quaternius CC0 animal GLBs
 * (Atlas 6.3.6), `SkeletonUtils.clone` per unit, `AnimationMixer` for clips and ordinary parenting
 * under the bundle's `humanoid-sockets` joints for carried equipment. Each model
 * loads on first use and is instantiated per unit; until it has loaded the shared presenter keeps
 * drawing the procedural model. Clip choice and clock come from the shared presenter (#184).
 * Presentation only.
 */
const ANIMAL_URLS: Readonly<Record<Exclude<QuaterniusKind, "Ranger">, string>> = {
  Wolf: new URL("../../../../assets/quaternius/Wolf.glb", import.meta.url).href,
  Rat: new URL("../../../../assets/quaternius/Rat.glb", import.meta.url).href,
  Pig: new URL("../../../../assets/quaternius/Pig.glb", import.meta.url).href,
};
/** The unpacked bundle `scripts/fetch-bundles.sh` places in `web/public`. */
const RANGER_BUNDLE = `${import.meta.env.BASE_URL}characters/humanoid.ranger.male/`;

/** One drawn model of an asset, posed by clip name and clip time. */
type Posable = { model: THREE.Object3D; pose(clip: string, time: number): void; dispose(): void };

type Asset = {
  /** Clip durations in seconds by name. */
  clips: Map<string, number>;
  /** Clip roles and strike contact times by name, from the bundle's `humanoid-clips` (Ranger only). */
  motions?: ReadonlyMap<string, ClipMotion>;
  instantiate(dress: Dress): Posable;
  /** Rest (idle at 0) bounds: lowest point, height and horizontal centre, model space. */
  floor: number;
  height: number;
  centre: [number, number];
};

type Instance = {
  kind: QuaterniusKind;
  dress: string;
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

/** Rest bounds of a model posed at the start of its idle clip. */
function measure(posable: Posable, idle: string): Pick<Asset, "floor" | "height" | "centre"> {
  posable.pose(idle, 0);
  const probe = posable.model;
  probe.updateMatrixWorld(true);
  const box = new THREE.Box3();
  probe.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) { return; }
    const position = object.geometry.getAttribute("position");
    for (let i = 0; i < position.count; i++) {
      object.getVertexPosition(i, point);
      box.expandByPoint(point.applyMatrix4(object.matrixWorld));
    }
  });
  if (box.isEmpty()) {
    return { floor: 0, height: 1, centre: [0, 0] };
  }
  return { floor: box.min.y, height: box.max.y - box.min.y, centre: [(box.min.x + box.max.x) / 2, (box.min.z + box.max.z) / 2] };
}

/** `model` posed by clip name and time through its own `AnimationMixer`. */
function mixerPosable(model: THREE.Object3D, sources: ReadonlyMap<string, THREE.AnimationClip>, dispose?: () => void): Posable {
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
    dispose: () => {
      mixer.stopAllAction();
      mixer.uncacheRoot(model);
      dispose?.();
    },
  };
}

async function loadAnimal(loader: GLTFLoader, kind: Exclude<QuaterniusKind, "Ranger">, rest: string): Promise<Asset> {
  const gltf = await loader.loadAsync(ANIMAL_URLS[kind]);
  const sources = new Map<string, THREE.AnimationClip>();
  for (const clip of gltf.animations) {
    // Blender prefixes clips with their armature ("AnimalArmature|Walk").
    sources.set(clip.name.split("|").pop() ?? clip.name, clip);
  }
  const instantiate = (): Posable => mixerPosable(cloneSkinned(gltf.scene), sources);
  const clips = new Map([...sources].map(([name, clip]) => [name, clip.duration]));
  const probe = instantiate();
  return { clips, instantiate, ...measure(probe, rest) };
}

/** A humanoid-sockets descriptor socket as a matrix in its joint's frame. */
type Socket = { joint: string; matrix: THREE.Matrix4 };

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
    sockets.set(entry.id, { joint: entry.joint, matrix });
  }
  return sockets;
}

/** A clip's `humanoid-clips` motion data: its semantic role and the contact time of a strike. */
export type ClipMotion = { role: string | null; contactSeconds: number | null };

/** Reads the `humanoid-clips-v1` attachment by clip name. */
export function clipMotionsOf(descriptor: unknown): Map<string, ClipMotion> {
  const value = descriptor as { schema?: unknown; clips?: unknown } | null;
  if (value?.schema !== "humanoid-clips-v1" || !Array.isArray(value.clips)) {
    throw new Error("The Ranger bundle's humanoid-clips attachment is not humanoid-clips-v1.");
  }
  return new Map((value.clips as { name: string; role?: string; contact?: { seconds?: number } }[]).map((clip) => [
    clip.name,
    { role: clip.role ?? null, contactSeconds: typeof clip.contact?.seconds === "number" ? clip.contact.seconds : null },
  ]));
}

/** Which bundle item goes to which socket, per loadout. */
const LOADOUT_ITEMS: Readonly<Record<Exclude<Loadout, null>, readonly { key: string; socket: string }[]>> = {
  "sword-and-shield": [
    { key: "equipment.sword.arming", socket: "hand_r.weapon" },
    { key: "equipment.shield.heater", socket: "hand_l.shield" },
  ],
  "bow-and-quiver": [
    { key: "equipment.bow.long", socket: "back.bow.stowed" },
    { key: "equipment.quiver.back", socket: "back.quiver" },
  ],
};

type DownloadEntry = { key: string; variant: string; files: string[]; clips?: string };

/** Select the existing Dark archer default from the rolling download index. */
function bundleEntry(index: unknown, key: string, variant = "default"): DownloadEntry {
  const value = index as { schema?: unknown; entries?: DownloadEntry[] } | null;
  const entry = value?.schema === "asset-downloads-v1" && Array.isArray(value.entries)
    ? value.entries.find((entry) => entry.key === key && entry.variant === variant) : undefined;
  if (!entry) { throw new Error(`The Ranger download index has no ${key}/${variant} entry.`); }
  return entry;
}

function entryFile(entry: DownloadEntry, suffix: string): string {
  const files = entry.files.filter((file) => file.endsWith(suffix));
  if (files.length !== 1) { throw new Error(`The Ranger entry ${entry.key} must declare one ${suffix} file.`); }
  return files[0]!;
}

/** An item placed by its `grip` frame, as 3d-lab's attach did: holder = socket × grip⁻¹. */
function attachItem(model: THREE.Object3D, item: THREE.Object3D, gripInverse: THREE.Matrix4, socket: Socket): void {
  const joint = model.getObjectByName(socket.joint);
  if (!joint) {
    throw new Error(`The Ranger has no joint ${socket.joint}.`);
  }
  const holder = new THREE.Group();
  holder.matrix.multiplyMatrices(socket.matrix, gripInverse);
  holder.matrixAutoUpdate = false;
  holder.add(item);
  joint.add(holder);
}

/** What the bundle's `humanoid-appearance` says about the head: hood, hair tints and what headwear hides. */
type HeadLook = {
  /** The mesh nodes of the toggleable hood (part `outfit.hood`). */
  hood: string[];
  /** Tint base colours by colour id (`tint.<color>`). */
  tints: Map<string, [number, number, number]>;
  /** Materials a hairstyle's tint applies to, by hair entry. */
  hairMaterials: Map<string, string[]>;
  /** The eyebrow nodes and materials that follow the hair tint. */
  eyebrows: { nodes: string[]; materials: string[] };
  /** What each headwear entry hides (`outfit.hood`, `hair`). */
  hides: Map<string, string[]>;
};

function headLook(appearance: unknown): HeadLook {
  type Named = { id?: unknown; entry?: unknown; nodes?: unknown; tintMaterials?: unknown; hides?: unknown; baseColorFactor?: unknown };
  const value = appearance as { parts?: Named[]; tints?: Named[]; hair?: Named[]; headwear?: Named[]; eyebrows?: Named & { followsHairTint?: unknown } };
  const strings = (list: unknown): string[] => (Array.isArray(list) && list.every((item) => typeof item === "string") ? list : []);
  const hood = value.parts?.find((part) => part.id === "outfit.hood");
  if (!hood || strings(hood.nodes).length === 0) {
    throw new Error("The Ranger's humanoid-appearance has no outfit.hood part.");
  }
  const tints = new Map((value.tints ?? []).flatMap((tint) => {
    const c = tint.baseColorFactor;
    return typeof tint.id === "string" && Array.isArray(c) ? [[tint.id, [Number(c[0]), Number(c[1]), Number(c[2])] as [number, number, number]] as const] : [];
  }));
  const hairMaterials = new Map((value.hair ?? []).flatMap((hair) => (typeof hair.entry === "string" ? [[hair.entry, strings(hair.tintMaterials)] as const] : [])));
  const eyebrows = value.eyebrows?.followsHairTint === true
    ? { nodes: strings(value.eyebrows.nodes), materials: strings(value.eyebrows.tintMaterials) } : { nodes: [], materials: [] };
  const hides = new Map((value.headwear ?? []).flatMap((item) => (typeof item.entry === "string" ? [[item.entry, strings(item.hides)] as const] : [])));
  return { hood: strings(hood.nodes), tints, hairMaterials, eyebrows, hides };
}

/** Gives `object`'s meshes their own copies of the `names` materials, coloured `color` (linear). */
function tintMaterials(object: THREE.Object3D, names: readonly string[], color: readonly [number, number, number]): void {
  object.traverse((child) => {
    if (!(child instanceof THREE.Mesh)) { return; }
    const tint = (material: THREE.Material): THREE.Material => {
      if (!names.includes(material.name) || !(material instanceof THREE.MeshStandardMaterial)) { return material; }
      const copy = material.clone();
      copy.color.setRGB(color[0], color[1], color[2], THREE.LinearSRGBColorSpace);
      return copy;
    };
    child.material = Array.isArray(child.material) ? child.material.map(tint) : tint(child.material);
  });
}

async function loadRanger(loader: GLTFLoader, rest: string): Promise<Asset> {
  const url = (path: string) => new URL(path, new URL(RANGER_BUNDLE, window.location.href)).href;
  const json = async (path: string): Promise<unknown> => {
    const response = await fetch(url(path));
    if (!response.ok) {
      throw new Error(`Ranger bundle file ${path} is missing (${response.status}); run scripts/fetch-bundles.sh.`);
    }
    return response.json();
  };
  const manifest = await json("humanoid-ranger-male.index.json");
  const entry = bundleEntry(manifest, "humanoid.ranger.male");
  if (!entry.clips) { throw new Error("The Ranger download index has no shared clips."); }
  const [gltf, animationLibrary, sockets, motions] = await Promise.all([
    loader.loadAsync(url(entryFile(entry, ".gltf"))),
    loader.loadAsync(url(entry.clips)),
    json(entryFile(entry, ".humanoid-sockets.json")).then(socketsOf),
    json(entryFile(entry, ".humanoid-clips.json")).then(clipMotionsOf),
  ]);
  const keys = [...new Set([...Object.values(LOADOUT_ITEMS), ...Object.values(HEADWEAR_ITEMS)].flat().map((item) => item.key))];
  const look = headLook(await json(entryFile(entry, ".humanoid-appearance.json")));
  const load = async (key: string, variant?: string) => {
    const scene = (await loader.loadAsync(url(entryFile(bundleEntry(manifest, key, variant), ".gltf")))).scene;
    if (key === "hair.long") {
      // The long locks are authored lower than the buzzed scalp in the same asset.
      // Lift only the locks so the scalp stays seated on the head.hair socket.
      const locks = scene.getObjectByName("Hair_Long");
      if (!locks) { throw new Error("The Ranger's long-hair asset has no Hair_Long node."); }
      locks.position.y += 0.1;
    }
    scene.updateMatrixWorld(true);
    // Hairstyles are authored in rest model space with no grip: their origin is the socket's.
    const grip = scene.getObjectByName("grip");
    if (!grip && variant === undefined) { throw new Error(`The Ranger bundle's ${key} has no grip frame.`); }
    return [key, { scene, gripInverse: grip ? grip.matrixWorld.clone().invert() : new THREE.Matrix4() }] as const;
  };
  const items = new Map(await Promise.all([
    ...keys.map((key) => load(key)),
    ...Object.values(HAIR_ENTRIES).map((key) => load(key, HAIR_VARIANT)),
  ]));
  const sources = new Map(animationLibrary.animations.map((clip) => [clip.name, clip]));
  const instantiate = ({ loadout, headwear, hair }: Dress): Posable => {
    const model = cloneSkinned(gltf.scene);
    // The bundle's hood is on by default; any other chosen headwear replaces it.
    const hooded = headwear === null || headwear === "wayfarer-hood";
    for (const name of look.hood) {
      const node = model.getObjectByName(name);
      if (!node) { throw new Error(`The Ranger has no hood node ${name}.`); }
      node.visible = hooded;
    }
    const worn = [...(loadout ? LOADOUT_ITEMS[loadout] : []), ...(headwear ? HEADWEAR_ITEMS[headwear] : [])];
    const hairHidden = hooded || worn.some(({ key }) => look.hides.get(key)?.includes("hair"));
    const hairKey = hair && hair.style !== "bald" && !hairHidden ? HAIR_ENTRIES[hair.style] : null;
    const tint = hair ? look.tints.get(`tint.${hair.color}`) : undefined;
    for (const { key, socket } of [...worn, ...(hairKey ? [{ key: hairKey, socket: "head.hair" }] : [])]) {
      const item = items.get(key);
      const place = sockets.get(socket);
      if (!item || !place) {
        throw new Error(`The Ranger bundle has no ${key} for socket ${socket}.`);
      }
      // The bow is skinned to its own grip/nock joints; SkeletonUtils clones those too.
      const clone = cloneSkinned(item.scene);
      if (key === hairKey && tint) { tintMaterials(clone, look.hairMaterials.get(key) ?? [], tint); }
      attachItem(model, clone, item.gripInverse, place);
    }
    if (tint) {
      for (const name of look.eyebrows.nodes) {
        const node = model.getObjectByName(name);
        if (node) { tintMaterials(node, look.eyebrows.materials, tint); }
      }
    }
    return mixerPosable(model, sources);
  };
  const clips = new Map([...sources].map(([name, clip]) => [name, clip.duration]));
  const probe = instantiate({ loadout: null, headwear: null, hair: null });
  const measured = measure(probe, rest);
  probe.dispose();
  return { clips, motions, instantiate, ...measured };
}

export class QuaterniusUnits {
  readonly #loader = new GLTFLoader().setMeshoptDecoder(MeshoptDecoder);
  readonly #textures = new KTX2Loader().setTranscoderPath(`${import.meta.env.BASE_URL}basis/`);
  readonly #rest: RestClip;
  readonly #assets = new Map<QuaterniusKind, Asset | "loading" | "failed">();
  readonly #settled = new Map<QuaterniusKind, Promise<boolean>>();
  readonly #instances = new Map<string, Instance>();
  readonly #loaded: [QuaterniusKind, [string, number][]][] = [];
  #rootsChanged = false;

  constructor(rest: RestClip) {
    this.#rest = rest;
  }

  configureRenderer(renderer: ThreeSceneRenderer): void {
    renderer.configureTextureLoader(this.#textures);
    this.#loader.setKTX2Loader(this.#textures);
  }

  /** Starts loading `kind`; true once it can be drawn. */
  ready(kind: QuaterniusKind): boolean {
    const state = this.#assets.get(kind);
    if (state === undefined) {
      this.#assets.set(kind, "loading");
      this.#settled.set(kind, (kind === "Ranger" ? loadRanger(this.#loader, this.#rest(kind)) : loadAnimal(this.#loader, kind, this.#rest(kind))).then(
        (asset) => {
          this.#assets.set(kind, asset);
          this.#loaded.push([kind, [...asset.clips]]);
          return true;
        },
        (error: unknown) => {
          console.warn(`Unit model ${kind} failed to load; keeping the procedural model.`, error);
          this.#assets.set(kind, "failed");
          return false;
        },
      ));
      return false;
    }
    return typeof state === "object";
  }

  /** Starts loading `kind` and settles once it is drawable (true) or failed (false). */
  preload(kind: QuaterniusKind): Promise<boolean> {
    this.ready(kind);
    return this.#settled.get(kind) ?? Promise.resolve(true);
  }

  /** Models that finished loading since the last call, with their clip lengths. */
  drainLoaded(): [QuaterniusKind, [string, number][]][] {
    return this.#loaded.splice(0);
  }

  /** Poses every requested unit and drops the instances of units not requested. */
  update(units: readonly ImportedRequest[], viewer: { headwear: Headwear; hair: Hair }): void {
    const seen = new Set<string>();
    for (const unit of units) {
      const asset = this.#assets.get(unit.kind);
      if (typeof asset !== "object") { continue; }
      seen.add(unit.id);
      const loadout: Loadout = unit.swordAndShield ? "sword-and-shield" : unit.bowAndQuiver ? "bow-and-quiver" : null;
      let instance = this.#instances.get(unit.id);
      const dressed = unit.headwear && unit.kind === "Ranger";
      const dress: Dress = { loadout, headwear: dressed ? viewer.headwear : null, hair: dressed ? viewer.hair : null };
      if (instance && (instance.kind !== unit.kind || instance.dress !== dressKey(dress))) {
        this.#drop(unit.id, instance);
        instance = undefined;
      }
      instance ??= this.#create(unit, dress, asset);
      this.#pose(instance, asset, unit);
    }
    for (const [id, instance] of this.#instances) {
      if (!seen.has(id)) { this.#drop(id, instance); }
    }
  }

  #create(unit: ImportedRequest, dress: Dress, asset: Asset): Instance {
    const posable = asset.instantiate(dress);
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
      kind: unit.kind, dress: dressKey(dress), root, posable,
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

  /** The length in seconds of a loaded model's clip, or null before it loaded. */
  clipLength(kind: QuaterniusKind, clip: string): number | null {
    const asset = this.#assets.get(kind);
    return typeof asset === "object" ? asset.clips.get(clip) ?? null : null;
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
  pose(id: string): { clip: string; role: string | null; time: number; dead: boolean; root: number[]; skeleton: number[]; head: number[] | null } | null {
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
      clip: instance.clip, role: this.#motion(instance)?.role ?? null, time: round(instance.time), dead: instance.dead === true,
      root: [position.x, position.y, position.z, rotation.y, scale.x].map(round),
      skeleton: extent.isEmpty() ? [] : [...extent.min.toArray(), ...extent.max.toArray()].map(round), head,
    };
  }

  #motion(instance: Instance): ClipMotion | undefined {
    const asset = this.#assets.get(instance.kind);
    return typeof asset === "object" ? asset.motions?.get(instance.clip) : undefined;
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
