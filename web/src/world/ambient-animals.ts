import * as THREE from "three";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadBundleGltfs } from "./static-bundle";
import { litObject } from "./lighting";
import type { AmbientAnimalsSource } from "./scenery";
import { releaseMixer, retainedBundle } from "./zone-lifecycle";
import { ClipDetail, type ClipWork } from "./clip-detail";
import { HeldMatrices, shareSkeletons } from "./held-matrices";

/** The rolling `greyhaven-ambient-animals` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
const AMBIENT_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.ambient-animals/`;
/** The rolling `greyhaven-farm-animals` bundle (#849), whose keys start `greyhaven.farm.`. */
const FARM_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.farm-animals/`;
const FARM_PREFIX = "greyhaven.farm.";
/** The rolling `greyhaven-wildlife` bundle (#857), whose keys start `greyhaven.wild.`. */
const WILD_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.wildlife/`;
const WILD_PREFIX = "greyhaven.wild.";

/**
 * `lift` and `radius`: the species' bounding sphere, its centre `lift` metres above the feet (#1095). `held`: its
 * subtree's world matrices, recomputed only in a frame it moved or sampled its clip (#1257).
 */
type Animal = { root: THREE.Object3D; held: HeldMatrices; mixer: THREE.AnimationMixer; actions: (THREE.AnimationAction | null)[]; clip: number; time: number; lift: number; radius: number };

/** A species' bounding sphere around its feet, with room for its clips' reach; a generous default if unmeasurable. */
function speciesBounds(scene: THREE.Object3D): { lift: number; radius: number } {
  const sphere = new THREE.Box3().setFromObject(scene).getBoundingSphere(new THREE.Sphere());
  if (!Number.isFinite(sphere.radius) || sphere.radius <= 0) { return { lift: 1, radius: 3 }; }
  return { lift: sphere.center.y, radius: sphere.radius * 1.25 + Math.hypot(sphere.center.x, sphere.center.z) };
}

/**
 * Greyhaven's ambient cat, dog and rodent (#419) and its farm animals (#849): asset-tooling's skinned animals drawn
 * as imported roots, each posed by the shared Rust presenter's authored loop (`Idle`, in-place `Walk` and
 * a grazer's `Eat`, or `Idle` for a model without it) through its own `AnimationMixer`, as the native client does.
 * Only animals in view are drawn, and their clips sample by distance (#1095). Each species loads once; its animals are
 * skinned clones. Cosmetic only: never picked,
 * collided or replicated. Until the bundle loads, or if it fails, the animals are simply absent.
 */
export class AmbientAnimals {
  readonly #source: AmbientAnimalsSource;
  #animals: Animal[] = [];
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #rootsRevision = 0;
  #disposed = false;
  #last: { camera: THREE.Camera; seconds: number; animate: boolean } | null = null;
  /** Which animals sample their clip this frame (#1095); sized once they load. */
  #detail = new ClipDetail(0);
  /** The latest frame of every animal, drawn or not. */
  #frame: Float32Array = new Float32Array(0);

