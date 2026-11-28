import * as THREE from "three";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadBundleGltfs } from "./static-bundle";
import { litObject } from "./lighting";
import type { LivestockSource } from "./scenery";
import { releaseMixer, retainedBundle } from "./zone-lifecycle";
import { shareSkeletons } from "./held-matrices";

/** The rolling `greyhaven-farm-animals` bundle (#849) the ambient farm animals come from. */
const FARM_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.farm-animals/`;
/** Floats per animal in a livestock frame: 16 model floats, the clip index and the clip time. */
const STRIDE = 18;

type Sheep = { root: THREE.Object3D; mixer: THREE.AnimationMixer; actions: (THREE.AnimationAction | null)[]; clip: number; time: number };

/**
 * The zone's simulated livestock (#1019): Lise's stray ewe and the ewes of her fold are NPCs the zone moves and wounds,
 * and the shared presenter poses each one in view as a farm sheep (`livestockFrame`): walking while it moves, grazing
 * while it stands, lying on its side once it fell. Drawn like the ambient animals (#849) with skinned clones of the
 * farm bundle's sheep, one per projected animal, grown as more come into view. The bundle loads the first time any
 * livestock is projected; until then, or if it fails, they are simply absent.
 */
export class Livestock {
  readonly #source: LivestockSource;
  #gltf: GLTF | null = null;
  #loading: Promise<boolean> | null = null;
  #sheep: Sheep[] = [];
  #drawn = 0;
  #changed = false;
  #rootsRevision = 0;
  #disposed = false;

  constructor(source: LivestockSource) {
    this.#source = source;
  }

  /** Loads the sheep once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    if (this.#disposed || this.#source.species === "") { return Promise.resolve(false); }
    this.#loading ??= retainedBundle(`livestock:${FARM_BUNDLE}:${this.#source.species}`, () => loadBundleGltfs(FARM_BUNDLE, [this.#source.species]))
      .then((gltfs) => {
        if (!this.#disposed) { this.#gltf = gltfs[0] ?? null; }
        return this.#gltf !== null;
      }, (error: unknown) => {
        console.error(`The livestock bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
        return false;
      });
    return this.#loading;
  }

  /** Releases the clones' mixers and skeletons (#1005); the retained species scene stays. Later calls do nothing. */
  dispose(): void {
    if (this.#disposed) { return; }
    this.#disposed = true;
    for (const sheep of this.#sheep) {
      releaseMixer(sheep.mixer);
      sheep.root.traverse((object) => { if ((object as THREE.SkinnedMesh).isSkinnedMesh) { (object as THREE.SkinnedMesh).skeleton.dispose(); } });
    }
    this.#sheep = [];
    this.#changed = true;
    this.#rootsRevision += 1;
  }

  #clone(gltf: GLTF): Sheep {
    const root = litObject(cloneSkinned(gltf.scene));
    // One skeleton per skin, not per mesh (#1257).
    shareSkeletons(root);
    root.name = this.#source.species;
    root.matrixAutoUpdate = false;
    const mixer = new THREE.AnimationMixer(root);
    const actions = this.#source.clips.map((name) => {
      const clip = gltf.animations.find((animation) => animation.name === name);
      return clip ? mixer.clipAction(clip) : null;
    });
    return { root, mixer, actions, clip: -1, time: NaN };
  }

  /** Poses one sheep per animal of `frame` (the presenter's `livestockFrame`) and hides the rest. */
  update(frame: Float32Array): void {
    const count = Math.floor(frame.length / STRIDE);
    if (count > 0 && this.#gltf === null) { void this.load(); }
    const gltf = this.#gltf;
    if (gltf === null || this.#disposed) { return; }
    while (this.#sheep.length < count) {
      this.#sheep.push(this.#clone(gltf));
      this.#changed = true;
      this.#rootsRevision += 1;
    }
    this.#drawn = count;
    for (let index = 0; index < this.#sheep.length; index += 1) {
      const sheep = this.#sheep[index]!;
      sheep.root.visible = index < count;
      if (index >= count) { continue; }
      const at = index * STRIDE;
      sheep.root.matrix.fromArray(frame, at);
      sheep.root.matrixWorldNeedsUpdate = true;
      // A model without the clip (`Eat`) rests in `Idle`.
      const clip = sheep.actions[frame[at + 16]!] ? frame[at + 16]! : 0;
      const action = sheep.actions[clip];
      if (!action) { continue; }
      const duration = action.getClip().duration;
      const time = duration > 0 ? frame[at + 17]! % duration : 0;
      if (sheep.clip === clip && sheep.time === time) { continue; }
      if (sheep.clip !== clip) {
        sheep.mixer.stopAllAction();
        action.play();
        sheep.clip = clip;
      }
      action.time = time;
      sheep.time = time;
      sheep.mixer.update(0);
    }
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#sheep.map((sheep) => sheep.root), changed };
  }

  /** Moves whenever the drawn roots may change (the world frame's root registry, #1060). */
  get rootsRevision(): number { return this.#rootsRevision; }

  /** Appends the drawn roots without consuming `roots()`'s change flag. */
  collectRoots(into: THREE.Object3D[]): void {
    for (const sheep of this.#sheep) { into.push(sheep.root); }
  }

  /** Acceptance state: how many sheep the latest frame drew, and whether their art loaded. */
  observe(): { loaded: boolean; drawn: number } {
    return { loaded: this.#gltf !== null, drawn: this.#drawn };
  }
}
