import * as THREE from "three";
import { loadStaticBundle } from "./static-bundle";
import type { BuildingPresenterHandle } from "./scenery";

/** Where `scripts/fetch-bundles.sh` places the walk-in buildings' bundles in `web/public`. */
const BUILDINGS = `${import.meta.env.BASE_URL}buildings/`;

/** One walk-in building: its presenter and loaded roots. */
type Building = {
  presenter: BuildingPresenterHandle;
  bundle: string;
  door: number;
  doorEntry: string;
  entries: readonly string[];
  roots: THREE.Object3D[];
  frame: Float32Array | null;
};

/**
 * The walk-in buildings (#158): the Greyhaven inn and Marshal Elden Greywatch's house, asset-tooling
 * bundles drawn as imported roots, placed, swung and hidden by the shared Rust presenters the native
 * client runs. Until a bundle loads its building is simply not drawn (its colliders are the zone's).
 * Presentation only.
 */
export class WalkInBuildings {
  readonly #buildings: Building[];
  #loading: Promise<boolean> | null = null;
  #changed = false;

  constructor(presenters: readonly BuildingPresenterHandle[]) {
    this.#buildings = presenters.map((presenter) => ({
      presenter, bundle: presenter.bundle(), door: presenter.door(), doorEntry: presenter.doorEntry(), entries: presenter.entries(),
      roots: [], frame: null,
    }));
  }

  /**
   * Starts loading the bundles once and settles when all are drawable (true) or any failed (false); a
   * missing bundle is reported, never fatal to the world.
   */
  load(): Promise<boolean> {
    this.#loading ??= Promise.all(this.#buildings.map((building) => this.#load(building).then(() => true, (error: unknown) => {
      console.error(`The ${building.bundle} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    }))).then((loaded) => loaded.every(Boolean));
    return this.#loading;
  }

  async #load(building: Building): Promise<void> {
    building.roots = await loadStaticBundle(`${BUILDINGS}${building.bundle}/`, building.entries);
    this.#changed = true;
    place(building);
  }

  /** Eases each door toward its state in `doors` and hides roofs and ceilings while `viewer` (feet, metres) is inside. */
  update(doors: readonly { door: number; open: boolean }[], viewer: readonly [number, number, number] | null, dt: number, reducedMotion: boolean): void {
    for (const building of this.#buildings) {
      const open = doors.some((door) => door.door === building.door && door.open);
      building.frame = building.presenter.frame(open, Float64Array.from(viewer ?? []), dt, reducedMotion);
      place(building);
    }
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#buildings.flatMap((building) => building.roots), changed };
  }

  /** Shown roots for picking (occluders), with each door leaf's door ID. */
  pickables(): { door: number | null; object: THREE.Object3D }[] {
    return this.#buildings.flatMap((building) => building.roots
      .filter((root) => root.visible)
      .map((root) => ({ door: root.name === building.doorEntry ? building.door : null, object: root })));
  }

  /** Acceptance state of `bundle`'s building: whether its entries are loaded and which are shown. */
  observe(bundle: string): { loaded: boolean; shown: string[] } | null {
    const building = this.#buildings.find((candidate) => candidate.bundle === bundle);
    return building
      ? { loaded: building.roots.length > 0, shown: building.roots.filter((root) => root.visible).map((root) => root.name) }
      : null;
  }
}

function place(building: Building): void {
  const frame = building.frame;
  if (!frame) {
    return;
  }
  building.roots.forEach((root, index) => {
    root.matrix.fromArray(frame, index * 17);
    root.matrixWorldNeedsUpdate = true;
    root.visible = frame[index * 17 + 16] === 1;
  });
}
