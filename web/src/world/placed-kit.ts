import * as THREE from "three";
import { InstancedLevels } from "./instanced-levels";
import { loadBundleGltfs } from "./static-bundle";

/** A bundle the shared Rust presentation places at fixed placements. */
export type PlacedKitName = "nature" | "climbing";

/** Where `scripts/fetch-bundles.sh` puts each bundle in `web/public`, and its descriptor attachment. */
const KITS: Record<PlacedKitName, { bundle: string; descriptor: string; label: string }> = {
  /** The pinned `greyhaven-nature` bundle: Greyhaven's trees, rocks and bushes (#49). */
  nature: { bundle: `${import.meta.env.BASE_URL}scenery/greyhaven.nature/`, descriptor: "nature-descriptor", label: "greyhaven.nature" },
  /** The rolling `climbing-kit` bundle: Greyhaven's ladders, ropes and ivy (#621). */
  climbing: { bundle: `${import.meta.env.BASE_URL}scenery/climbing.kit/`, descriptor: "climbing-descriptor", label: "climbing.kit" },
};

/** The shared level-of-detail selection (`mmorpg_presentation::lod::Lods`, through `mmorpg-wasm`'s `KitLods`). */
export type KitLodsHandle = {
  /** An entry's variants, finest first, as `entry/variant`. */
  variants(entry: string): string[];
  /**
   * Per drawn level its placement, level and fade (1 settled; `t` incoming, `-t` outgoing), three floats each, or
   * `undefined` when unchanged since the previous update (the camera rests and nothing fades).
   */
  update(x: number, y: number, z: number, fovY: number, clipFromWorld: Float32Array, seconds: number, bias: number, fade: boolean): Float32Array | undefined;
};

export type PlacedKitSource = {
  kit: PlacedKitName;
  entries: readonly string[];
  /** Every placement: its entry and 16 column-major model floats (metres). */
  placements: readonly { key: string; matrix: Float32Array }[];
  /** The shared selection for these placements from the bundle's descriptor JSON. */
  lods(descriptor: string): KitLodsHandle;
};

/**
 * An asset-tooling bundle drawn at the placements the shared Rust presentation gives, as the native client does:
 * Greyhaven's trees, rocks and bushes from `greyhaven-nature` (#49) and its ladders, ropes and ivy from the
 * `climbing-kit` (#621). Each entry level is one instanced mesh per primitive whose geometry and materials every
 * placement shares ([`InstancedLevels`]); the shared selection picks each placement's level per frame and cross-fades
 * switches. Until the bundle loads (or when it fails) none is drawn and the static scene keeps the kit's procedural
 * fallback. Presentation only.
 */
export class PlacedKit {
  readonly #source: PlacedKitSource;
  #levels: InstancedLevels | null = null;
  #lods: KitLodsHandle | null = null;
  #loading: Promise<boolean> | null = null;
  #changed = false;
  readonly #clip = new THREE.Matrix4();
  readonly #clipFloats = new Float32Array(16);

  constructor(source: PlacedKitSource) {
    this.#source = source;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The ${KITS[this.#source.kit].label} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    const placements = this.#source.placements;
    if (placements.length === 0) { return; }
    const kit = KITS[this.#source.kit];
    const base = new URL(kit.bundle, window.location.href);
    const response = await fetch(new URL("manifest.json", base));
    if (!response.ok) { throw new Error(`manifest ${response.status}`); }
    const manifest = await response.json() as { assets?: { attachments?: { name: string; path: string }[] }[] };
    const attachment = manifest.assets?.flatMap((asset) => asset.attachments ?? []).find((entry) => entry.name === kit.descriptor);
    if (!attachment) { throw new Error(`the bundle has no ${kit.descriptor}`); }
    const descriptor = await fetch(new URL(attachment.path, base));
    if (!descriptor.ok) { throw new Error(`${kit.descriptor} ${descriptor.status}`); }
    const lods = this.#source.lods(await descriptor.text());
    const entries = this.#source.entries;
    const variants = entries.map((entry) => lods.variants(entry));
    const gltfs = await loadBundleGltfs(kit.bundle, variants.flat());
    let next = 0;
    const levels = variants.map((entryLevels) => entryLevels.map(() => gltfs[next++]!.scene));
    this.#levels = new InstancedLevels(kit.label, entries, levels, placements);
    this.#lods = lods;
    this.#changed = true;
  }

  /** Whether the kit is drawn (loaded); the static scene hides its fallback then. */
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
