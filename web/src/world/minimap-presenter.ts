import type { EnvironmentStyle } from "./environment";
import {
  COARSE_MARGIN_METRES, buildMinimapLayers, buildMinimapVectors, coarseMinimapFrame, fullMinimapFrame, minimapTerrainSource, paintMinimapImage,
  vectorsWithin, type MinimapDisturbance, type MinimapImage, type MinimapUnit,
} from "./minimap";
import { rasterizeMinimapTerrain, type MinimapLayers, type MinimapRasterRequest, type MinimapRasterResult, type MinimapVectors } from "./minimap-raster";
import type { Scenery } from "./scenery";

/** The part of a `Worker` the refinement uses. */
export type MinimapWorker = {
  postMessage(message: MinimapRasterRequest, transfer: Transferable[]): void;
  terminate(): void;
  onmessage: ((event: MessageEvent<MinimapRasterResult>) => void) | null;
  onerror: ((event: ErrorEvent) => void) | null;
};

/** What the presenter needs from the page; tests replace these. */
export type MinimapPresenterPlatform = {
  /** Null where workers are unavailable. */
  createWorker(): MinimapWorker | null;
  paint(layers: MinimapLayers): MinimapImage;
  whenIdle(run: () => void): void;
};

function whenIdle(run: () => void): void {
  if (typeof requestIdleCallback === "function") { requestIdleCallback(run, { timeout: 2_000 }); } else { setTimeout(run, 200); }
}

function createMinimapWorker(): MinimapWorker | null {
  if (typeof Worker !== "function") { return null; }
  try {
    return new Worker(new URL("./minimap-raster.worker.ts", import.meta.url), { type: "module" });
  } catch {
    return null;
  }
}

const BROWSER_PLATFORM: MinimapPresenterPlatform = { createWorker: createMinimapWorker, paint: paintMinimapImage, whenIdle };

/** What the presenter shows the image on. */
export type MinimapDisplay = {
  show(image: MinimapImage | null): void;
  draw(self: { x: number; z: number }, heading: number, facing: number, units: readonly MinimapUnit[], disturbance: MinimapDisturbance | null): void;
};

/** Full maps kept for re-entering a world: the Vale and an arena, so entering a third drops the oldest. */
const RETAINED_MAPS = 2;

function release(image: MinimapImage | undefined): void {
  if (image && "close" in image.source) { image.source.close(); }
}

/**
 * Chooses the minimap's image without holding up the world (#527): a small map around the player from the first frame,
 * then the whole map from a worker, whose finished bitmap arrives without touching the main thread. Where workers
 * are unavailable or fail, the page builds the whole map from idle time, as it used to.
 */
export class MinimapPresenter {
  readonly #display: MinimapDisplay;
  readonly #environment: EnvironmentStyle;
  readonly #platform: MinimapPresenterPlatform;
  readonly #retained = new Map<string, MinimapImage>();
  /** The scenery this map is for; null until the first frame of a world. */
  #scenery: Scenery | null = null;
  #vectors: MinimapVectors | null = null;
  #coarseCenter: { x: number; z: number } | null = null;
  #worker: MinimapWorker | null = null;
  #refined = false;

  constructor(display: MinimapDisplay, environment: EnvironmentStyle, platform: MinimapPresenterPlatform = BROWSER_PLATFORM) {
    this.#display = display;
    this.#environment = environment;
    this.#platform = platform;
  }

  /** Forgets the shown map for another world and stops any full map still being made; finished maps stay retained. */
  reset(): void {
    this.#worker?.terminate();
    this.#worker = null;
    this.#scenery = null;
    this.#vectors = null;
    this.#coarseCenter = null;
    this.#refined = false;
    this.#display.show(null);
  }

  /** Called every frame with the player's position in metres. */
  update(scenery: Scenery, self: { x: number; z: number }): void {
    if (this.#refined) { return; }
    if (this.#scenery !== scenery) {
      this.#scenery = scenery;
      const retained = this.#retained.get(scenery.presentationFingerprint);
      if (retained) {
        this.#retained.delete(scenery.presentationFingerprint);
        this.#finish(scenery, retained);
        return;
      }
      this.#vectors = buildMinimapVectors(scenery, this.#environment);
      this.#showCoarse(scenery, self);
      this.#platform.whenIdle(() => this.#refine(scenery));
      return;
    }
    const center = this.#coarseCenter;
    if (center && Math.hypot(self.x - center.x, self.z - center.z) > COARSE_MARGIN_METRES) { this.#showCoarse(scenery, self); }
  }

  draw(self: { x: number; z: number }, heading: number, facing: number, units: readonly MinimapUnit[], disturbance: MinimapDisturbance | null): void {
    this.#display.draw(self, heading, facing, units, disturbance);
  }

  #showCoarse(scenery: Scenery, self: { x: number; z: number }): void {
    const frame = coarseMinimapFrame(self);
    const terrain = rasterizeMinimapTerrain(minimapTerrainSource(scenery, this.#environment), frame);
    this.#coarseCenter = { x: self.x, z: self.z };
    this.#display.show(this.#platform.paint({ ...frame, ...vectorsWithin(this.#vectors!, frame), terrain }));
  }

  #refine(scenery: Scenery): void {
    if (this.#scenery !== scenery || this.#refined || this.#worker) { return; }
    const vectors = this.#vectors!;
    const frame = fullMinimapFrame();
    const worker = this.#platform.createWorker();
    if (!worker) {
      this.#buildOnPage(scenery);
      return;
    }
    this.#worker = worker;
    worker.onmessage = ({ data }) => {
      worker.terminate();
      if (this.#worker !== worker) {
        if ("bitmap" in data) { data.bitmap.close(); }
        return;
      }
      this.#worker = null;
      this.#finish(scenery, "bitmap" in data ? { source: data.bitmap, frame } : this.#platform.paint({ ...frame, ...vectors, terrain: data.terrain }));
    };
    worker.onerror = () => {
      worker.terminate();
      if (this.#worker !== worker) { return; }
      this.#worker = null;
      this.#platform.whenIdle(() => this.#buildOnPage(scenery));
    };
    const source = minimapTerrainSource(scenery, this.#environment);
    const heights = Int32Array.from(scenery.terrain.heights);
    const biomes = Uint8Array.from(scenery.terrain.biomes);
    worker.postMessage({ source: { ...source, heights, biomes }, frame, vectors }, [heights.buffer, biomes.buffer]);
  }

  #buildOnPage(scenery: Scenery): void {
    if (this.#scenery === scenery && !this.#refined) {
      this.#finish(scenery, this.#platform.paint(buildMinimapLayers(scenery, this.#environment)));
    }
  }

  #finish(scenery: Scenery, image: MinimapImage): void {
    this.#refined = true;
    this.#coarseCenter = null;
    this.#vectors = null;
    this.#display.show(image);
    this.#retained.set(scenery.presentationFingerprint, image);
    while (this.#retained.size > RETAINED_MAPS) {
      const oldest = this.#retained.keys().next().value!;
      release(this.#retained.get(oldest));
      this.#retained.delete(oldest);
    }
  }
}
