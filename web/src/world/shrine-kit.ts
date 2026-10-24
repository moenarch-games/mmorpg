import * as THREE from "three";
import { loadStaticBundle } from "./static-bundle";

/** The pinned `shrine-kit` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
const SHRINE_BUNDLE = `${import.meta.env.BASE_URL}scenery/shrine.kit/`;

export type ShrinePlacement = { key: string; matrix: Float32Array };

/**
 * The Sunken Shrine (#431): asset-tooling's lake-temple kit modules drawn where the shared Rust
 * presentation places them over the zone's shrine colliders, as the native client does. The
 * modules are static; until the bundle loads the shrine is simply not drawn. Presentation only.
 */
export class ShrineKit {
  readonly #entries: readonly string[];
  readonly #placements: readonly ShrinePlacement[];
  #roots: THREE.Object3D[] = [];
  #loading: Promise<boolean> | null = null;
  #changed = false;

  constructor(entries: readonly string[], placements: readonly ShrinePlacement[]) {
    this.#entries = entries;
    this.#placements = placements;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The shrine kit bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    if (this.#placements.length === 0) { return; }
    const modules = new Map((await loadStaticBundle(SHRINE_BUNDLE, this.#entries)).map((root) => [root.name, root]));
    this.#roots = this.#placements.map(({ key, matrix }, index) => {
      const module = modules.get(key);
      if (!module) { throw new Error(`The shrine kit has no ${key}.`); }
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

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#roots, changed };
  }

  /** Acceptance state: whether the modules are loaded, and which entries are placed. */
  observe(): { loaded: boolean; placed: string[] } {
    return { loaded: this.#roots.length > 0, placed: this.#placements.map((placement) => placement.key) };
  }
}
