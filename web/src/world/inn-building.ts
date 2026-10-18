import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
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
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #frame: Float32Array | null = null;

  constructor(presenter: InnPresenterHandle, entries: readonly string[]) {
    this.#presenter = presenter;
    this.#entries = entries;
  }

  /**
   * Starts loading the bundle once and settles when it is drawable (true) or failed (false); a
   * missing bundle is reported, never fatal to the world.
   */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The Greyhaven inn bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    const url = (path: string) => new URL(path, new URL(INN_BUNDLE, window.location.href)).href;
    const response = await fetch(url("manifest.json"));
    if (!response.ok) {
      throw new Error(`Inn bundle manifest is missing (${response.status}); run scripts/fetch-bundles.sh.`);
    }
    const assets = ((await response.json()) as { assets?: { key: string; variant: string; path: string }[] }).assets ?? [];
    const loader = new GLTFLoader();
    const roots: THREE.Object3D[] = [];
    for (const key of this.#entries) {
      const entry = assets.find((asset) => asset.key === key && asset.variant === "default");
      if (!entry) {
        throw new Error(`The inn bundle has no ${key} entry.`);
      }
      // Plain three.js loading of the bundle's glTF (asset-tooling Atlas 7.1.1, #244).
      const root = (await loader.loadAsync(url(entry.path))).scene;
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
