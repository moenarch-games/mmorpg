import * as THREE from "three";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadBundleGltfs } from "./static-bundle";
import { litObject } from "./lighting";
import type { AmbientAnimalsSource } from "./scenery";
import { releaseMixer, retainedBundle } from "./zone-lifecycle";

/** The rolling `greyhaven-ambient-animals` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
const AMBIENT_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.ambient-animals/`;
/** The rolling `greyhaven-farm-animals` bundle (#849), whose keys start `greyhaven.farm.`. */
const FARM_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.farm-animals/`;
const FARM_PREFIX = "greyhaven.farm.";
/** The rolling `greyhaven-wildlife` bundle (#857), whose keys start `greyhaven.wild.`. */
const WILD_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.wildlife/`;
const WILD_PREFIX = "greyhaven.wild.";

type Animal = { root: THREE.Object3D; mixer: THREE.AnimationMixer; actions: (THREE.AnimationAction | null)[]; clip: number };

/**
 * Greyhaven's ambient cat, dog and rodent (#419) and its farm animals (#849): asset-tooling's skinned animals drawn
 * as imported roots, each posed every frame by the shared Rust presenter's authored loop (`Idle`, in-place `Walk` and
 * a grazer's `Eat`, or `Idle` for a model without it) through its own `AnimationMixer`, as the native client does.
 * Each species loads once; its animals are skinned clones. Cosmetic only: never picked,
 * collided or replicated. Until the bundle loads, or if it fails, the animals are simply absent.
 */
export class AmbientAnimals {
  readonly #source: AmbientAnimalsSource;
  #animals: Animal[] = [];
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #rootsRevision = 0;
  #disposed = false;
  #last: { seconds: number; animate: boolean } | null = null;

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
    this.#animals = entries.map((key, index) => {
      const gltf = loaded.get(key)!;
      // Each animal is a skinned clone of its species' untouched, retained scene: its own skeleton, shared geometry.
      const scene = cloneSkinned(gltf.scene);
      // Lit like the rest of the world (#600).
      const root = litObject(scene);
      root.name = entries[index]!;
      root.matrixAutoUpdate = false;
      const mixer = new THREE.AnimationMixer(root);
      const actions = this.#source.clips.map((name) => {
        const clip = gltf.animations.find((animation) => animation.name === name);
        return clip ? mixer.clipAction(clip) : null;
      });
      return { root, mixer, actions, clip: -1 };
    });
    this.#changed = true;
    this.#rootsRevision += 1;
    if (this.#last) { this.update(this.#last.seconds, this.#last.animate); }
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

  /** Poses every animal at presentation `seconds`; without `animate` they hold still. */
  update(seconds: number, animate: boolean): void {
    this.#last = { seconds, animate };
    if (this.#animals.length === 0) { return; }
    const frame = this.#source.frame(seconds, animate);
    this.#animals.forEach((animal, index) => {
      const at = index * 18;
      animal.root.matrix.fromArray(frame, at);
      animal.root.matrixWorldNeedsUpdate = true;
      // A model without the clip (`Eat` on a pig or sheep) rests in `Idle`.
      const clip = animal.actions[frame[at + 16]!] ? frame[at + 16]! : 0;
      const action = animal.actions[clip];
      if (!action) { return; }
      if (animal.clip !== clip) {
        animal.mixer.stopAllAction();
        action.play();
        animal.clip = clip;
      }
      const duration = action.getClip().duration;
      action.time = duration > 0 ? frame[at + 17]! % duration : 0;
      animal.mixer.update(0);
    });
  }

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

  /** Acceptance state: whether the animals loaded, and each one's entry, clip and feet in metres. */
  observe(): { loaded: boolean; animals: { key: string; clip: string | null; feet: [number, number, number] }[] } {
    return {
      loaded: this.#animals.length > 0,
      animals: this.#animals.map((animal) => {
        const m = animal.root.matrix.elements;
        return { key: animal.root.name, clip: this.#source.clips[animal.clip] ?? null, feet: [m[12]!, m[13]!, m[14]!] };
      }),
    };
  }
}
