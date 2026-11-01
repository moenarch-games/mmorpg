import * as THREE from "three";
import { InstancedLevels } from "./instanced-levels";
import { loadBundleGltfs, readBundleAttachment, shareMaterials } from "./static-bundle";

/** The rolling `fortress-kit` bundle `scripts/fetch-bundles.sh` places in `web/public` (#591). */
export const FORTRESS_BUNDLE = `${import.meta.env.BASE_URL}scenery/fortress.kit/`;

/** What the shared Rust presentation draws of a composition kit (`mmorpg_presentation::composition`). */
export type CompositionDressing = {
  /** Every entry key drawn, sorted. */
  entries: string[];
  /** Every piece: entry key and 16 column-major entry-to-world floats (metres); the lifts at their low stop. */
  placements: { key: string; matrix: number[] }[];
  /** Lift pieces: the platform the unit presenter places and the piece's index into `placements`. */
  lifts: { platform: number; placement: number }[];
  /** Per entry its variants, finest first. */
  lods: { key: string; variants: string[] }[];
};

/** A composition kit the world draws over its graybox: the bundle, where its descriptor is, and the Rust dressing. */
export type CompositionKitSource = {
  bundle: string;
  name: string;
  descriptor: { key: string; variant: string; attachment: string };
  /** The pieces whose bodies the descriptor fits exactly. */
  dress(descriptor: string): CompositionDressing;
  /** Moves a platform's lift piece to its base (metres): its new model matrix, or `undefined` when it did not move. */
  place(platform: number, x: number, y: number, z: number): Float32Array | undefined;
  /**
   * The shared selection (`mmorpg_presentation::lod::Lods`) for a camera: per drawn level its placement, level and
   * fade, three floats each, or `undefined` when unchanged (the camera rests, nothing fades, no lift moved).
   */
  update(x: number, y: number, z: number, fovY: number, clipFromWorld: Float32Array, seconds: number, bias: number, fade: boolean): Float32Array | undefined;
};

/** Where the unit presenter puts a platform the kit draws: its box's bottom centre, metres. */
export type PlatformPlacement = { platform: number; base: [number, number, number] };

/**
 * An asset-tooling composition kit (#591, the Battle Royale fortress): every piece whose bodies the shared Rust
 * presentation matched is drawn at its placement, each entry level one instanced mesh per primitive
 * ([`InstancedLevels`], #599); the shared selection picks each piece's level by the descriptor's screen sizes,
 * cross-fades switches and culls by the descriptor bounds. Lift pieces follow their platforms. Until the bundle
 * loads (or when it fails) nothing is drawn, and the static scene keeps the graybox. Presentation only.
 */
export class CompositionKit {
  readonly #source: CompositionKitSource;
  #levels: InstancedLevels | null = null;
  /** Per lift platform its piece's placement index. */
  #lifts = new Map<number, number>();
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #dressing: CompositionDressing | null = null;
  readonly #clip = new THREE.Matrix4();
  readonly #clipFloats = new Float32Array(16);

  constructor(source: CompositionKitSource) {
    this.#source = source;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The ${this.#source.name} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      this.#levels = null;
      this.#lifts.clear();
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    const { bundle, descriptor } = this.#source;
    const json = await readBundleAttachment(bundle, descriptor.key, descriptor.variant, descriptor.attachment);
    const dressing = this.#source.dress(json);
    const variants = new Map(dressing.lods.map((entry) => [entry.key, entry.variants]));
    const levels = dressing.entries.map((key) => variants.get(key) ?? ["default"]);
    const files = dressing.entries.flatMap((key, entry) => levels[entry]!.map((variant) => variant === "default" ? key : `${key}/${variant}`));
    const gltfs = await loadBundleGltfs(bundle, files, 4);
    shareMaterials(gltfs);
    let next = 0;
    const scenes = levels.map((entryLevels) => entryLevels.map(() => gltfs[next++]!.scene));
    this.#levels = new InstancedLevels(this.#source.name, dressing.entries, scenes, dressing.placements);
    this.#lifts = new Map(dressing.lifts.map(({ platform, placement }) => [platform, placement]));
    this.#dressing = dressing;
    this.#changed = true;
  }

  /** Whether the kit is drawn (loaded); the static scene hides the graybox of the bodies it dresses then. */
  get drawn(): boolean {
    return this.#levels !== null;
  }

  /** The platforms the kit's lift pieces draw, for the unit presenter (empty until drawn). */
  get platforms(): number[] {
    return [...this.#lifts.keys()];
  }

  /** Moves each lift piece to where the unit presenter puts its platform this frame; a resting lift costs nothing. */
  place(platforms: readonly PlatformPlacement[]): void {
    const levels = this.#levels;
    if (!levels) { return; }
    for (const { platform, base } of platforms) {
      const placement = this.#lifts.get(platform);
      if (placement === undefined) { continue; }
      const model = this.#source.place(platform, base[0], base[1], base[2]);
      if (model) { levels.move(placement, model); }
    }
  }

  /** Picks each piece's level for `camera` and places the instances; `fade` false switches at once. */
  update(camera: THREE.PerspectiveCamera, seconds: number, fade: boolean, bias = 1): void {
    const levels = this.#levels;
    if (!levels) { return; }
    this.#clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.#clipFloats.set(this.#clip.elements);
    const p = camera.position;
    // Unchanged (camera at rest, nothing fading, no lift moved): the instances stay uploaded as they are.
    const shown = this.#source.update(p.x, p.y, p.z, THREE.MathUtils.degToRad(camera.fov), this.#clipFloats, seconds, bias, fade);
    if (shown) { levels.apply(shown); }
  }

  /** The drawn root, and whether it changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#levels ? [this.#levels.root] : [], changed };
  }

  /** Acceptance state: whether the kit is drawn, its entries, how many static and lift pieces and instances per level. */
  observe(): { loaded: boolean; entries: string[]; statics: number; lifts: number; levels: Record<string, number> } {
    const levels: Record<string, number> = {};
    for (const entryLevels of this.#levels?.counts ?? []) {
      entryLevels.forEach((count, level) => {
        if (count > 0) { levels[`level${level}`] = (levels[`level${level}`] ?? 0) + count; }
      });
    }
    const lifts = this.#dressing?.lifts.length ?? 0;
    return {
      loaded: this.drawn,
      entries: this.#dressing?.entries ?? [],
      statics: (this.#dressing?.placements.length ?? 0) - lifts,
      lifts,
      levels,
    };
  }
}
