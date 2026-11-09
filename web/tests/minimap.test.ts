import { describe, expect, test } from "bun:test";
import { ENVIRONMENT } from "../src/world/environment";
import {
  Minimap, buildMinimapLayers, buildMinimapVectors, clampToRim, coarseMinimapFrame, dispositionOf, fullMinimapFrame, minimapOffset, minimapRotation,
  minimapTerrainSource, type MinimapElements, type MinimapImage,
} from "../src/world/minimap";
import { MinimapPresenter, type MinimapPresenterPlatform, type MinimapWorker } from "../src/world/minimap-presenter";
import { minimapFrame, rasterizeMinimapTerrain, runMinimapRaster, type MinimapRasterRequest, type MinimapRasterResult } from "../src/world/minimap-raster";
import type { Scenery } from "../src/world/scenery";
import type { EntityState } from "../src/replication";
import { fixtureScenery } from "./support/scenery-fixture";
import { NO_FLAGS, playerEntity } from "./support/snapshots";

/** The rasterization before #527 moved it: the map must keep these exact bytes. */
function previousTerrainRaster(scenery: Scenery): Uint8ClampedArray {
  const { terrain, unitsPerMetre } = scenery;
  const overlayNames = new Set(["road", "lake bed", "shore", "plaza", "farmland"]);
  const names = new Map(scenery.biomes.map((biome) => [biome.id, biome.name]));
  const overlay = new Set([...names.entries()].filter(([, name]) => overlayNames.has(name)).map(([id]) => id));
  const meadow = [...names.entries()].find(([, name]) => name === "meadow")?.[0];
  const ground = terrain.biomes.map((biome, index) => {
    if (!overlay.has(biome)) { return biome; }
    const column = index % terrain.columns;
    const row = Math.floor(index / terrain.columns);
    const counts = new Map<number, number>();
    for (let dr = -2; dr <= 2; dr += 1) {
      for (let dc = -2; dc <= 2; dc += 1) {
        const c = column + dc;
        const r = row + dr;
        if (c < 0 || r < 0 || c >= terrain.columns || r >= terrain.rows) { continue; }
        const neighbour = terrain.biomes[r * terrain.columns + c]!;
        if (!overlay.has(neighbour)) { counts.set(neighbour, (counts.get(neighbour) ?? 0) + 1); }
      }
    }
    let best = meadow ?? biome;
    let bestCount = 0;
    for (const [candidate, count] of [...counts.entries()].sort(([a], [b]) => a - b)) {
      if (count > bestCount) { best = candidate; bestCount = count; }
    }
    return best;
  });
  const channels = (color: string) => { const value = Number.parseInt(color.slice(1), 16); return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff]; };
  const colorOf = new Map(scenery.biomes.map((biome) => [biome.id, channels(ENVIRONMENT.biomeColors[biome.name] ?? biome.color)]));
  const { min, width, height, pixelsPerMetre } = fullMinimapFrame();
  const step = terrain.step / unitsPerMetre;
  const originX = terrain.originXz[0] / unitsPerMetre;
  const originZ = terrain.originXz[1] / unitsPerMetre;
  const sample = (column: number, row: number) => {
    const c = Math.min(terrain.columns - 1, Math.max(0, column));
    const r = Math.min(terrain.rows - 1, Math.max(0, row));
    const index = r * terrain.columns + c;
    return { color: colorOf.get(ground[index] ?? 0) ?? [128, 128, 128], height: (terrain.heights[index] ?? 0) / unitsPerMetre };
  };
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let py = 0; py < height; py += 1) {
    const gz = (min[1] + (py + 0.5) / pixelsPerMetre - originZ) / step;
    const row = Math.floor(gz);
    const fz = gz - row;
    for (let px = 0; px < width; px += 1) {
      const gx = (min[0] + (px + 0.5) / pixelsPerMetre - originX) / step;
      const column = Math.floor(gx);
      const fx = gx - column;
      const s00 = sample(column, row);
      const s10 = sample(column + 1, row);
      const s01 = sample(column, row + 1);
      const s11 = sample(column + 1, row + 1);
      const dx = ((s10.height - s00.height) + (s11.height - s01.height)) / (2 * step);
      const dz = ((s01.height - s00.height) + (s11.height - s10.height)) / (2 * step);
      const shade = Math.min(1.25, Math.max(0.6, 1 + (dx + dz) * 0.9));
      const offset = (py * width + px) * 4;
      for (let channel = 0; channel < 3; channel += 1) {
        const top = s00.color[channel]! * (1 - fx) + s10.color[channel]! * fx;
        const bottom = s01.color[channel]! * (1 - fx) + s11.color[channel]! * fx;
        pixels[offset + channel] = (top * (1 - fz) + bottom * fz) * shade;
      }
      pixels[offset + 3] = 255;
    }
  }
  return pixels;
}

