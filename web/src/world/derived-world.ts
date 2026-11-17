/**
 * The presented world's derived scenery, static scene and grass, built in a worker (#855) by its own instance of the
 * same `mmorpg-wasm` build, so the page's instance seeds them instead of blocking the page for seconds deriving them.
 * Every failure, or a browser without workers, reads as null: the page then derives as before.
 */

/** What the page sends the worker: the compiled module at once, then the derived-data cache's terrain grids (#526) or null. */
export type DerivedWorldRequest = { kind: "module"; module: WebAssembly.Module } | { kind: "grids"; grids: Uint8Array | null };

/** The terrain grids (`terrainGridCache`) and the scenery export (`scenery`). */
export type DerivedScenery = { grids: Uint8Array; scenery: string };

/** The static scene's layout and buffers (`staticScene*`) and its skeleton (`staticSceneSkeleton`, with the terrain's weights). */
export type DerivedScene = {
  layout: string;
  vertices: Float32Array;
  indices: Uint32Array;
  models: Float32Array;
  skeleton: Uint8Array;
};

export type DerivedWorldMessage =
  | ({ kind: "scenery" } & DerivedScenery)
  | ({ kind: "scene" } & DerivedScene)
  | { kind: "grass"; grass: Uint8Array }
  | { kind: "failed"; message: string };

/**
 * The worker's three answers, in order, each null once the worker failed or never ran. The scenery waits for
 * `supplyGrids`: the cached terrain grids, or null to derive them. `grass` is the grass's seed (`grassSeed`).
 */
export type DerivedWorld = {
  supplyGrids(grids: Uint8Array | null): void;
  scenery: Promise<DerivedScenery | null>;
  scene: Promise<DerivedScene | null>;
  grass: Promise<Uint8Array | null>;
};

/** The worker as the page sees it; tests replace it. */
export type DerivedWorldWorker = {
  onmessage: ((event: MessageEvent<DerivedWorldMessage>) => void) | null;
  onerror: ((event: ErrorEvent) => void) | null;
  postMessage(message: DerivedWorldRequest): void;
  terminate(): void;
};

function createWorker(): DerivedWorldWorker | null {
  if (typeof Worker !== "function") { return null; }
  try {
    return new Worker(new URL("./derived-world.worker.ts", import.meta.url), { type: "module" });
  } catch {
    return null;
  }
}

/** Starts the derivation in a worker; the page ends it after the grass, or on failure. */
export function deriveWorld(module: WebAssembly.Module, create: () => DerivedWorldWorker | null = createWorker): DerivedWorld {
  const worker = create();
  if (!worker) {
    return { supplyGrids: () => {}, scenery: Promise.resolve(null), scene: Promise.resolve(null), grass: Promise.resolve(null) };
  }
  let settleScenery: (value: DerivedScenery | null) => void = () => {};
  let settleScene: (value: DerivedScene | null) => void = () => {};
  let settleGrass: (value: Uint8Array | null) => void = () => {};
  const scenery = new Promise<DerivedScenery | null>((resolve) => { settleScenery = resolve; });
  const scene = new Promise<DerivedScene | null>((resolve) => { settleScene = resolve; });
  const grass = new Promise<Uint8Array | null>((resolve) => { settleGrass = resolve; });
  const fail = (message: string) => {
    console.warn(`Deriving the world in a worker failed; the page derives it instead: ${message}`);
    settleScenery(null);
    settleScene(null);
    settleGrass(null);
    worker.terminate();
  };
  worker.onmessage = ({ data }) => {
    if (data.kind === "scenery") {
      settleScenery({ grids: data.grids, scenery: data.scenery });
    } else if (data.kind === "scene") {
      const { layout, vertices, indices, models, skeleton } = data;
      settleScene({ layout, vertices, indices, models, skeleton });
    } else if (data.kind === "grass") {
      settleGrass(data.grass);
      worker.terminate();
    } else {
      fail(data.message);
    }
  };
  worker.onerror = (event) => {
    event.preventDefault();
    fail(event.message);
  };
  worker.postMessage({ kind: "module", module });
  return { supplyGrids: (grids) => { worker.postMessage({ kind: "grids", grids }); }, scenery, scene, grass };
}
