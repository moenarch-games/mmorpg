import * as THREE from "three";
import type { CameraView, Vec3 } from "./orbit-camera";
import { BatchedRoots } from "./static-batches";
import { loadStaticBundle } from "./static-bundle";
import { freeHandle, retainedBundle } from "./zone-lifecycle";
import type { WalkInSetHandle } from "./scenery";

/** Where `scripts/fetch-bundles.sh` places the walk-in buildings' bundles in `web/public`. */
const BUILDINGS = `${import.meta.env.BASE_URL}buildings/`;

/** One walk-in building: its facts and loaded roots; the shared set presents it. */
type Building = {
  bundle: string;
  door: number;
  doorEntry: string;
  entries: readonly string[];
  /** The entries' roots, out of the scene: placed, swung and hidden here, drawn through `batched`. */
  roots: THREE.Object3D[];
  /** The roots batched per material (#615); null until loaded. */
  batched: BatchedRoots | null;
  /** Its 17 floats per root within the set's frame. */
  frame: Float32Array | null;
  /** The frame the batches last followed, 17 floats per root (matrix and shown). */
  placed: Float32Array | null;
};

/**
 * The walk-in buildings (#158): the Greyhaven inn and Marshal Elden Greywatch's house, asset-tooling
 * bundles drawn as imported roots, placed, swung and hidden by the shared Rust presenters the native
 * client runs. Until a bundle loads its building is simply not drawn (its colliders are the zone's).
 * Presentation only.
 */
export class WalkInBuildings {
  readonly #buildings: Building[];
  /** The shared presenter of every building (#1286), null without any. */
  readonly #set: WalkInSetHandle | null;
  /** Inputs reused every frame instead of a fresh array per call (#1286). */
  readonly #feet = new Float64Array(3);
  readonly #eye = new Float64Array(3);
  readonly #target = new Float64Array(3);
  readonly #none = new Float64Array(0);
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #rootsRevision = 0;
  #disposed = false;