class FakeWorker implements MinimapWorker {
  onmessage: MinimapWorker["onmessage"] = null;
  onerror: MinimapWorker["onerror"] = null;
  posted: { message: MinimapRasterRequest; transfer: Transferable[] }[] = [];
  terminated = false;
  postMessage(message: MinimapRasterRequest, transfer: Transferable[]): void { this.posted.push({ message, transfer }); }
  terminate(): void { this.terminated = true; }
  answer(result: MinimapRasterResult): void { this.onmessage?.({ data: result } as MessageEvent<MinimapRasterResult>); }
}

function presenterHarness(createWorker: () => MinimapWorker | null) {
  const shown: (MinimapImage | null)[] = [];
  const painted: number[] = [];
  const idle: (() => void)[] = [];
  const platform: MinimapPresenterPlatform = {
    createWorker,
    paint: ({ min, max, pixelsPerMetre, width, height }) => { painted.push(pixelsPerMetre); return { source: {} as HTMLCanvasElement, frame: { min, max, pixelsPerMetre, width, height } }; },
    whenIdle: (run) => { idle.push(run); },
  };
  const presenter = new MinimapPresenter({ show: (image) => shown.push(image), draw: () => undefined }, ENVIRONMENT, platform);
  return { presenter, shown, painted, idle };
}

const bitmap = () => ({ closed: false, close() { this.closed = true; } }) as unknown as ImageBitmap & { closed: boolean };

describe("minimap projection", () => {
  test("north-up maps north (−Z) up and east (+X) right", () => {
    expect(minimapOffset(0, -10, 1.2, false, 2)).toEqual([0, -20]);
    expect(minimapOffset(5, 0, 1.2, false, 2)).toEqual([10, 0]);
    expect(minimapRotation(1.2, false)).toBe(0);
  });

  test("heading-up puts the camera heading up and the view's right on the right", () => {
    for (const heading of [0, 0.7, Math.PI / 2, 2.5, Math.PI, 5.9]) {
      const [ux, uy] = minimapOffset(Math.sin(heading), Math.cos(heading), heading, true, 1);
      expect(ux).toBeCloseTo(0, 9);
      expect(uy).toBeCloseTo(-1, 9);
      // The view's right is heading − 90°: (−cos, sin).
      const [rx, ry] = minimapOffset(-Math.cos(heading), Math.sin(heading), heading, true, 1);
      expect(rx).toBeCloseTo(1, 9);
      expect(ry).toBeCloseTo(0, 9);
    }
    // The canvas rotation applies the same mapping.
    const heading = 0.9;
    const angle = minimapRotation(heading, true);
    const [x, y] = [3, -4];
    const rotated = [x * Math.cos(angle) - y * Math.sin(angle), x * Math.sin(angle) + y * Math.cos(angle)];
    const direct = minimapOffset(x, y, heading, true, 1);
    expect(rotated[0]).toBeCloseTo(direct[0], 9);
    expect(rotated[1]).toBeCloseTo(direct[1], 9);
  });

  test("dots beyond the rim move onto it, others stay", () => {
    expect(clampToRim([3, 4], 10)).toEqual([3, 4]);
    const [x, y] = clampToRim([30, 40], 10);
    expect(x).toBeCloseTo(6, 9);
    expect(y).toBeCloseTo(8, 9);
  });

  test("dots take the unit's disposition: players, hostile or neutral creatures, friendly NPCs", () => {
    const creature = (hostile: boolean): EntityState => ({ ...playerEntity(9, [0, 45, 0]), kind: "creature", appearance: 1, flags: { ...NO_FLAGS, hostile } });
    expect(dispositionOf(playerEntity(2, [0, 90, 0]))).toBe("player");
    expect(dispositionOf(creature(true))).toBe("hostile");
    expect(dispositionOf(creature(false))).toBe("neutral");
    expect(dispositionOf({ ...playerEntity(6, [0, 90, 0]), kind: "npc", appearance: 6 })).toBe("friendly");
  });
});

