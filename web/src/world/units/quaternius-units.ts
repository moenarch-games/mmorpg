import * as THREE from "three";
import { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { MeshoptDecoder } from "three/examples/jsm/libs/meshopt_decoder.module.js";
import type { ThreeSceneRenderer } from "@moritzbrantner/three-d-renderer";
import { GLTFLoader, type GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { EntityRef } from "../../entity-ref";
import { characterVisualProfile } from "../../character-visuals";
import type { CharacterClassId } from "../../character-selection";
import { figureAppearance, varyPreset } from "../wasm-runtime";
import type { ClassOutfit, ClipLayer, ImportedModelKind, ImportedRequest, LayerMask, Loadout as CarriedLoadout } from "../unit-presenter";

type QuaterniusKind = ImportedModelKind;
/**
 * What a model carries: a Warden's sword and shield (#156), a Ranger's bow and quiver (#244), the
 * Rogue's dagger, a caster's staff (asset-tooling #375), the Arcanist's staff and tome or the
 * Cleric's mace (#379), a guard's sword, or nothing.
 */
type Loadout = CarriedLoadout | null;
/**
 * The viewer's headwear (#374): the class outfit's own (#379), an authored item on the
 * `head.armor` socket, the bundle's own hood, or none. Units without a viewer style keep the
 * bundle's default look.
 */
export type Headwear = "class" | "wayfarer-hood" | "ranger-cap" | "ironcrest-helm" | "none";
const HEADWEAR_ITEMS: Readonly<Record<Headwear, readonly { key: string; socket: string }[]>> = {
  class: [],
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
type Dress = {
  loadout: Loadout; headwear: Headwear | null; hair: Hair | null; face: FaceChoice | null; resident: string | null;
  /** The Ranger figure a resident is drawn on (`imported::Figure`, #493). */
  figure: string | null;
  /** A varied resident's look seed (asset-tooling #380), once that look has loaded. */
  variety: number | null;
  /** A player's class starter outfit (#379), once its preset has loaded. */
  outfit: ClassOutfit | null;
};
/** The viewer's face (#411): a `humanoid-appearance.face` feature id to one of its option ids. */
export type FaceChoice = Readonly<Record<string, string>>;
/** One face feature the bundle offers (#411), with its options' morph-target weights. */
export type FaceFeature = {
  id: string; label: string; default: string;
  options: readonly { id: string; label: string; morphs: Readonly<Record<string, number>> }[];
};
/** The face morphs of the loaded Ranger entry: the nodes carrying them and the features. */
type FaceLook = { nodes: readonly string[]; features: readonly FaceFeature[] };
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
/**
 * The Ranger figures (`imported::Figure`): the male carries the players, the class outfits and the
 * male residents and enemies, the female Tanner Hilda Brook (#493). `scripts/fetch-bundles.sh`
 * places each figure's bundle in `web/public/characters/<character>/`.
 */
const MALE = "humanoid.ranger.male";
const FIGURES = [MALE, "humanoid.ranger.female"] as const;

/** One drawn model of an asset, posed by weighted, masked clip layers (Atlas 6.6.45). */
type Posable = {
  model: THREE.Object3D; pose(layers: readonly ClipLayer[]): void; dispose(): void;
  /** A varied resident's height factor on the collision-height scale (asset-tooling #380). */
  scale?: number;
  /** The rest bounds of a look on another figure than the asset's (#493); the asset's otherwise. */
  bounds?: Pick<Asset, "floor" | "height" | "centre">;
};

type Asset = {
  /** Clip durations in seconds by name. */
  clips: Map<string, number>;
  /** Clip roles and strike contact times by name, from the bundle's `humanoid-clips` (Ranger only). */
  motions?: ReadonlyMap<string, ClipMotion>;
  instantiate(dress: Dress): Posable;
  /** Loads the residents' appearance presets (#418), Ranger only; resolves once they can be drawn. */
  loadResidents?: () => Promise<void>;
  /**
   * Whether a class starter outfit preset (#379) can be drawn, Ranger only; the first ask starts
   * loading it, and a failed preset stays false (the unit keeps the bundle's default look).
   */
  outfitReady?: (preset: string) => boolean;
  /**
   * Whether a varied resident's own look (`preset` varied by `seed`, asset-tooling #380) can be
   * drawn, Ranger only; the first ask starts building it, and until then (or after it failed) the
   * resident wears the plain preset.
   */
  variedReady?: (preset: string, seed: number, figure: string) => boolean;
  /** The face features the drawn entry supports (#411); null when it carries no face morphs. */
  face?: FaceLook | null;
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
  /** The layers posed this frame. */
  layers: readonly ClipLayer[];
  materials: { material: THREE.MeshStandardMaterial; base: THREE.Color }[];
  /** The colour factor applied to `materials`; undefined until the first pose. */
  tint: readonly [number, number, number] | null | undefined;
  head: THREE.Object3D | null;
  entity: EntityRef;
};

const point = new THREE.Vector3();

/** Rest bounds of a model posed at the start of its idle clip. */
function measure(posable: Posable, idle: string): Pick<Asset, "floor" | "height" | "centre"> {
  posable.pose([{ clip: idle, time: 0, weight: 1, mask: "all" }]);
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

/** The shared presenter's upper-body root (`imported::UPPER_BODY_ROOT`): spine, arms and head move under it. */
const UPPER_BODY_ROOT = "spine_01";

/** The node names of `model`'s upper body, empty for a model without one. */
function upperBody(model: THREE.Object3D): Set<string> {
  const names = new Set<string>();
  model.getObjectByName(UPPER_BODY_ROOT)?.traverse((object) => { names.add(object.name); });
  return names;
}

/** Per source clip, its masked copies by mask and slot, so each layer drives its own action. */
const variants = new WeakMap<THREE.AnimationClip, Map<string, THREE.AnimationClip>>();

/**
 * `clip` keeping only the tracks `mask` drives. A clip in the same mask twice (a restarted strike
 * crossfading from its previous swing) takes a copy per `slot`, since a mixer plays one action per clip.
 */
function variant(clip: THREE.AnimationClip, mask: LayerMask, slot: number, upper: ReadonlySet<string>): THREE.AnimationClip {
  if (mask === "all" && slot === 0) { return clip; }
  let byKey = variants.get(clip);
  if (!byKey) { variants.set(clip, byKey = new Map()); }
  const key = `${mask}:${slot}:${upper.size}`;
  let masked = byKey.get(key);
  if (!masked) {
    const tracks = mask === "all" ? clip.tracks : clip.tracks.filter((track) =>
      upper.has(THREE.PropertyBinding.parseTrackName(track.name).nodeName ?? "") === (mask === "upper"));
    masked = new THREE.AnimationClip(`${clip.name}|${key}`, clip.duration, tracks);
    byKey.set(key, masked);
  }
  return masked;
}

/**
 * `model` posed by the shared presenter's clip layers through its own `AnimationMixer`: one action
 * per layer, its tracks masked to the layer's region and weighted, so a joint averages the layers
 * driving it and rests for the remainder, like the native client's CPU skinning.
 */
function mixerPosable(model: THREE.Object3D, sources: ReadonlyMap<string, THREE.AnimationClip>, dispose?: () => void): Posable {
  const mixer = new THREE.AnimationMixer(model);
  const upper = upperBody(model);
  const playing = new Map<string, THREE.AnimationAction>();
  return {
    model,
    pose(layers) {
      const used = new Set<string>();
      for (const layer of layers) {
        const source = sources.get(layer.clip);
        if (!source) { continue; }
        let slot = 0;
        while (used.has(`${layer.clip}|${layer.mask}|${slot}`)) { slot += 1; }
        const key = `${layer.clip}|${layer.mask}|${slot}`;
        used.add(key);
        let action = playing.get(key);
        if (!action) {
          action = mixer.clipAction(variant(source, layer.mask, slot, upper)).play();
          playing.set(key, action);
        }
        action.setEffectiveWeight(layer.weight);
        // Hold a one-shot clip on its last frame rather than letting the loop wrap it to 0.
        action.time = Math.min(layer.time, Math.max(0, source.duration - 1e-4));
      }
      for (const [key, action] of playing) {
        if (!used.has(key)) {
          action.stop();
          playing.delete(key);
        }
      }
      mixer.update(0);
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
type Socket = { id: string; joint: string; matrix: THREE.Matrix4 };

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
    sockets.set(entry.id, { id: entry.id, joint: entry.joint, matrix });
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
const LOADOUT_ITEMS: Readonly<Record<CarriedLoadout, readonly { key: string; socket: string }[]>> = {
  "sword-and-shield": [
    { key: "equipment.sword.arming", socket: "hand_r.weapon" },
    { key: "equipment.shield.heater", socket: "hand_l.shield" },
  ],
  "bow-and-quiver": [
    { key: "equipment.bow.long", socket: "back.bow.stowed" },
    { key: "equipment.quiver.back", socket: "back.quiver" },
  ],
  sword: [{ key: "equipment.sword.arming", socket: "hand_r.weapon" }],
  dagger: [{ key: "equipment.dagger.blackened", socket: "hand_r.weapon" }],
  "staff.bone": [{ key: "equipment.staff.bone", socket: "hand_r.staff" }],
  // The tome is held like a shield (asset-tooling #379): spine along the forearm.
  "staff.crystal-and-tome": [
    { key: "equipment.staff.crystal", socket: "hand_r.staff" },
    { key: "equipment.book.tome", socket: "hand_l.shield" },
  ],
  "mace.flanged": [{ key: "equipment.mace.flanged", socket: "hand_r.weapon" }],
  "staff.gnarled": [{ key: "equipment.staff.gnarled", socket: "hand_r.staff" }],
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
  if (HANGING_SOCKETS.has(socket.id)) {
    let held = hanging.get(model);
    if (!held) { hanging.set(model, held = []); }
    held.push({ holder, joint, base: holder.matrix.clone() });
  }
}

/**
 * The sockets whose items hang loose and swing (Atlas 6.6.39): the shared presenter's
 * `sway::hangs`. Held items follow the hands and worn plates the body.
 */
const HANGING_SOCKETS: ReadonlySet<string> = new Set(["back.quiver", "back.bow.stowed", "back.stowed", "hip_l.stowed"]);
/** Each model's hanging items: the holder, its joint and its authored socket matrix. */
const hanging = new WeakMap<THREE.Object3D, { holder: THREE.Group; joint: THREE.Object3D; base: THREE.Matrix4 }[]>();
const modelInverse = new THREE.Matrix4();
const jointInModel = new THREE.Matrix4();
const swungAt = new THREE.Vector3();
const swungTurn = new THREE.Quaternion();
const swungScale = new THREE.Vector3();
const swingTurn = new THREE.Quaternion();

/**
 * Turns `model`'s hanging items about their grip by the shared presenter's `swing`, a quaternion
 * in the model's frame, or puts them back as authored for null. Call after posing and placing.
 */
function swingHanging(model: THREE.Object3D, swing: readonly [number, number, number, number] | null): void {
  const held = hanging.get(model);
  if (!held) { return; }
  if (!swing) {
    for (const { holder, base } of held) {
      if (!holder.matrix.equals(base)) { holder.matrix.copy(base); }
    }
    return;
  }
  swingTurn.set(swing[0], swing[1], swing[2], swing[3]);
  model.updateWorldMatrix(true, false);
  modelInverse.copy(model.matrixWorld).invert();
  for (const { holder, joint, base } of held) {
    joint.updateWorldMatrix(true, false);
    // The socket in model space, turned about its own origin in the model's axes.
    jointInModel.multiplyMatrices(modelInverse, joint.matrixWorld);
    holder.matrix.multiplyMatrices(jointInModel, base).decompose(swungAt, swungTurn, swungScale);
    swungTurn.premultiply(swingTurn);
    holder.matrix.compose(swungAt, swungTurn, swungScale).premultiply(jointInModel.invert());
  }
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

/** The meshes under `nodes` of `model` that carry morph targets. */
function morphMeshes(model: THREE.Object3D, nodes: readonly string[]): THREE.Mesh[] {
  const meshes: THREE.Mesh[] = [];
  for (const name of nodes) {
    model.getObjectByName(name)?.traverse((child) => {
      if (child instanceof THREE.Mesh && child.morphTargetInfluences && child.morphTargetDictionary) { meshes.push(child); }
    });
  }
  return meshes;
}

/**
 * The `humanoid-appearance.face` section (#411) for the drawn `variant`, or null when the bundle has
 * none, lists the variant as unsupported (its levels of detail carry no targets) or the model lacks
 * a target an option names: the character then keeps the base face and the editor says so.
 */
export function faceLookOf(appearance: unknown, variant: string, model: THREE.Object3D): FaceLook | null {
  const face = (appearance as { face?: unknown }).face as
    { nodes?: unknown; features?: unknown; variants?: unknown } | undefined;
  if (!face || !Array.isArray(face.nodes) || !Array.isArray(face.features) || !Array.isArray(face.variants)) { return null; }
  if (!face.variants.includes(variant)) { return null; }
  const nodes = face.nodes.filter((node): node is string => typeof node === "string");
  const targets = new Set(morphMeshes(model, nodes).flatMap((mesh) => Object.keys(mesh.morphTargetDictionary ?? {})));
  const features: FaceFeature[] = [];
  for (const raw of face.features as { id?: unknown; label?: unknown; default?: unknown; options?: unknown }[]) {
    if (typeof raw.id !== "string" || typeof raw.default !== "string" || !Array.isArray(raw.options)) { return null; }
    const options = (raw.options as { id?: unknown; label?: unknown; morphs?: unknown }[]).map((option) => ({
      id: String(option.id), label: String(option.label ?? option.id),
      morphs: Object.fromEntries(Object.entries((option.morphs ?? {}) as Record<string, unknown>).map(([target, weight]) => [target, Number(weight)])),
    }));
    if (!options.some((option) => option.id === raw.default)
      || options.some((option) => Object.keys(option.morphs).some((target) => !targets.has(target)))) { return null; }
    features.push({ id: raw.id, label: typeof raw.label === "string" ? raw.label : raw.id, default: raw.default, options });
  }
  return features.length > 0 ? { nodes, features } : null;
}

/** Resets `model`'s face morphs and applies the chosen option (or the default) of every feature. */
export function applyFace(model: THREE.Object3D, look: FaceLook, choice: FaceChoice): void {
  const meshes = morphMeshes(model, look.nodes);
  for (const mesh of meshes) { mesh.morphTargetInfluences!.fill(0); }
  for (const feature of look.features) {
    const option = feature.options.find((candidate) => candidate.id === choice[feature.id])
      ?? feature.options.find((candidate) => candidate.id === feature.default);
    for (const [target, weight] of Object.entries(option?.morphs ?? {})) {
      for (const mesh of meshes) {
        const index = mesh.morphTargetDictionary![target];
        if (index !== undefined) { mesh.morphTargetInfluences![index] = weight; }
      }
    }
  }
}

/**
 * One Ranger figure's bundle (`imported::Figure`, #493): its default entry, shared clips, sockets,
 * `humanoid-appearance` (with the game's presets, `figureAppearance`), the worn items loaded once,
 * and its rest bounds. Every figure has its own skeleton, so its looks pose with its own clips.
 */
type FigureBundle = {
  character: string;
  gltf: GLTF;
  sources: ReadonlyMap<string, THREE.AnimationClip>;
  sockets: ReadonlyMap<string, Socket>;
  motions: ReadonlyMap<string, ClipMotion>;
  appearance: unknown;
  look: HeadLook;
  /** The face features of the default entry (#411); null when it carries no face morphs. */
  face: FaceLook | null;
  /** A bundle entry's scene, loaded once: the residents', the class outfits' and the drops'. */
  scene(key: string, variant: string): Promise<THREE.Object3D>;
  /** Dresses `model`: socket items, then the hair tinted with its eyebrows. */
  wear(model: THREE.Object3D, worn: readonly { key: string; socket: string }[], hairKey: string | null,
    tint: readonly [number, number, number] | undefined): void;
  /** Rest bounds of the default entry posed in `rest`. */
  bounds: Pick<Asset, "floor" | "height" | "centre">;
};

/**
 * Loads the figure `character` from `characters/<character>/`: its default entry and clips, the
 * `keys` items (socketed by their grip) and every hairstyle in the tintable variant, `hair`
 * among them.
 */
async function loadFigure(loader: GLTFLoader, character: string, keys: readonly string[], hair: readonly string[],
  rest: string): Promise<FigureBundle> {
  const directory = `${import.meta.env.BASE_URL}characters/${character}/`;
  const url = (path: string) => new URL(path, new URL(directory, window.location.href)).href;
  const json = async (path: string): Promise<unknown> => {
    const response = await fetch(url(path));
    if (!response.ok) {
      throw new Error(`Ranger bundle file ${path} is missing (${response.status}); run scripts/fetch-bundles.sh.`);
    }
    return response.json();
  };
  const manifest = await json(`${character.replaceAll(".", "-")}.index.json`);
  const entry = bundleEntry(manifest, character);
  if (!entry.clips) { throw new Error(`The ${character} download index has no shared clips.`); }
  const [gltf, animationLibrary, sockets, motions] = await Promise.all([
    loader.loadAsync(url(entryFile(entry, ".gltf"))),
    loader.loadAsync(url(entry.clips)),
    json(entryFile(entry, ".humanoid-sockets.json")).then(socketsOf),
    json(entryFile(entry, ".humanoid-clips.json")).then(clipMotionsOf),
  ]);
  const appearance = figureAppearance(character, await json(entryFile(entry, ".humanoid-appearance.json")));
  const look = headLook(appearance);
  const face = faceLookOf(appearance, entry.variant, gltf.scene);
  if (!face) { console.info(`The ${character} bundle has no face morphs for its drawn entry; faces keep the base shape.`); }
  const load = async (key: string, variant?: string) => {
    const scene = (await loader.loadAsync(url(entryFile(bundleEntry(manifest, key, variant), ".gltf")))).scene;
    if (character === MALE && key === "hair.long") {
      // The male long locks are authored lower than the buzzed scalp in the same asset.
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
  const scenes = new Map<string, Promise<THREE.Object3D>>();
  const scene = (key: string, variant: string): Promise<THREE.Object3D> => {
    const id = `${key}/${variant}`;
    let loaded = scenes.get(id);
    if (!loaded) {
      loaded = loader.loadAsync(url(entryFile(bundleEntry(manifest, key, variant), ".gltf"))).then((result) => {
        result.scene.updateMatrixWorld(true);
        return result.scene;
      });
      scenes.set(id, loaded);
    }
    return loaded;
  };
  // Every hairstyle the bundle offers in the tintable variant: the viewer's and the varied residents'.
  const hairKeys = [...new Set([...hair, ...tintableHair(appearance)])];
  const items = new Map(await Promise.all([
    ...keys.map((key) => load(key)),
    ...hairKeys.map((key) => load(key, HAIR_VARIANT)),
  ]));
  const sources = new Map(animationLibrary.animations.map((clip) => [clip.name, clip]));
  const wear: FigureBundle["wear"] = (model, worn, hairKey, tint) => {
    for (const { key, socket } of [...worn, ...(hairKey ? [{ key: hairKey, socket: "head.hair" }] : [])]) {
      const item = items.get(key);
      const place = sockets.get(socket);
      if (!item || !place) {
        throw new Error(`The ${character} bundle has no ${key} for socket ${socket}.`);
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
  };
  const probe = mixerPosable(cloneSkinned(gltf.scene), sources);
  const bounds = measure(probe, rest);
  probe.dispose();
  return { character, gltf, sources, sockets, motions, appearance, look, face, scene, wear, bounds };
}

async function loadRanger(loader: GLTFLoader, rest: string): Promise<Asset> {
  // The farmer's straw hat (#418) rides with the viewer's headwear choices.
  const keys = [...new Set([...[...Object.values(LOADOUT_ITEMS), ...Object.values(HEADWEAR_ITEMS)].flat().map((item) => item.key), "equipment.hat.straw"])];
  const male = await loadFigure(loader, MALE, keys, Object.values(HAIR_ENTRIES), rest);
  const { gltf, sources, sockets, motions, appearance, look, face, scene, wear } = male;
  /** The other figures, loaded with the residents (#493). */
  const figures = new Map<string, FigureBundle>([[MALE, male]]);
  let residents: Map<string, ResidentLook> | null = null;
  /** Loaded class outfit presets (#379): a look, or null while loading or after failing. */
  const outfits = new Map<string, ResidentLook | null>();
  /** The drop armour's pieces by socket: what an equipped drop wears (`DROP_ARMOR_OUTFIT`). */
  let drops: Map<string, THREE.Object3D> | null = null;
  /** Puts an outfit's or a drop's armour plate on its socket. */
  const plate = (model: THREE.Object3D, node: THREE.Object3D, socket: string) => {
    const place = sockets.get(socket);
    if (!place) { throw new Error(`The Ranger bundle has no socket ${socket}.`); }
    // Plates and beards are authored in rest model space: their origin is the socket's.
    attachItem(model, node.clone(), new THREE.Matrix4(), place);
  };
  /**
   * A player in their class starter outfit (#379). The viewer (a chosen `headwear`) keeps their own
   * face and hair; everyone else wears the preset's face, hair and beards. The chosen headwear
   * decides the head: the outfit's own hood and head plate (`class`), the outfit's hood or else the
   * bundle's (`wayfarer-hood`), an item, or nothing. Equipped drops replace the outfit's plates on
   * the sockets they cover; the class colour stays on the `classTrim` garments.
   */
  const dressOutfit = (outfit: ClassOutfit, { loadout, headwear, hair, face: chosen }: Dress): THREE.Object3D => {
    const preset = outfits.get(outfit.preset);
    if (!preset) { throw new Error(`The Ranger bundle's class outfit ${outfit.preset} has not loaded.`); }
    const viewer = headwear !== null;
    const head = headwear ?? "class";
    const model = cloneSkinned(preset.character);
    if (preset.face) { applyFace(model, preset.face, viewer && chosen ? chosen : preset.choice); }
    const ownHood = (head === "class" || head === "wayfarer-hood") && preset.hood.length > 0;
    // `outfit.hood` names every variant's hood: the bundle's and the class outfits' own. An outfit
    // variant without the bundle's hood shows the hair under `wayfarer-hood` instead.
    const baseHoods = look.hood.filter((hood) => !preset.hood.includes(hood))
      .flatMap((name) => model.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(name)) ?? []);
    const baseHood = head === "wayfarer-hood" && preset.hood.length === 0 && baseHoods.length > 0;
    for (const node of baseHoods) { node.visible = baseHood; }
    for (const name of preset.hood) {
      const node = model.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(name));
      if (node) { node.visible = ownHood; }
    }
    const items = HEADWEAR_ITEMS[head];
    for (const piece of preset.pieces) {
      if (piece.beard) {
        if (viewer) { continue; }
        const clone = piece.node.clone();
        if (piece.tint) { tintMaterials(clone, piece.materials, piece.tint); }
        attachItem(model, clone, new THREE.Matrix4(), sockets.get(piece.socket)!);
        continue;
      }
      if (outfit.replaced.includes(piece.socket) || (piece.socket === "head.armor" && head !== "class")) { continue; }
      plate(model, piece.node, piece.socket);
    }
    for (const socket of outfit.replaced) {
      // A chosen headwear item takes the head over a drop's helm.
      if (socket === "head.armor" && items.length > 0) { continue; }
      const node = drops?.get(socket);
      if (node) { plate(model, node, socket); }
    }
    const worn = [...(loadout ? LOADOUT_ITEMS[loadout] : []), ...items];
    const hooded = ownHood || baseHood;
    if (viewer) {
      const hairHidden = hooded || worn.some(({ key }) => look.hides.get(key)?.includes("hair"));
      const hairKey = hair && hair.style !== "bald" && !hairHidden ? HAIR_ENTRIES[hair.style] : null;
      wear(model, worn, hairKey, hair ? look.tints.get(`tint.${hair.color}`) : undefined);
    } else {
      wear(model, worn, hooded ? null : preset.hair, preset.tint);
    }
    if (preset.trim) { tintMaterials(model, preset.trim.materials, preset.trim.color); }
    return model;
  };
  const outfitReady = (preset: string): boolean => {
    if (outfits.has(preset)) { return outfits.get(preset) !== null; }
    outfits.set(preset, null);
    void Promise.all([
      residentLooks(appearance, look, scene, (id) => id === preset),
      drops ? Promise.resolve(drops) : dropPieces(appearance, scene),
    ]).then(([looks, pieces]) => {
      drops = pieces;
      const loaded = looks.get(preset);
      if (!loaded) { throw new Error(`The Ranger bundle has no class outfit preset ${preset}.`); }
      outfits.set(preset, loaded);
    }).catch((error: unknown) => {
      console.warn(`The class outfit ${preset} failed to load; keeping the default Ranger look.`, error);
    });
    return false;
  };
  const instantiate = (dress: Dress): Posable => {
    const { loadout, headwear, hair, face: chosen, resident } = dress;
    if (dress.outfit !== null) { return mixerPosable(dressOutfit(dress.outfit, dress), sources); }
    if (resident !== null) {
      const figure = figures.get(dress.figure ?? MALE);
      const preset = dress.variety !== null ? varied.get(`${resident}#${dress.variety}`) : residents?.get(resident);
      if (!figure || !preset) { throw new Error(`The Ranger bundle has no resident preset ${resident}.`); }
      const model = cloneSkinned(preset.character);
      if (preset.face) { applyFace(model, preset.face, preset.choice); }
      // The painted head skin (#380) before the tone, which multiplies it like the plain skin.
      if (preset.headSkin) { applyHeadSkin(model, preset.headSkin); }
      for (const name of figure.look.hood) {
        const node = model.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(name));
        if (node) { node.visible = false; }
      }
      for (const piece of preset.pieces) {
        const place = figure.sockets.get(piece.socket);
        if (!place) { throw new Error(`The ${figure.character} bundle has no socket ${piece.socket}.`); }
        const clone = piece.node.clone();
        if (piece.tint) { tintMaterials(clone, piece.materials, piece.tint); }
        // Plates and beards are authored in rest model space: their origin is the socket's.
        attachItem(model, clone, new THREE.Matrix4(), place);
      }
      const worn = [...(loadout ? LOADOUT_ITEMS[loadout] : []), ...(preset.headwear ? [{ key: preset.headwear, socket: "head.armor" }] : [])];
      figure.wear(model, worn, preset.hair, preset.tint);
      if (preset.faction) { tintMaterials(model, preset.faction.materials, preset.faction.color); }
      if (preset.tone) { tintMaterials(model, preset.tone.materials, preset.tone.color); }
      // Age and build keys (#380) after the face: build targets are on everything worn, age
      // targets on the head, brows and beards; each mesh takes the targets it has.
      if (Object.keys(preset.morphs).length > 0) { setMorphs(model, preset.morphs); }
      const posable = mixerPosable(model, figure.sources);
      posable.scale = preset.height;
      // Another figure stands on its own rest bounds, scaled to the collision height like the male.
      if (figure !== male) { posable.bounds = figure.bounds; }
      return posable;
    }
    const model = cloneSkinned(gltf.scene);
    if (face && chosen) { applyFace(model, face, chosen); }
    // The bundle's hood is on by default; any other chosen headwear replaces it.
    const hooded = headwear === null || headwear === "wayfarer-hood";
    // `outfit.hood` also names the class outfits' hoods (#379), absent from the default entry.
    const hoods = look.hood.flatMap((name) => model.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(name)) ?? []);
    if (hoods.length === 0) { throw new Error("The Ranger has none of its outfit.hood nodes."); }
    for (const node of hoods) { node.visible = hooded; }
    const worn = [...(loadout ? LOADOUT_ITEMS[loadout] : []), ...(headwear ? HEADWEAR_ITEMS[headwear] : [])];
    const hairHidden = hooded || worn.some(({ key }) => look.hides.get(key)?.includes("hair"));
    const hairKey = hair && hair.style !== "bald" && !hairHidden ? HAIR_ENTRIES[hair.style] : null;
    wear(model, worn, hairKey, hair ? look.tints.get(`tint.${hair.color}`) : undefined);
    return mixerPosable(model, sources);
  };
  let pending: Promise<void> | null = null;
  /** Every figure's residents (and the male's enemies), all or none (#418, #39, #493). */
  const loadResidents = () => {
    pending ??= (async () => {
      const others = await Promise.all(FIGURES.filter((character) => character !== MALE)
        .map((character) => loadFigure(loader, character, [], [], rest)));
      for (const figure of others) { figures.set(figure.character, figure); }
      const looks = await Promise.all([...figures.values()].map((figure) =>
        residentLooks(figure.appearance, figure.look, figure.scene, (id) => id.startsWith("resident.") || id.startsWith("enemy."))));
      residents = new Map(looks.flatMap((map) => [...map]));
    })();
    return pending;
  };
  /** Varied residents' looks (#380) by `<preset>#<seed>`: a look, or null while building or after failing. */
  const varied = new Map<string, ResidentLook | null>();
  const variedReady = (preset: string, seed: number, character: string): boolean => {
    const id = `${preset}#${seed}`;
    if (varied.has(id)) { return varied.get(id) !== null; }
    varied.set(id, null);
    void (async () => {
      const figure = figures.get(character);
      if (!figure) { throw new Error(`The Ranger figure ${character} has not loaded.`); }
      const own = varyPreset(figure.appearance, preset, seed);
      const looks = await residentLooks({ ...(figure.appearance as object), presets: [{ ...(own as object), id }] }, figure.look, figure.scene);
      const loaded = looks.get(id);
      if (!loaded) { throw new Error(`The ${character} bundle has no resident preset ${preset}.`); }
      varied.set(id, loaded);
    })().catch((error: unknown) => {
      console.warn(`The varied look ${id} failed to load; keeping the ${preset} preset.`, error);
    });
    return false;
  };
  const clips = new Map([...sources].map(([name, clip]) => [name, clip.duration]));
  return { clips, motions, instantiate, loadResidents, outfitReady, variedReady, face, ...male.bounds };
}

/** The drop armour outfit's plates by socket (#379), from the bundle's `DROP_ARMOR_OUTFIT`. */
async function dropPieces(appearance: unknown, load: (key: string, variant: string) => Promise<THREE.Object3D>): Promise<Map<string, THREE.Object3D>> {
  const outfits = (appearance as { outfits?: OutfitValue[] }).outfits ?? [];
  const armor = outfits.find((outfit) => outfit.id === DROP_ARMOR_OUTFIT)?.armor;
  if (!armor || typeof armor.entry !== "string") { throw new Error(`The Ranger bundle has no ${DROP_ARMOR_OUTFIT} armour.`); }
  const scene = await load(armor.entry, "default");
  const pieces = new Map<string, THREE.Object3D>();
  for (const piece of armor.pieces ?? []) {
    if (typeof piece.node !== "string" || typeof piece.socket !== "string") { continue; }
    const node = scene.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(piece.node));
    if (!node) { throw new Error(`The Ranger bundle's ${armor.entry} has no node ${piece.node}.`); }
    pieces.set(piece.socket, node);
  }
  return pieces;
}

/** The bundle outfit whose plates an equipped drop wears (`imported::DROP_ARMOR_OUTFIT`). */
const DROP_ARMOR_OUTFIT = "outfit.knight";

/** The class colour of a `class.<id>` preset's `classTrim` (#379): `character-visuals` accent, linear. */
function classTrimColor(preset: string): [number, number, number] | null {
  const classId = preset.startsWith("class.") ? preset.slice("class.".length) as CharacterClassId : null;
  if (!classId) { return null; }
  const color = new THREE.Color(characterVisualProfile({ classId, sex: "male" }).shoulderColor);
  return [color.r, color.g, color.b];
}

/**
 * One appearance preset resolved against the bundle: a resident's (#418) or a class starter
 * outfit's (#379) outfit model and what it wears.
 */
type ResidentLook = {
  /** The outfit variant for the preset's skin. */
  character: THREE.Object3D;
  face: FaceLook | null;
  choice: FaceChoice;
  /** Armour plates and beards on their sockets, beards tinted like the hair. */
  pieces: { node: THREE.Object3D; socket: string; beard: boolean; materials: readonly string[]; tint: readonly [number, number, number] | null }[];
  headwear: string | null;
  hair: string | null;
  tint: readonly [number, number, number] | undefined;
  /** The outfit's faction materials in the preset's colour (linear). */
  faction: { materials: readonly string[]; color: readonly [number, number, number] } | null;
  /** The outfit's `classTrim` materials in the class colour (linear, #379). */
  trim: { materials: readonly string[]; color: readonly [number, number, number] } | null;
  /** The glTF nodes of the outfit's own hood garment, if it has one (#379). */
  hood: readonly string[];
  /** The age and build morph weights (asset-tooling #380), set on every mesh carrying the target. */
  morphs: Readonly<Record<string, number>>;
  /** The root scale factor of the preset's height (#380). */
  height: number;
  /** The Light skin's skin materials in the preset's tone (linear, #380). */
  tone: { materials: readonly string[]; color: readonly [number, number, number] } | null;
  /** The painted head skin (#380) the head material wears: its age's and stubble's, or null. */
  headSkin: HeadSkin | null;
};

/** A painted head skin: the character's head material and the `skin.head` material whose maps it takes. */
type HeadSkin = { material: string; skin: THREE.MeshStandardMaterial };

type PresetValue = {
  id?: unknown; outfit?: unknown; skin?: unknown; hair?: unknown; tint?: unknown; headwear?: unknown;
  facialHair?: unknown; face?: unknown; hiddenArmor?: unknown; faction?: unknown;
  /** A varied look's `ages`/`builds`/`skinTones` option ids, height and stubble (asset-tooling #380). */
  age?: unknown; build?: unknown; tone?: unknown; height?: unknown; stubble?: unknown;
};
type OptionList = { default?: unknown; options?: { id?: unknown; morphs?: unknown; skin?: unknown }[] };
type HeadSkinsValue = {
  entry?: unknown; material?: unknown; stubbleShades?: Record<string, unknown>;
  textures?: { skin?: unknown; age?: unknown; stubble?: unknown; material?: unknown }[];
};
type VarietyValue = {
  /** The figure entry's key (`humanoid.ranger.male`, …) the outfit variants belong to. */
  character?: unknown;
  presets?: PresetValue[]; outfits?: OutfitValue[]; facialHair?: FacialHairValue;
  tints?: { id?: unknown }[]; face?: { features?: { id?: unknown; options?: { id?: unknown }[] }[] };
  ages?: OptionList; builds?: OptionList; heights?: { min?: unknown; max?: unknown };
  skinTones?: { skin?: unknown; materials?: unknown; tones?: { id?: unknown; baseColorFactor?: unknown }[] };
  headSkins?: HeadSkinsValue;
};

/**
 * The `headSkins` texture (#380) `preset` wears: for its skin, its age's painted skin and, when it
 * shows stubble, its hair tint's stubble shade. Null for the authored skin or a bundle without
 * head skins.
 */
async function headSkinOf(value: VarietyValue, preset: PresetValue,
  load: (key: string, variant: string) => Promise<THREE.Object3D>): Promise<HeadSkin | null> {
  const skins = value.headSkins;
  if (!skins || typeof skins.entry !== "string" || typeof skins.material !== "string") { return null; }
  const age = value.ages?.options?.find((option) => option.id === preset.age)?.skin;
  const ageSkin = typeof age === "string" ? age : "base";
  const shade = preset.stubble === true && typeof preset.tint === "string" ? skins.stubbleShades?.[preset.tint] : undefined;
  const painted = skins.textures?.find((texture) => texture.skin === preset.skin && texture.age === ageSkin
    && texture.stubble === (typeof shade === "string" ? shade : undefined))?.material;
  if (typeof painted !== "string") { return null; }
  let skin = null as THREE.MeshStandardMaterial | null;
  (await load(skins.entry, "default")).traverse((child) => {
    if (child instanceof THREE.Mesh && child.material instanceof THREE.MeshStandardMaterial && child.material.name === painted) { skin = child.material; }
  });
  if (!skin) { throw new Error(`The Ranger bundle's ${skins.entry} has no material ${painted}.`); }
  return { material: skins.material, skin };
}

/** Gives `model`'s head material its own copy wearing the painted skin's colour and normal maps. */
function applyHeadSkin(model: THREE.Object3D, { material, skin }: HeadSkin): void {
  model.traverse((child) => {
    if (!(child instanceof THREE.Mesh)) { return; }
    const swap = (own: THREE.Material): THREE.Material => {
      if (own.name !== material || !(own instanceof THREE.MeshStandardMaterial)) { return own; }
      const copy = own.clone();
      copy.map = skin.map;
      copy.normalMap = skin.normalMap;
      copy.needsUpdate = true;
      return copy;
    };
    child.material = Array.isArray(child.material) ? child.material.map(swap) : swap(child.material);
  });
}

/** The hair entries the bundle offers in the tintable `HAIR_VARIANT`. */
function tintableHair(appearance: unknown): string[] {
  const hair = (appearance as { hair?: { entry?: unknown; textures?: { variant?: unknown }[] }[] }).hair ?? [];
  return hair.flatMap((style) => (typeof style.entry === "string" && style.textures?.some((texture) => texture.variant === HAIR_VARIANT) ? [style.entry] : []));
}

/** Sets the `morphs` weights on every mesh of `model` (and its attached items) carrying the target. */
function setMorphs(model: THREE.Object3D, morphs: Readonly<Record<string, number>>): void {
  model.traverse((child) => {
    if (!(child instanceof THREE.Mesh) || !child.morphTargetInfluences || !child.morphTargetDictionary) { return; }
    for (const [target, weight] of Object.entries(morphs)) {
      const index = child.morphTargetDictionary[target];
      if (index !== undefined) { child.morphTargetInfluences[index] = weight; }
    }
  });
}

type OutfitValue = {
  id?: unknown; skins?: { id?: unknown; variant?: unknown }[];
  armor?: { entry?: unknown; pieces?: { node?: unknown; socket?: unknown }[] };
  faction?: { materials?: unknown };
  classTrim?: { materials?: unknown };
  garments?: { id?: unknown; nodes?: unknown }[];
  hood?: unknown;
};
type FacialHairValue = {
  entry?: unknown; options?: { id?: unknown; node?: unknown; socket?: unknown; tintMaterials?: unknown }[];
  tints?: { id?: unknown; baseColorFactor?: unknown }[];
};

/**
 * Resolves the `humanoid-appearance.presets` entries `select` picks (#418, #379): the outfit
 * variant of its skin (`outfits[].skins`), the outfit's armour plates less `hiddenArmor`, the
 * chosen beards in the matching `facialHair.tints` colour, headwear, hair and tint, face, faction
 * colour, and a class outfit's `classTrim` in the class colour and its own hood. `load` loads
 * each entry once.
 */
export async function residentLooks(appearance: unknown, look: Pick<HeadLook, "tints">,
  load: (key: string, variant: string) => Promise<THREE.Object3D>,
  select: (id: string) => boolean = () => true): Promise<Map<string, ResidentLook>> {
  const value = appearance as VarietyValue;
  const strings = (list: unknown): string[] => (Array.isArray(list) && list.every((item) => typeof item === "string") ? list : []);
  const rgb = (c: unknown): [number, number, number] | null =>
    (Array.isArray(c) && c.length >= 3 ? [Number(c[0]), Number(c[1]), Number(c[2])] : null);
  const scene = load;
  const node = async (key: string, name: string) => {
    // GLTFLoader drops the dots of `knight.breastplate` from node names.
    const found = (await scene(key, "default")).getObjectByName(THREE.PropertyBinding.sanitizeNodeName(name));
    if (!found) { throw new Error(`The Ranger bundle's ${key} has no node ${name}.`); }
    return found;
  };
  const looks = new Map<string, ResidentLook>();
  for (const preset of value.presets ?? []) {
    if (typeof preset.id !== "string" || !select(preset.id)) { continue; }
    const outfit = value.outfits?.find((candidate) => candidate.id === preset.outfit);
    const variant = outfit?.skins?.find((skin) => skin.id === preset.skin)?.variant;
    if (!outfit || typeof variant !== "string") {
      throw new Error(`The resident preset ${preset.id} names no outfit variant for its skin.`);
    }
    const character = await scene(String(value.character), variant);
    const hidden = strings(preset.hiddenArmor);
    const pieces: ResidentLook["pieces"] = [];
    const armor = outfit.armor;
    if (armor && typeof armor.entry === "string") {
      for (const piece of armor.pieces ?? []) {
        if (typeof piece.node !== "string" || typeof piece.socket !== "string" || hidden.includes(piece.node)) { continue; }
        pieces.push({ node: await node(armor.entry, piece.node), socket: piece.socket, beard: false, materials: [], tint: null });
      }
    }
    const tintId = typeof preset.tint === "string" ? preset.tint : null;
    const facialHair = value.facialHair;
    const beardTint = rgb(facialHair?.tints?.find((tint) => tint.id === tintId)?.baseColorFactor);
    for (const id of strings(preset.facialHair)) {
      const option = facialHair?.options?.find((candidate) => candidate.id === id);
      if (!option || typeof facialHair?.entry !== "string" || typeof option.node !== "string" || typeof option.socket !== "string") {
        throw new Error(`The resident preset ${preset.id} names an unknown beard ${id}.`);
      }
      pieces.push({ node: await node(facialHair.entry, option.node), socket: option.socket, beard: true, materials: strings(option.tintMaterials), tint: beardTint });
    }
    const factionColor = rgb(preset.faction);
    const factionMaterials = strings(outfit.faction?.materials);
    const trimMaterials = strings(outfit.classTrim?.materials);
    const trimColor = classTrimColor(preset.id);
    const hood = outfit.garments?.find((garment) => garment.id === outfit.hood);
    const morphsOf = (list: OptionList | undefined, option: unknown): Record<string, number> => {
      const morphs = list?.options?.find((candidate) => candidate.id === option)?.morphs;
      return Object.fromEntries(Object.entries((morphs ?? {}) as Record<string, unknown>).map(([target, weight]) => [target, Number(weight)]));
    };
    const toneColor = preset.skin === value.skinTones?.skin
      ? rgb(value.skinTones?.tones?.find((tone) => tone.id === preset.tone)?.baseColorFactor) : null;
    const toneMaterials = strings(value.skinTones?.materials);
    const headSkin = await headSkinOf(value, preset, scene);
    looks.set(preset.id, {
      headSkin,
      morphs: { ...morphsOf(value.ages, preset.age), ...morphsOf(value.builds, preset.build) },
      height: typeof preset.height === "number" && Number.isFinite(preset.height) ? preset.height : 1,
      tone: toneColor && toneMaterials.length > 0 ? { materials: toneMaterials, color: toneColor } : null,
      character,
      face: faceLookOf(appearance, variant, character),
      choice: Object.fromEntries(Object.entries((preset.face ?? {}) as Record<string, unknown>).map(([feature, option]) => [feature, String(option)])),
      pieces,
      headwear: typeof preset.headwear === "string" ? preset.headwear : null,
      hair: typeof preset.hair === "string" ? preset.hair : null,
      tint: tintId ? look.tints.get(tintId) : undefined,
      faction: factionColor && factionMaterials.length > 0 ? { materials: factionMaterials, color: factionColor } : null,
      trim: trimColor && trimMaterials.length > 0 ? { materials: trimMaterials, color: trimColor } : null,
      hood: strings(hood?.nodes),
    });
  }
  return looks;
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
  #residents: Promise<boolean> | null = null;
  #residentsLoaded = false;

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

  /**
   * Loads the residents' appearance presets (#418) after the Ranger; settles true once they can
   * be drawn, false when they failed (the residents then keep their procedural models).
   */
  preloadResidents(): Promise<boolean> {
    this.#residents ??= this.preload("Ranger").then(async (ready) => {
      const asset = this.#assets.get("Ranger");
      if (!ready || typeof asset !== "object" || !asset.loadResidents) { return false; }
      await asset.loadResidents();
      this.#residentsLoaded = true;
      return true;
    }).catch((error: unknown) => {
      console.warn("The residents' looks failed to load; keeping their procedural models.", error);
      return false;
    });
    return this.#residents;
  }

  /** Whether the residents' presets are drawable. */
  residentsLoaded(): boolean {
    return this.#residentsLoaded;
  }

  /** Models that finished loading since the last call, with their clip lengths. */
  drainLoaded(): [QuaterniusKind, [string, number][]][] {
    return this.#loaded.splice(0);
  }

  /** Poses every requested unit and drops the instances of units not requested. */
  /**
   * The face features the loaded Ranger supports (#411): undefined while it loads, null when it
   * failed or carries no face morphs.
   */
  faceFeatures(): readonly FaceFeature[] | null | undefined {
    const asset = this.#assets.get("Ranger");
    if (asset === undefined || asset === "loading") { return undefined; }
    return asset === "failed" ? null : asset.face?.features ?? null;
  }

  update(units: readonly ImportedRequest[], viewer: { headwear: Headwear; hair: Hair; face: FaceChoice }): void {
    const seen = new Set<string>();
    for (const unit of units) {
      const asset = this.#assets.get(unit.kind);
      if (typeof asset !== "object") { continue; }
      seen.add(unit.id);
      let instance = this.#instances.get(unit.id);
      const dressed = unit.headwear && unit.kind === "Ranger";
      const resident = unit.kind === "Ranger" && this.#residentsLoaded ? unit.resident : null;
      // Until its preset loads, a player keeps the bundle's default look.
      const outfit = unit.kind === "Ranger" && unit.outfit && asset.outfitReady?.(unit.outfit.preset) ? unit.outfit : null;      const dress: Dress = {
        loadout: unit.loadout, headwear: dressed ? viewer.headwear : null, hair: dressed ? viewer.hair : null,
        face: dressed ? viewer.face : null, resident, figure: resident !== null ? unit.figure : null, outfit,
        // Until a guard's own look has built, he wears the plain preset.
        variety: resident !== null && unit.variety !== null && asset.variedReady?.(resident, unit.variety, unit.figure ?? MALE) ? unit.variety : null,
      };
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
    const bounds = posable.bounds ?? asset;
    model.position.set(-bounds.centre[0], -bounds.floor, -bounds.centre[1]);
    const root = new THREE.Group();
    root.name = `${unit.id}-model`;
    root.add(model);
    const instance: Instance = {
      kind: unit.kind, dress: dressKey(dress), root, posable,
      clip: "", time: 0, layers: [], materials, tint: undefined, head: model.getObjectByName("Head") ?? null, entity: unit.entity,
    };
    this.#instances.set(unit.id, instance);
    this.#rootsChanged = true;
    return instance;
  }

  #pose(instance: Instance, asset: Asset, unit: ImportedRequest): void {
    const { clip } = unit;
    instance.clip = clip;
    instance.time = Math.min(unit.time, Math.max(0, (asset.clips.get(clip) ?? 0) - 1e-4));
    instance.layers = unit.layers;
    instance.posable.pose(unit.layers);
    const tint = unit.tint;
    if (instance.tint === undefined || String(instance.tint) !== String(tint)) {
      instance.tint = tint;
      for (const { material, base } of instance.materials) {
        material.color.copy(base);
        if (tint) { material.color.multiply(new THREE.Color(tint[0], tint[1], tint[2])); }
      }
    }
    const { root } = instance;
    root.position.set(unit.feet[0], unit.feet[1], unit.feet[2]);
    root.rotation.set(0, unit.yaw, 0);
    root.scale.setScalar((unit.heightMetres / (instance.posable.bounds ?? asset).height) * (instance.posable.scale ?? 1));
    swingHanging(instance.posable.model, unit.swing);
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
   * Debug read of a drawn unit's actual pose: its (upper-body) clip and clip time, its posed layers, colour tint, root transform, the
   * world-space extent of its posed skeleton and its head joint (metres, rounded to 0.1 mm); null
   * when no model is drawn.
   */
  pose(id: string): { clip: string; role: string | null; time: number; layers: readonly ClipLayer[]; tint: readonly number[] | null; root: number[]; skeleton: number[]; head: number[] | null } | null {
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
      clip: instance.clip, role: this.#motion(instance)?.role ?? null, time: round(instance.time), layers: instance.layers.map((layer) => ({ ...layer, time: round(layer.time), weight: round(layer.weight) })),
      tint: instance.tint ?? null,
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