  constructor(set: WalkInSetHandle | null) {
    this.#set = set;
    this.#buildings = Array.from({ length: set?.count() ?? 0 }, (_, index) => ({
      bundle: set!.bundle(index), door: set!.door(index), doorEntry: set!.doorEntry(index), entries: set!.entries(index),
      roots: [], batched: null, frame: null, placed: null,
    }));
  }

  /**
   * Starts loading the bundles once and settles when all are drawable (true) or any failed (false); a
   * missing bundle is reported, never fatal to the world.
   */
  load(): Promise<boolean> {
    if (this.#disposed) { return Promise.resolve(false); }
    this.#loading ??= Promise.all(this.#buildings.map((building) => this.#load(building).then(() => true, (error: unknown) => {
      console.error(`The ${building.bundle} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    }))).then((loaded) => loaded.every(Boolean));
    return this.#loading;
  }

  async #load(building: Building): Promise<void> {
    // A walk-in cottage (#843) has no bundle: the Outpost kit and the settlement props draw it.
    if (building.entries.length === 0) { return; }
    // The entries are the bundle's, retained for every zone that draws it (#1005); this zone places its own clones,
    // which share their geometry and materials.
    const base = `${BUILDINGS}${building.bundle}/`;
    const entries = await retainedBundle(`walk-in:${base}:${building.entries.join(",")}`, () => loadStaticBundle(base, building.entries));
    // A zone replaced while the bundle loaded keeps nothing (#1005).
    if (this.#disposed) { return; }
    building.roots = entries.map((root) => root.clone());
    building.batched = new BatchedRoots(building.bundle, building.roots);
    this.#changed = true;
    this.#rootsRevision += 1;
    place(building);
  }

  /**
   * Releases what the buildings own for their zone (#1005): their batches and the shared presenters' WASM handles. The
   * retained entries stay for a revisit; a load in flight attaches nothing. Later calls do nothing.
   */
  dispose(): void {
    if (this.#disposed) { return; }
    this.#disposed = true;
    for (const building of this.#buildings) {
      building.batched?.dispose();
      building.batched = null;
      building.roots = [];
    }
    if (this.#set) { freeHandle(this.#set); }
    this.#changed = true;
    this.#rootsRevision += 1;
  }

  /** Eases each door toward its state in `doors` and hides roofs and ceilings while `viewer` (feet, metres) is inside. */
  update(doors: readonly { door: number; open: boolean }[], viewer: readonly [number, number, number] | null, dt: number, reducedMotion: boolean): void {
    if (!this.#set) { return; }
    const open = Uint32Array.from(doors.filter((door) => door.open).map((door) => door.door));
    // One call for every building (#1286); each building's 17 floats per root follow the last's.
    const frame = this.#set.frame(open, viewer ? this.#feetOf(viewer) : this.#none, dt, reducedMotion);
    let start = 0;
    for (const building of this.#buildings) {
      const end = start + building.entries.length * 17;
      building.frame = frame.subarray(start, end);
      start = end;
      place(building);
    }
  }

  /** Shared adaptive boom; each presenter retains its smooth recovery outside the room. */
  camera(view: CameraView, viewer: Vec3, dt: number, reduced: boolean): CameraView {
    if (!this.#set) { return view; }
    this.#eye.set(view.eye);
    this.#target.set(view.target);
    const eye = this.#set.camera(this.#feetOf(viewer), this.#eye, this.#target, dt, reduced);
    return { eye: [eye[0]!, eye[1]!, eye[2]!], target: view.target };
  }

  /** The walk-in cottages (#843), by house collider, whose room feet at `viewer` (metres) stand in. */
  cottagesInside(viewer: Vec3): Set<number> {
    return new Set(this.#set?.cottagesInside(this.#feetOf(viewer)) ?? []);
  }

  interior(viewer: Vec3): number {
    return this.#set?.lighting(this.#feetOf(viewer)) ?? 0;
  }

  pointLights(existing: Float32Array): Float32Array {
    const lamps = this.#set?.pointLights();
    if (!lamps?.length) { return existing; }
    const all = new Float32Array(existing.length + lamps.length);
    all.set(existing);
    all.set(lamps, existing.length);
    return all;
  }

  /** `viewer` in the reused feet array. */
  #feetOf(viewer: readonly [number, number, number]): Float64Array {
    this.#feet[0] = viewer[0];
    this.#feet[1] = viewer[1];
    this.#feet[2] = viewer[2];
    return this.#feet;
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#buildings.flatMap((building) => building.batched ? [building.batched.root] : []), changed };
  }

  /** Moves whenever the drawn roots may change (the world frame's root registry, #1060). */
  get rootsRevision(): number { return this.#rootsRevision; }

  /** Appends the drawn roots without consuming `roots()`'s change flag. */
  collectRoots(into: THREE.Object3D[]): void {
    for (const building of this.#buildings) {
      if (building.batched) { into.push(building.batched.root); }
    }
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

/** Places, swings and shows the roots by the presenter's frame; only roots whose part of it changed are re-batched. */
function place(building: Building): void {
  const frame = building.frame;
  const batched = building.batched;
  if (!frame || !batched) {
    return;
  }
  const placed = building.placed?.length === frame.length ? building.placed : null;
  building.roots.forEach((root, index) => {
    const start = index * 17;
    if (placed && sameRange(placed, frame, start, start + 17)) { return; }
    root.matrix.fromArray(frame, start);
    root.matrixWorldNeedsUpdate = true;
    root.visible = frame[start + 16] === 1;
    batched.sync(index);
  });
  building.placed = Float32Array.from(frame);
}

function sameRange(a: Float32Array, b: Float32Array, start: number, end: number): boolean {
  for (let index = start; index < end; index += 1) {
    if (a[index] !== b[index]) { return false; }
  }
  return true;
}