describe("minimap layers", () => {
  test("are built once from scenery, deterministically, as opaque pixels plus vectors", () => {
    const scenery = fixtureScenery();
    const layers = buildMinimapLayers(scenery, ENVIRONMENT);
    // The Vale, the farmland south of it (#424), the highlands north of it (#425) and the wetlands east of it (#426), 2 px per metre.
    expect([layers.width, layers.height]).toEqual([1_696, 2_560]);
    expect(layers.terrain.length).toBe(layers.width * layers.height * 4);
    for (let offset = 3; offset < layers.terrain.length; offset += 4 * 97) {
      expect(layers.terrain[offset]).toBe(255);
    }
    expect(layers.roads).toHaveLength(1);
    expect(layers.roads[0]!.width).toBeCloseTo(3, 9);
    expect(layers.water[0]!.center).toEqual([6, -12]);
    expect(layers.trees.length).toBe(3);
    expect(layers.buildings.length).toBeGreaterThan(5);
    expect(buildMinimapLayers(scenery, ENVIRONMENT)).toEqual(layers);
  });

  test("the full raster keeps the exact bytes of the rasterization before #527", () => {
    const scenery = fixtureScenery();
    expect(Buffer.compare(buildMinimapLayers(scenery, ENVIRONMENT).terrain, previousTerrainRaster(scenery))).toBe(0);
  });

  test("a window on the same pixel grid is exactly the matching crop of the full map", () => {
    const scenery = fixtureScenery();
    const source = minimapTerrainSource(scenery, ENVIRONMENT);
    const full = fullMinimapFrame();
    const fullPixels = rasterizeMinimapTerrain(source, full);
    const window = minimapFrame([-20, -30], [30, 15], full.pixelsPerMetre);
    const pixels = rasterizeMinimapTerrain(source, window);
    const startX = (window.min[0] - full.min[0]) * full.pixelsPerMetre;
    const startY = (window.min[1] - full.min[1]) * full.pixelsPerMetre;
    for (let row = 0; row < window.height; row += 1) {
      const from = (startY + row) * full.width * 4 + startX * 4;
      const line = pixels.subarray(row * window.width * 4, (row + 1) * window.width * 4);
      expect(Buffer.compare(line, fullPixels.subarray(from, from + window.width * 4))).toBe(0);
    }
  });

  test("the worker's job hands the terrain back without a copy when it cannot paint", () => {
    const scenery = fixtureScenery();
    const frame = coarseMinimapFrame({ x: 0, z: 0 });
    const { result, transfer } = runMinimapRaster({ source: minimapTerrainSource(scenery, ENVIRONMENT), frame, vectors: buildMinimapVectors(scenery, ENVIRONMENT) });
    expect("terrain" in result).toBe(true);
    if ("terrain" in result) {
      expect(transfer).toEqual([result.terrain.buffer]);
      expect(result.terrain.length).toBe(frame.width * frame.height * 4);
    }
  });
});

describe("minimap image", () => {
  test("coarse and full images land at the same world place under the player", () => {
    const calls: number[][] = [];
    const context = {
      setTransform() {}, clearRect() {}, save() {}, beginPath() {}, arc() {}, clip() {}, fillRect() {}, translate() {}, rotate() {}, restore() {},
      moveTo() {}, lineTo() {}, closePath() {}, fill() {}, stroke() {}, fillText() {},
      scale: (x: number) => calls.push([x]),
      drawImage: (_source: unknown, x: number, y: number) => calls[calls.length - 1]!.push(x, y),
    };
    const canvas = { clientWidth: 180, width: 0, height: 0, getContext: () => context } as unknown as HTMLCanvasElement;
    const button = () => ({ addEventListener() {}, setAttribute() {}, disabled: false }) as unknown as HTMLButtonElement;
    const elements = { root: {}, canvas, zoomIn: button(), zoomOut: button(), northUp: button() } as unknown as MinimapElements;
    (globalThis as unknown as { window: { devicePixelRatio: number } }).window = { devicePixelRatio: 1 };
    const minimap = new Minimap(elements);
    const self = { x: 10, z: -20 };
    const coarse = coarseMinimapFrame(self);
    const full = fullMinimapFrame();
    minimap.show({ source: {} as HTMLCanvasElement, frame: coarse });
    minimap.draw(self, 0, 0, []);
    minimap.show({ source: {} as HTMLCanvasElement, frame: full });
    minimap.draw(self, 0, 0, []);
    const worldUnderPlayer = ([scale, x, y]: number[], { min, pixelsPerMetre }: typeof coarse) => {
      expect(scale).toBeCloseTo((90 / 45) / pixelsPerMetre, 9);
      return [min[0] - x! / pixelsPerMetre, min[1] - y! / pixelsPerMetre];
    };
    expect(worldUnderPlayer(calls[0]!, coarse)).toEqual([self.x, self.z]);
    expect(worldUnderPlayer(calls[1]!, full)).toEqual([self.x, self.z]);
  });
});

