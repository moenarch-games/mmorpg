import * as THREE from "three";
import type { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { applyOutpostWear, OUTPOST_WEAR_MASKS, wearMasks } from "./outpost-wear";
import { BatchedRoots } from "./static-batches";
import { drawnResources, loadBundleGltfs, loadStaticBundle } from "./static-bundle";
import { releaseMixer, retainedBundle } from "./zone-lifecycle";

/** The rolling `shrine-kit` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const SHRINE_BUNDLE = `${import.meta.env.BASE_URL}scenery/shrine.kit/`;
/** The rolling `outpost-kit` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const OUTPOST_BUNDLE = `${import.meta.env.BASE_URL}buildings/outpost.kit/`;
/** The rolling `settlement-props` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const SETTLEMENT_BUNDLE = `${import.meta.env.BASE_URL}scenery/settlement.props/`;

/** A placed module; `wear` in [0, 1] ages it when its kit ships wear masks (the Outpost kit). */
/** `cottage`: the walk-in cottage (#843) whose roof part it is, by its house collider. */
export type KitPlacement = { key: string; matrix: Float32Array; wear?: number; cottage?: number };

/**
 * How a kit loads: `ktx2` transcodes its KTX2 textures (the Outpost kit's, asset-tooling docs/outpost-kit.md), and
 * `wear` blends each placement's wear from the bundle's wear masks when it has them.
 */
export type KitOptions = { ktx2?: KTX2Loader; wear?: boolean };

/**
 * An asset-tooling kit's static modules drawn where the shared Rust presentation places them, as
 * the native client does: the Sunken Shrine (#431), the Outpost kit buildings (#420) and the
 * market square's settlement props (#421). Until
 * the bundle loads (or when it fails) no module is drawn; the Outpost's static scene keeps its
 * blockouts and the square its prop boxes meanwhile. Presentation only.
 */
export class KitModules {
  readonly #bundle: string;
  readonly #name: string;
  readonly #entries: readonly string[];
  readonly #placements: readonly KitPlacement[];
  /** The placed modules, batched per material (#615); null until loaded. */
  #batched: BatchedRoots | null = null;
  /** The placed modules' roots, in placement order, once loaded. */
  #roots: THREE.Object3D[] = [];
  readonly #options: KitOptions;
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #rootsRevision = 0;
  /** The placed modules that play their entries' looping node clips (#891), by placement index. */
  #mixers: { mixer: THREE.AnimationMixer; index: number }[] = [];
  /** How many materials age by wear (0 without wear masks). */
  #worn = 0;
  #disposed = false;

  constructor(bundle: string, name: string, entries: readonly string[], placements: readonly KitPlacement[], options: KitOptions = {}) {
    this.#bundle = bundle;
    this.#name = name;
    this.#entries = entries;
    this.#placements = placements;
    this.#options = options;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    if (this.#disposed) { return Promise.resolve(false); }
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The ${this.#name} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    if (this.#placements.length === 0) { return; }
    const { ktx2, wear } = this.#options;
    // The modules are the bundle's, prepared once and retained for every zone that places them (#1005).
    const prepared = await retainedBundle(`kit-modules:${this.#bundle}:${wear ? "worn" : "pristine"}:${this.#entries.join(",")}`, async () => {
      const modules = new Map((await loadStaticBundle(this.#bundle, this.#entries, ktx2)).map((root) => [root.name, root]));
      // Wear masks: every wear level then draws with the pristine materials, so in their batches. A bundle without them
      // (before asset-tooling#511) draws every building pristine.
      const masks = wear
        ? await loadBundleGltfs(this.#bundle, [OUTPOST_WEAR_MASKS], 1, ktx2).then(([gltf]) => wearMasks(gltf!), () => null)
        : null;
      return { modules, worn: masks ? applyOutpostWear([...modules.values()], masks) : 0 };
    });
    // A zone replaced while the bundle loaded keeps nothing (#1005).
    if (this.#disposed) { return; }
    const modules = prepared.modules;
    this.#worn = prepared.worn;
    const roots = this.#placements.map(({ key, matrix }, index) => {
      const module = modules.get(key);
      if (!module) { throw new Error(`The ${this.#name} has no ${key}.`); }
      // Clones share the module's geometry and materials.
      const root = module.clone();
      root.name = `${key}#${index}`;
      root.matrixAutoUpdate = false;
      root.matrix.fromArray(matrix);
      root.matrixWorldNeedsUpdate = true;
      return root;
    });
    // A module with node clips (#891) loops them all; its moving nodes stay in the batches, re-read each pose.
    this.#mixers = roots.flatMap((root, index) => {
      const clips = modules.get(this.#placements[index]!.key)?.animations ?? [];
      if (clips.length === 0) { return []; }
      const mixer = new THREE.AnimationMixer(root);
      for (const clip of clips) { mixer.clipAction(clip).play(); }
      mixer.setTime(0);
      return [{ mixer, index }];
    });
    // Static modules: a few draw calls per material instead of one per module and primitive.
    this.#roots = roots;
    this.#batched = new BatchedRoots(this.#name, roots, this.#worn > 0 ? this.#placements.map((placement) => placement.wear ?? 0) : null);
    this.#changed = true;
    this.#rootsRevision += 1;
  }

  /**
   * Releases what these placements own for their zone (#1005): the batches and the clip mixers. The clones share the
   * retained modules' geometry and materials, which stay for a revisit; a load in flight attaches nothing. Later calls
   * do nothing.
   */
  dispose(): void {
    if (this.#disposed) { return; }
    this.#disposed = true;
    for (const { mixer } of this.#mixers) { releaseMixer(mixer); }
    this.#mixers = [];
    this.#batched?.dispose();
    this.#batched = null;
    this.#roots = [];
    this.#changed = true;
    this.#rootsRevision += 1;
  }

  /**
   * Poses the placed modules whose entries carry looping node clips (#891: mill wheels, sails, signs, vanes, the
   * scarecrows) at presentation `seconds`; without `animate` they hold their first pose. Presentation only.
   */
  update(seconds: number, animate: boolean): void {
    const batched = this.#batched;
    if (!batched) { return; }
    const time = animate ? seconds : 0;
    for (const { mixer, index } of this.#mixers) {
      mixer.setTime(time);
      batched.sync(index);
    }
  }

  /** How many placed modules play a node clip (#891). */
  get animated(): number {
    return this.#mixers.length;
  }

  /** Hides the roof parts of the walk-in cottages in `houses` (#843) and shows every other. */
  hideCottages(houses: ReadonlySet<number>): void {
    const batched = this.#batched;
    if (!batched) { return; }
    this.#placements.forEach((placement, index) => {
      if (placement.cottage === undefined) { return; }
      const root = this.#roots[index]!;
      const visible = !houses.has(placement.cottage);
      if (root.visible === visible) { return; }
      root.visible = visible;
      batched.sync(index);
    });
  }

  /** How many placements are walk-in cottages' roof parts (#843). */
  cottageRoofCount(): number {
    return this.#placements.filter((placement) => placement.cottage !== undefined).length;
  }

  /** Whether the modules are drawn (loaded); the static scene hides their fallback then. */
  get drawn(): boolean {
    return this.#batched !== null;
  }

  /** The drawn modules for picking: they occlude what stands behind them, as their hidden fallback did. */
  pickables(): { door: null; object: THREE.Object3D }[] {
    return this.#batched ? [{ door: null, object: this.#batched.root }] : [];
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#batched ? [this.#batched.root] : [], changed };
  }

  /** Moves whenever the drawn roots may change (the world frame's root registry, #1060). */
  get rootsRevision(): number { return this.#rootsRevision; }

  /** Appends the drawn roots without consuming `roots()`'s change flag. */
  collectRoots(into: THREE.Object3D[]): void {
    if (this.#batched) { into.push(this.#batched.root); }
  }

  /**
   * Acceptance state: whether the modules are loaded, which entries are placed, how many materials age by wear, how
   * many placements play node clips (#891) and the GPU bytes of the textures they draw with (a compressed texture's
   * mip levels as uploaded, an image as RGBA8 with its mip chain).
   */
  observe(): { loaded: boolean; placed: string[]; worn: number; animated: number; textureBytes: number } {
    return { loaded: this.drawn, placed: this.#placements.map((placement) => placement.key), worn: this.#worn, animated: this.animated, textureBytes: this.#textureBytes() };
  }

  #textureBytes(): number {
    const { textures } = drawnResources(this.#batched?.root);
    let bytes = 0;
    for (const texture of textures) {
      if (texture instanceof THREE.CompressedTexture) {
        bytes += texture.mipmaps.reduce((sum, level) => sum + ((level as { data?: ArrayBufferView }).data?.byteLength ?? 0), 0);
      } else {
        const image = texture.image as { width?: number; height?: number } | undefined;
        bytes += Math.round((image?.width ?? 0) * (image?.height ?? 0) * 4 * 4 / 3);
      }
    }
    return bytes;
  }
}
