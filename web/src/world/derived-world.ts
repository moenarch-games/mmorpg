/**
 * The zone content's costly base parts and the presented world's derived scenery, static scene, terrain far levels and
 * grass, built in a
 * worker (#855) by its own instance of the same `mmorpg-wasm` build, so the page's instance seeds them instead of
 * blocking the page for seconds deriving them.
 * Every failure, or a browser without workers, reads as null: the page then derives as before.
 */

/**
 * What the page sends the worker: the compiled module at once, then what the derived-data cache held for this release
 * (#526, #855): the terrain grids and the zone content's base parts (`LocalZone.hostedContentSeed`), each or both null.
 */
export type DerivedWorldRequest =
  | { kind: "module"; module: WebAssembly.Module }
  | { kind: "cached"; grids: Uint8Array | null; content: Int32Array | null };

/** The terrain grids (`terrainGridCache`) and the scenery export (`scenery`). */
export type DerivedScenery = { grids: Uint8Array; scenery: string };

/**
 * The static scene's layout (`staticSceneLayout`: its JSON as UTF-8, handed to the page's instance without decoding),
 * its buffers (`staticScene*`) and its skeleton (`staticSceneSkeleton`, with the terrain's weights).
 */
export type DerivedScene = {
  layout: Uint8Array;
  vertices: Float32Array;
  indices: Uint32Array;
  models: Float32Array;
  skeleton: Uint8Array;
};

/** Every terrain tile's far level (`terrainFarLevels`) of the scene with `prefix` (`staticScenePrefix`). */
export type DerivedFarLevels = { prefix: string; levels: Float32Array };

export type DerivedWorldMessage =
  | { kind: "content"; seed: Int32Array | null }
  | ({ kind: "scenery" } & DerivedScenery)
  | ({ kind: "scene" } & DerivedScene)
  | ({ kind: "far" } & DerivedFarLevels)
  | { kind: "grass"; grass: Uint8Array }
  | { kind: "failed"; message: string };

/**
 * The worker's five answers, in order, each null once the worker failed or never ran. The worker starts once
 * `supplyCached` gave it the derived-data cache's terrain grids and content seed (each null on a miss). `content` is the
 * zone content's costly base parts (`LocalZone.hostedContentSeed`) as the worker built them, or null when it reused the
 * cached seed. `far` is the terrain's far levels, `grass` the grass's seed (`grassSeed`).
 */
export type DerivedWorld = {
  supplyCached(grids: Uint8Array | null, content: Int32Array | null): void;
  content: Promise<Int32Array | null>;
  scenery: Promise<DerivedScenery | null>;
  scene: Promise<DerivedScene | null>;
  far: Promise<DerivedFarLevels | null>;
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
    return {
      supplyCached: () => {}, content: Promise.resolve(null), scenery: Promise.resolve(null), scene: Promise.resolve(null),
      far: Promise.resolve(null), grass: Promise.resolve(null),
    };
  }
  let settleContent: (value: Int32Array | null) => void = () => {};
  let settleScenery: (value: DerivedScenery | null) => void = () => {};
  let settleScene: (value: DerivedScene | null) => void = () => {};
  let settleFar: (value: DerivedFarLevels | null) => void = () => {};
  let settleGrass: (value: Uint8Array | null) => void = () => {};
  const content = new Promise<Int32Array | null>((resolve) => { settleContent = resolve; });
  const scenery = new Promise<DerivedScenery | null>((resolve) => { settleScenery = resolve; });
  const scene = new Promise<DerivedScene | null>((resolve) => { settleScene = resolve; });
  const far = new Promise<DerivedFarLevels | null>((resolve) => { settleFar = resolve; });
  const grass = new Promise<Uint8Array | null>((resolve) => { settleGrass = resolve; });
  const fail = (message: string) => {
    console.warn(`Deriving the world in a worker failed; the page derives it instead: ${message}`);
    settleContent(null);
    settleScenery(null);
    settleScene(null);
    settleFar(null);
    settleGrass(null);
    worker.terminate();
  };
  worker.onmessage = ({ data }) => {
    if (data.kind === "content") {
      settleContent(data.seed);
    } else if (data.kind === "scenery") {
      settleScenery({ grids: data.grids, scenery: data.scenery });
    } else if (data.kind === "scene") {
      const { layout, vertices, indices, models, skeleton } = data;
      settleScene({ layout, vertices, indices, models, skeleton });
    } else if (data.kind === "far") {
      settleFar({ prefix: data.prefix, levels: data.levels });
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
  return { supplyCached: (grids, content) => { worker.postMessage({ kind: "cached", grids, content }); }, content, scenery, scene, far, grass };
}
