import * as THREE from "three";
import { loadBundleGltfs } from "./static-bundle";
import { litObject } from "./lighting";
import type { AmbientAnimalsSource } from "./scenery";

/** The rolling `greyhaven-ambient-animals` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
const AMBIENT_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.ambient-animals/`;

type Animal = { root: THREE.Object3D; mixer: THREE.AnimationMixer; actions: (THREE.AnimationAction | null)[]; clip: number };

/**
 * Greyhaven's ambient cat, dog and rodent (#419): asset-tooling's skinned animals drawn as imported
 * roots, each posed every frame by the shared Rust presenter's authored loop (`Idle` and in-place
 * `Walk`) through its own `AnimationMixer`, as the native client does. Cosmetic only: never picked,
 * collided or replicated. Until the bundle loads, or if it fails, the animals are simply absent.
 */
export class AmbientAnimals {
  readonly #source: AmbientAnimalsSource;
  #animals: Animal[] = [];
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #last: { seconds: number; animate: boolean } | null = null;

  constructor(source: AmbientAnimalsSource) {
    this.#source = source;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The ambient animals bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      this.#animals = [];
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    if (this.#source.entries.length === 0) { return; }
    const gltfs = await loadBundleGltfs(AMBIENT_BUNDLE, this.#source.entries);
    this.#animals = gltfs.map((gltf, index) => {
      // Lit like the rest of the world (#600).
      const root = litObject(gltf.scene);
      root.name = this.#source.entries[index]!;
      root.matrixAutoUpdate = false;
      const mixer = new THREE.AnimationMixer(root);
      const actions = this.#source.clips.map((name) => {
        const clip = gltf.animations.find((animation) => animation.name === name);
        return clip ? mixer.clipAction(clip) : null;
      });
      return { root, mixer, actions, clip: -1 };
    });
    this.#changed = true;
    if (this.#last) { this.update(this.#last.seconds, this.#last.animate); }
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
      const clip = frame[at + 16]!;
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