describe("minimap presenter", () => {
  test("shows the coarse local map at once, then the worker's full map replaces it", () => {
    const worker = new FakeWorker();
    const { presenter, shown, painted, idle } = presenterHarness(() => worker);
    const scenery = fixtureScenery();
    presenter.update(scenery, { x: 3, z: 4 });
    expect(shown).toHaveLength(1);
    expect(shown[0]!.frame).toEqual(coarseMinimapFrame({ x: 3, z: 4 }));
    expect(painted).toEqual([1]);
    expect(worker.posted).toHaveLength(0); // The full map waits for idle time.
    idle.shift()!();
    expect(worker.posted).toHaveLength(1);
    const { message, transfer } = worker.posted[0]!;
    expect(message.frame).toEqual(fullMinimapFrame());
    expect(transfer).toEqual([(message.source.heights as Int32Array).buffer, (message.source.biomes as Uint8Array).buffer]);
    const full = bitmap();
    worker.answer({ bitmap: full });
    expect(shown).toHaveLength(2);
    expect(shown[1]!.source).toBe(full);
    expect(shown[1]!.frame).toEqual(fullMinimapFrame());
    expect(worker.terminated).toBe(true);
    presenter.update(scenery, { x: 500, z: 500 });
    expect(shown).toHaveLength(2); // Refined: no more coarse maps.
  });

  test("the coarse map follows the player only while the full map is pending", () => {
    const { presenter, shown } = presenterHarness(() => new FakeWorker());
    const scenery = fixtureScenery();
    presenter.update(scenery, { x: 0, z: 0 });
    presenter.update(scenery, { x: 30, z: 0 });
    expect(shown).toHaveLength(1);
    presenter.update(scenery, { x: 41, z: 0 });
    expect(shown).toHaveLength(2);
    expect(shown[1]!.frame).toEqual(coarseMinimapFrame({ x: 41, z: 0 }));
  });

  test("without a worker the page builds the full map from idle time", () => {
    const { presenter, shown, painted, idle } = presenterHarness(() => null);
    presenter.update(fixtureScenery(), { x: 0, z: 0 });
    idle.shift()!();
    expect(painted).toEqual([1, 2]);
    expect(shown[1]!.frame.width).toBe(1_696);
  });

  test("a failing worker falls back to the page, and a world change discards a late bitmap", () => {
    const failing = new FakeWorker();
    const fallback = presenterHarness(() => failing);
    fallback.presenter.update(fixtureScenery(), { x: 0, z: 0 });
    fallback.idle.shift()!();
    failing.onerror?.({} as ErrorEvent);
    expect(failing.terminated).toBe(true);
    fallback.idle.shift()!();
    expect(fallback.painted).toEqual([1, 2]);

    const worker = new FakeWorker();
    const { presenter, shown, idle } = presenterHarness(() => worker);
    presenter.update(fixtureScenery(), { x: 0, z: 0 });
    idle.shift()!();
    presenter.reset();
    expect(shown.at(-1)).toBeNull();
    expect(worker.terminated).toBe(true);
    const late = bitmap();
    worker.answer({ bitmap: late });
    expect(late.closed).toBe(true);
    expect(shown.at(-1)).toBeNull();
  });

  test("re-entering a world reuses its finished map without rasterizing again", () => {
    const worker = new FakeWorker();
    let created = 0;
    const { presenter, shown, painted, idle } = presenterHarness(() => { created += 1; return worker; });
    const scenery = fixtureScenery();
    presenter.update(scenery, { x: 0, z: 0 });
    idle.shift()!();
    const full = bitmap();
    worker.answer({ bitmap: full });
    presenter.reset();
    presenter.update(scenery, { x: 0, z: 0 });
    expect(created).toBe(1);
    expect(painted).toEqual([1]);
    expect(shown.at(-1)!.source).toBe(full);
    expect(full.closed).toBe(false);
  });

  test("keeps at most two finished maps and releases the oldest", () => {
    const worker = new FakeWorker();
    const { presenter, idle } = presenterHarness(() => worker);
    const enter = (fingerprint: string) => {
      presenter.reset();
      presenter.update(fixtureScenery({ presentationFingerprint: fingerprint }), { x: 0, z: 0 });
      idle.shift()!();
      const image = bitmap();
      worker.answer({ bitmap: image });
      return image;
    };
    const first = enter("00000000000000aa");
    enter("00000000000000bb");
    enter("00000000000000cc");
    expect(first.closed).toBe(true);
  });
});
