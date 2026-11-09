import type { Color } from "./scenery";

/**
 * The minimap's terrain rasterizer and painter, free of the DOM and of anything else a worker cannot import: the coarse
 * local map runs it on the main thread, the full map in `minimap-raster.worker.ts` (#527).
 */

/** The world rectangle an image covers and its size in pixels. */
export type MinimapFrame = {
  /** North-west (min X, min Z) and south-east corners in metres. */
  min: readonly [number, number];
  max: readonly [number, number];
  pixelsPerMetre: number;
  width: number;
  height: number;
};

/** The map's vector features, in metres. */
export type MinimapVectors = {
  roads: { points: (readonly [number, number])[]; width: number; color: Color }[];
  water: { center: readonly [number, number]; radii: readonly [number, number]; color: Color }[];
  buildings: { corners: (readonly [number, number])[]; color: Color }[];
  trees: { center: readonly [number, number]; radius: number; color: Color }[];
};

export type MinimapLayers = MinimapFrame & MinimapVectors & {
  /** RGBA rows from −Z (north, top) to +Z, each from −X to +X. */
  terrain: Uint8ClampedArray<ArrayBuffer>;
};

/** The terrain grid in the form the rasterizer reads; typed arrays cross to a worker without copying. */
export type MinimapTerrainSource = {
  originXz: readonly [number, number];
  step: number;
  columns: number;
  rows: number;
  heights: ArrayLike<number>;
  biomes: ArrayLike<number>;
  unitsPerMetre: number;
  biomeNames: ReadonlyMap<number, string>;
  biomeColors: ReadonlyMap<number, readonly [number, number, number]>;
};

export function minimapFrame(min: readonly [number, number], max: readonly [number, number], pixelsPerMetre: number): MinimapFrame {
  return {
    min, max, pixelsPerMetre,
    width: Math.round((max[0] - min[0]) * pixelsPerMetre),
    height: Math.round((max[1] - min[1]) * pixelsPerMetre),
  };
}

const OVERLAY_BIOMES: ReadonlySet<string> = new Set(["road", "lake bed", "shore", "plaza", "farmland"]);

/**
 * The biome a terrain sample shows on the map: overlay biomes (roads, water, the plaza and the field) replaced by the
 * most common other biome within two samples, or by the meadow where only overlay surrounds them. Each sample is
 * resolved once and only when asked for, so a local window never pays for the whole grid.
 */
export function groundBiomeLookup(
  grid: { columns: number; rows: number; biomes: ArrayLike<number> },
  biomeNames: ReadonlyMap<number, string>,
): (index: number) => number {
  const overlay = new Set([...biomeNames.entries()].filter(([, name]) => OVERLAY_BIOMES.has(name)).map(([id]) => id));
  const meadow = [...biomeNames.entries()].find(([, name]) => name === "meadow")?.[0];
  const resolved = new Int16Array(grid.biomes.length).fill(-1);
  return (index) => {
    const biome = grid.biomes[index]!;
    if (!overlay.has(biome)) {
      return biome;
    }
    const known = resolved[index]!;
    if (known >= 0) {
      return known;
    }
    const column = index % grid.columns;
    const row = Math.floor(index / grid.columns);
    const counts = new Map<number, number>();
    for (let dr = -2; dr <= 2; dr += 1) {
      for (let dc = -2; dc <= 2; dc += 1) {
        const c = column + dc;
        const r = row + dr;
        if (c < 0 || r < 0 || c >= grid.columns || r >= grid.rows) {
          continue;
        }
        const neighbour = grid.biomes[r * grid.columns + c]!;
        if (!overlay.has(neighbour)) {
          counts.set(neighbour, (counts.get(neighbour) ?? 0) + 1);
        }
      }
    }
    let best = meadow ?? biome;
    let bestCount = 0;
    for (const [candidate, count] of [...counts.entries()].sort(([a], [b]) => a - b)) {
      if (count > bestCount) {
        best = candidate;
        bestCount = count;
      }
    }
    resolved[index] = best;
    return best;
  };
}

const UNKNOWN_BIOME_COLOR = [128, 128, 128] as const;

