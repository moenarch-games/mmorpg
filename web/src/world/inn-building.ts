import * as THREE from "three";
import { adaptImportedItemBundle } from "@moritzbrantner/three-d-renderer/imported-character";
import type { InnPresenterHandle } from "./scenery";

/** The unpacked `greyhaven-inn-v1` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
const INN_BUNDLE = `${import.meta.env.BASE_URL}buildings/greyhaven.inn/`;
const DOOR_ENTRY = "greyhaven.inn.door";
/** The inn's one door (`mmorpg_core::greyhaven_vale::inn::DOOR`). */
export const INN_DOOR = 1;

/**
 * The walk-in Greyhaven inn (#158): asset-tooling's bundle entries drawn as imported roots, placed,
 * swung and hidden by the shared Rust presenter the native client runs. The procedural inn is gone;
 * until the bundle loads the inn is simply not drawn (its colliders are the zone's). Presentation only.
 */
export class InnBuilding {
  readonly #presenter: InnPresenterHandle;
  readonly #entries: readonly string[];
  #roots: THREE.Object3D[] = [];
  #loading = false;
  #changed = false;
  #frame: Float32Array | null = null;

  constructor(presenter: InnPresenterHandle, entries: readonly string[]) {
    this.#presenter = presenter;
    this.#entries = entries;
  }

  /** Starts loading the bundle once; a missing bundle is reported, never fatal to the world. */
  load(): void {
    if (this.#loading) { return; }
    this.#loading = true;
    void this.#load().catch((error: unknown) => {
      console.error(`The Greyhaven inn bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
    });
  }

  async #load(): Promise<void> {
    const read = async (path: string) => {
      const response = await fetch(new URL(path, new URL(INN_BUNDLE, window.location.href)));
      if (!response.ok) {
        throw new Error(`Inn bundle file ${path} is missing (${response.status}); run scripts/fetch-bundles.sh.`);
      }
      return response.arrayBuffer();
    };
    const manifest: unknown = JSON.parse(new TextDecoder().decode(await read("manifest.json")));
    const roots: THREE.Object3D[] = [];
    for (const key of this.#entries) {
      const asset = await adaptImportedItemBundle({ manifest, key, readResource: read });
      const root = asset.createInstance().root;
      root.name = key;
      root.matrixAutoUpdate = false;
      roots.push(root);
    }
    this.#roots = roots;
    this.#changed = true;
    if (this.#frame) { this.#place(this.#frame); }
  }

  /** Eases the door toward `open` and hides the roof and ceiling while `viewer` (feet, metres) is inside. */
  update(open: boolean, viewer: readonly [number, number, number] | null, dt: number, reducedMotion: boolean): void {
    this.#frame = this.#presenter.frame(open, Float64Array.from(viewer ?? []), dt, reducedMotion);
    this.#place(this.#frame);
  }

  #place(frame: Float32Array): void {
    this.#roots.forEach((root, index) => {
      root.matrix.fromArray(frame, index * 17);
      root.matrixWorldNeedsUpdate = true;
      root.visible = frame[index * 17 + 16] === 1;
    });
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#roots, changed };
  }

  /** Shown roots for picking (occluders), with the door leaf's door ID. */
  pickables(): { door: number | null; object: THREE.Object3D }[] {
    return this.#roots
      .filter((root) => root.visible)
      .map((root) => ({ door: root.name === DOOR_ENTRY ? INN_DOOR : null, object: root }));
  }

  /** Acceptance state: which entries are loaded and shown. */
  observe(): { loaded: boolean; shown: string[] } {
    return { loaded: this.#roots.length > 0, shown: this.#roots.filter((root) => root.visible).map((root) => root.name) };
  }
}