  constructor(source: AmbientAnimalsSource) {
    this.#source = source;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    if (this.#disposed) { return Promise.resolve(false); }
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The ambient animals bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      this.#animals = [];
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    if (this.#source.entries.length === 0) { return; }
    const entries = this.#source.entries;
    const species = [...new Set(entries)];
    const loaded = new Map<string, GLTF>();
    for (const [bundle, keys] of [
      [AMBIENT_BUNDLE, species.filter((key) => !key.startsWith(FARM_PREFIX) && !key.startsWith(WILD_PREFIX))],
      [FARM_BUNDLE, species.filter((key) => key.startsWith(FARM_PREFIX))],
      [WILD_BUNDLE, species.filter((key) => key.startsWith(WILD_PREFIX))],
    ] as const) {
      if (keys.length === 0) { continue; }
      // The species are the bundle's, retained for every zone that draws them (#1005).
      (await retainedBundle(`ambient:${bundle}:${keys.join(",")}`, () => loadBundleGltfs(bundle, keys)))
        .forEach((gltf, index) => loaded.set(keys[index]!, gltf));
    }
    // A zone replaced while the bundles loaded keeps nothing (#1005).
    if (this.#disposed) { return; }
    const bounds = new Map([...loaded].map(([key, gltf]) => [key, speciesBounds(gltf.scene)]));
    this.#animals = entries.map((key, index) => {
      const gltf = loaded.get(key)!;
      // Each animal is a skinned clone of its species' untouched, retained scene: its own skeleton, shared geometry.
      const scene = cloneSkinned(gltf.scene);
      shareSkeletons(scene);
      // Lit like the rest of the world (#600).
      const root = litObject(scene);
      root.name = entries[index]!;
      root.matrixAutoUpdate = false;
      const mixer = new THREE.AnimationMixer(root);
      const actions = this.#source.clips.map((name) => {
        const clip = gltf.animations.find((animation) => animation.name === name);
        return clip ? mixer.clipAction(clip) : null;
      });
      return { root, held: new HeldMatrices(root), mixer, actions, clip: -1, time: NaN, ...bounds.get(key)! };
    });
    this.#detail = new ClipDetail(this.#animals.length);
    this.#changed = true;
    this.#rootsRevision += 1;
    if (this.#last) { this.update(this.#last.camera, this.#last.seconds, this.#last.animate); }
  }

  /**
   * Releases what the animals own for their zone (#1005): their mixers and their clones' skeletons. The retained species
   * scenes stay for a revisit; a load in flight attaches nothing. Later calls do nothing.
   */
  dispose(): void {
    if (this.#disposed) { return; }
    this.#disposed = true;
    for (const animal of this.#animals) {
      releaseMixer(animal.mixer);
      animal.root.traverse((object) => { if ((object as THREE.SkinnedMesh).isSkinnedMesh) { (object as THREE.SkinnedMesh).skeleton.dispose(); } });
    }
    this.#animals = [];
    this.#changed = true;
    this.#rootsRevision += 1;
  }

  /**
   * Poses the animals at presentation `seconds` for `camera` (its world matrix and projection current); without
   * `animate` they hold still. An animal outside the view is hidden and left as it is; one in view is placed, and
   * samples its clip as [`ClipDetail`] decides, at the frame's clip time, so it resumes on its true phase (#1095).
   */
  update(camera: THREE.Camera, seconds: number, animate: boolean): void {
    this.#last = { camera, seconds, animate };
    if (this.#animals.length === 0) { return; }
    const frame = this.#source.frame(seconds, animate);
    this.#frame = frame;
    const detail = this.#detail;
    detail.begin(camera, animate);
    for (let index = 0; index < this.#animals.length; index += 1) {
      const animal = this.#animals[index]!;
      const at = index * 18;
      const x = frame[at + 12]!, y = frame[at + 13]! + animal.lift, z = frame[at + 14]!;
      const visible = detail.visible(x, y, z, animal.radius);
      animal.root.visible = visible;
      if (!visible) { continue; }
      // Placed whenever it moved, so a visible animal is never drawn where it was.
      const matrix = animal.root.matrix.elements;
      let moved = false;
      for (let i = 0; i < 16; i += 1) { if (matrix[i] !== frame[at + i]) { moved = true; break; } }
      if (moved) {
        animal.root.matrix.fromArray(frame, at);
        animal.root.matrixWorldNeedsUpdate = true;
      }
      if (!detail.sample(index, x, y, z, animal.radius)) { continue; }
      // A model without the clip (`Eat` on a pig or sheep) rests in `Idle`.
      const clip = animal.actions[frame[at + 16]!] ? frame[at + 16]! : 0;
      const action = animal.actions[clip];
      if (!action) { continue; }
      const duration = action.getClip().duration;
      const time = duration > 0 ? frame[at + 17]! % duration : 0;
      // An unchanged pose (still, or a held clip) is not sampled again.
      if (animal.clip === clip && animal.time === time) { continue; }
      if (animal.clip !== clip) {
        animal.mixer.stopAllAction();
        action.play();
        animal.clip = clip;
      }
      action.time = time;
      animal.time = time;
      animal.mixer.update(0);
      animal.held.touch();
    }
  }

  /** The latest frame's clip work (`ambient-clips`, #1095). */
  get clipWork(): Readonly<ClipWork> { return this.#detail.work; }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#animals.map((animal) => animal.root), changed };
  }

  /** Moves whenever the drawn roots may change (the world frame's root registry, #1060). */
  get rootsRevision(): number { return this.#rootsRevision; }

  /** Appends the drawn roots without consuming `roots()`'s change flag. */
  collectRoots(into: THREE.Object3D[]): void {
    for (const animal of this.#animals) { into.push(animal.root); }
  }

  /**
   * Acceptance state: whether the animals loaded, and each one's entry, clip and feet in metres from the latest frame,
   * drawn or not.
   */
  observe(): { loaded: boolean; animals: { key: string; clip: string | null; feet: [number, number, number] }[] } {
    const frame = this.#frame;
    return {
      loaded: this.#animals.length > 0,
      animals: this.#animals.map((animal, index) => {
        const at = index * 18;
        if (frame.length < at + 18) { return { key: animal.root.name, clip: null, feet: [0, 0, 0] }; }
        const clip = animal.actions[frame[at + 16]!] ? frame[at + 16]! : 0;
        return { key: animal.root.name, clip: this.#source.clips[clip] ?? null, feet: [frame[at + 12]!, frame[at + 13]!, frame[at + 14]!] };
      }),
    };
  }
}