/** Terrain colours with hill shading for `frame`; deterministic and pure. */
export function rasterizeMinimapTerrain(source: MinimapTerrainSource, frame: MinimapFrame): Uint8ClampedArray<ArrayBuffer> {
  const { columns, rows, heights, biomeColors, unitsPerMetre } = source;
  const { min, width, height, pixelsPerMetre } = frame;
  const ground = groundBiomeLookup(source, source.biomeNames);
  const step = source.step / unitsPerMetre;
  const originX = source.originXz[0] / unitsPerMetre;
  const originZ = source.originXz[1] / unitsPerMetre;
  const indexOf = (column: number, row: number) => Math.min(rows - 1, Math.max(0, row)) * columns + Math.min(columns - 1, Math.max(0, column));
  const colorAt = (index: number) => biomeColors.get(ground(index)) ?? UNKNOWN_BIOME_COLOR;
  const heightAt = (index: number) => (heights[index] ?? 0) / unitsPerMetre;
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let py = 0; py < height; py += 1) {
    const z = min[1] + (py + 0.5) / pixelsPerMetre;
    const gz = (z - originZ) / step;
    const row = Math.floor(gz);
    const fz = gz - row;
    for (let px = 0; px < width; px += 1) {
      const x = min[0] + (px + 0.5) / pixelsPerMetre;
      const gx = (x - originX) / step;
      const column = Math.floor(gx);
      const fx = gx - column;
      const i00 = indexOf(column, row);
      const i10 = indexOf(column + 1, row);
      const i01 = indexOf(column, row + 1);
      const i11 = indexOf(column + 1, row + 1);
      const h00 = heightAt(i00);
      const h10 = heightAt(i10);
      const h01 = heightAt(i01);
      const h11 = heightAt(i11);
      const c00 = colorAt(i00);
      const c10 = colorAt(i10);
      const c01 = colorAt(i01);
      const c11 = colorAt(i11);
      // Hill shading from the height gradient, lit from the north-west.
      const dx = ((h10 - h00) + (h11 - h01)) / (2 * step);
      const dz = ((h01 - h00) + (h11 - h10)) / (2 * step);
      const shade = Math.min(1.25, Math.max(0.6, 1 + (dx + dz) * 0.9));
      const offset = (py * width + px) * 4;
      for (let channel = 0; channel < 3; channel += 1) {
        const top = c00[channel]! * (1 - fx) + c10[channel]! * fx;
        const bottom = c01[channel]! * (1 - fx) + c11[channel]! * fx;
        pixels[offset + channel] = (top * (1 - fz) + bottom * fz) * shade;
      }
      pixels[offset + 3] = 255;
    }
  }
  return pixels;
}

/** Paints terrain pixels with the vectors drawn over them. */
export function paintMinimapLayers(context: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D, layers: MinimapLayers): void {
  // The array becomes the image's storage: nothing here copies it before `putImageData` does.
  context.putImageData(new ImageData(layers.terrain, layers.width, layers.height), 0, 0);
  const scale = layers.pixelsPerMetre;
  context.setTransform(scale, 0, 0, scale, -layers.min[0] * scale, -layers.min[1] * scale);
  for (const water of layers.water) {
    context.fillStyle = water.color;
    context.beginPath();
    context.ellipse(water.center[0], water.center[1], water.radii[0], water.radii[1], 0, 0, 2 * Math.PI);
    context.fill();
  }
  context.lineCap = "round";
  context.lineJoin = "round";
  for (const road of layers.roads) {
    context.strokeStyle = road.color;
    context.lineWidth = road.width * 1.3;
    context.beginPath();
    road.points.forEach(([x, z], index) => (index === 0 ? context.moveTo(x, z) : context.lineTo(x, z)));
    context.stroke();
  }
  for (const tree of layers.trees) {
    context.fillStyle = tree.color;
    context.beginPath();
    context.arc(tree.center[0], tree.center[1], tree.radius, 0, 2 * Math.PI);
    context.fill();
  }
  context.lineWidth = 0.35;
  context.strokeStyle = "rgb(30 22 16 / 70%)";
  for (const building of layers.buildings) {
    context.fillStyle = building.color;
    context.beginPath();
    building.corners.forEach(([x, z], index) => (index === 0 ? context.moveTo(x, z) : context.lineTo(x, z)));
    context.closePath();
    context.fill();
    context.stroke();
  }
}

export type MinimapRasterRequest = { source: MinimapTerrainSource; frame: MinimapFrame; vectors: MinimapVectors };

/** A finished map: a bitmap where the worker could paint it, otherwise the terrain for the page to paint. */
export type MinimapRasterResult = { bitmap: ImageBitmap } | { terrain: Uint8ClampedArray<ArrayBuffer> };

/** The worker's job; the result's buffers are listed in `transfer` so none of them is copied back. */
export function runMinimapRaster({ source, frame, vectors }: MinimapRasterRequest): { result: MinimapRasterResult; transfer: Transferable[] } {
  const terrain = rasterizeMinimapTerrain(source, frame);
  if (typeof OffscreenCanvas === "function") {
    const canvas = new OffscreenCanvas(frame.width, frame.height);
    const context = canvas.getContext("2d");
    if (context) {
      paintMinimapLayers(context, { ...frame, ...vectors, terrain });
      const bitmap = canvas.transferToImageBitmap();
      return { result: { bitmap }, transfer: [bitmap] };
    }
  }
  return { result: { terrain }, transfer: [terrain.buffer] };
}
