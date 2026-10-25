import * as THREE from "three";
import { loadStaticBundle } from "./static-bundle";

/** The pinned `shrine-kit` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const SHRINE_BUNDLE = `${import.meta.env.BASE_URL}scenery/shrine.kit/`;
/** The pinned `outpost-kit` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const OUTPOST_BUNDLE = `${import.meta.env.BASE_URL}buildings/outpost.kit/`;
/** The pinned `settlement-props` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const SETTLEMENT_BUNDLE = `${import.meta.env.BASE_URL}scenery/settlement.props/`;

export type KitPlacement = { key: string; matrix: Float32Array };

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
  #roots: THREE.Object3D[] = [];
  #loading: Promise<boolean> | null = null;
  #changed = false;

  constructor(bundle: string, name: string, entries: readonly string[], placements: readonly KitPlacement[]) {
    this.#bundle = bundle;
    this.#name = name;
    this.#entries = entries;
    this.#placements = placements;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The ${this.#name} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    if (this.#placements.length === 0) { return; }
    const modules = new Map((await loadStaticBundle(this.#bundle, this.#entries)).map((root) => [root.name, root]));
    this.#roots = this.#placements.map(({ key, matrix }, index) => {
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
    this.#changed = true;
  }

  /** Whether the modules are drawn (loaded); the static scene hides their fallback then. */
  get drawn(): boolean {
    return this.#roots.length > 0;
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#roots, changed };
  }

  /** Acceptance state: whether the modules are loaded, and which entries are placed. */
  observe(): { loaded: boolean; placed: string[] } {
    return { loaded: this.drawn, placed: this.#placements.map((placement) => placement.key) };
  }
}
