import * as THREE from "three";
import { InstancedLevels } from "./instanced-levels";
import { loadBundleGltfs } from "./static-bundle";

/** The pinned `greyhaven-nature` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const NATURE_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.nature/`;

/** The shared level-of-detail selection (`mmorpg_presentation::nature::Lods`, through `mmorpg-wasm`). */
export type NatureLodsHandle = {
  /** An entry's variants, finest first, as `entry/variant`. */
  variants(entry: string): string[];
  /**
   * Per drawn level its placement, level and fade (1 settled; `t` incoming, `-t` outgoing), three floats each, or
   * `undefined` when unchanged since the previous update (the camera rests and nothing fades).
   */
  update(x: number, y: number, z: number, fovY: number, clipFromWorld: Float32Array, seconds: number, bias: number, fade: boolean): Float32Array | undefined;
};

export type NatureSource = {
  entries: readonly string[];
  /** Every placed tree, rock and bush: its entry and 16 column-major model floats (metres). */
  placements: readonly { key: string; matrix: Float32Array }[];
  /** The shared selection for these placements from the bundle's `nature-descriptor` JSON. */
  lods(descriptor: string): NatureLodsHandle;
};

/**
 * Greyhaven's trees, rocks and bushes from asset-tooling's `greyhaven-nature` bundle (#49), placed where the shared Rust
 * presentation places them, as the native client does. Each entry level is one instanced mesh per primitive whose
 * geometry and materials every placement shares ([`InstancedLevels`]); the shared selection picks each placement's
 * level per frame and cross-fades switches. Until the bundle loads (or when it fails) none is drawn and the static
 */
export class Nature {
  readonly #source: NatureSource;
  #levels: InstancedLevels | null = null;
  #lods: NatureLodsHandle | null = null;
  #loading: Promise<boolean> | null = null;
  #changed = false;
  readonly #clip = new THREE.Matrix4();
  readonly #clipFloats = new Float32Array(16);

  constructor(source: NatureSource) {
    this.#source = source;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The Greyhaven nature bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    const placements = this.#source.placements;
    if (placements.length === 0) { return; }
    const base = new URL(NATURE_BUNDLE, window.location.href);
    const response = await fetch(new URL("manifest.json", base));
    if (!response.ok) { throw new Error(`manifest ${response.status}`); }
    const manifest = await response.json() as { assets?: { attachments?: { name: string; path: string }[] }[] };
    const attachment = manifest.assets?.flatMap((asset) => asset.attachments ?? []).find((entry) => entry.name === "nature-descriptor");
    if (!attachment) { throw new Error("the bundle has no nature-descriptor"); }
    const descriptor = await fetch(new URL(attachment.path, base));
    if (!descriptor.ok) { throw new Error(`nature-descriptor ${descriptor.status}`); }
    const lods = this.#source.lods(await descriptor.text());
    const entries = this.#source.entries;
    const variants = entries.map((entry) => lods.variants(entry));
    const gltfs = await loadBundleGltfs(NATURE_BUNDLE, variants.flat());
    let next = 0;
    const levels = variants.map((entryLevels) => entryLevels.map(() => gltfs[next++]!.scene));
    this.#levels = new InstancedLevels("greyhaven.nature", entries, levels, placements);
    this.#lods = lods;
    this.#changed = true;
  }

  /** Whether the trees, rocks and bushes are drawn (loaded); the static scene hides their fallback then. */
  get drawn(): boolean {
    return this.#lods !== null;
  }

  /** Picks each placement's level for `camera` and places the instances; `fade` false switches at once. */
  update(camera: THREE.PerspectiveCamera, seconds: number, fade: boolean, bias = 1): void {
    const lods = this.#lods;
    if (!lods || !this.#levels) { return; }
    this.#clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.#clipFloats.set(this.#clip.elements);
    const p = camera.position;
    // Unchanged (camera at rest, nothing fading): the instances stay uploaded as they are.
    const shown = lods.update(p.x, p.y, p.z, THREE.MathUtils.degToRad(camera.fov), this.#clipFloats, seconds, bias, fade);
    if (shown) { this.#levels.apply(shown); }
  }

  /** The drawn root, and whether it changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.drawn && this.#levels ? [this.#levels.root] : [], changed };
  }

  /** Acceptance state: whether the bundle is drawn, how many placements and how many instances each entry level draws. */
  observe(): { loaded: boolean; placements: number; drawn: Record<string, number[]> } {
    const drawn: Record<string, number[]> = {};
    const counts = this.#levels?.counts ?? [];
    this.#source.entries.forEach((entry, index) => { drawn[entry] = [...(counts[index] ?? [])]; });
    return { loaded: this.drawn, placements: this.#source.placements.length, drawn };
  }
}
