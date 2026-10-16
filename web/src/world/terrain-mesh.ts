import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { mixColor, shadeColor, type EnvironmentStyle } from "./environment";
import type { SceneBatcher } from "./mesh-batching";
import { MeshBuilder, hash01, type Vec3 } from "./mesh-builder";
import type { Color, Scenery, TerrainGrid } from "./scenery";

/** Shared relief grids with smooth normals and per-vertex biome/tone colours.
 * The renderer owns distance fog; terrain colours never bake in daytime haze.
 */
type Grid = TerrainGrid;

/** Lift of road surfaces above the rendered terrain, in metres. */
const ROAD_LIFT_METRES = 0.05;
const VERGE_LIFT_METRES = 0.03;
const VERGE_EXTRA_METRES = 0.45;
/** Far-ring vertices inside the terrain grid sink this far below it, so the fine grid covers the overlap. */
const FAR_OVERLAP_SINK_METRES = 4;

function sampleIndex(grid: Grid, column: number, row: number): number {
  return row * grid.columns + column;
}

function heightMetres(grid: Grid, unitsPerMetre: number, column: number, row: number): number {
  const c = Math.min(grid.columns - 1, Math.max(0, column));
  const r = Math.min(grid.rows - 1, Math.max(0, row));
  return (grid.heights[sampleIndex(grid, c, r)] ?? 0) / unitsPerMetre;
}

/** Smooth vertex normal from central differences of the height field. */
function gridNormal(grid: Grid, unitsPerMetre: number, column: number, row: number): Vec3 {
  const step = grid.step / unitsPerMetre;
  const dx = (heightMetres(grid, unitsPerMetre, column + 1, row) - heightMetres(grid, unitsPerMetre, column - 1, row)) / (2 * step);
  const dz = (heightMetres(grid, unitsPerMetre, column, row + 1) - heightMetres(grid, unitsPerMetre, column, row - 1)) / (2 * step);
  const length = Math.hypot(dx, 1, dz);
  return [-dx / length, 1 / length, -dz / length];
}

/**
 * The rendered terrain height in metres at `(x, z)` metres: the same
 * triangulation the mesh uses, so props and road surfaces sit on what is
 * drawn. Outside the grid it clamps to the edge.
 */
export function terrainSurfaceY(grid: Grid, unitsPerMetre: number, x: number, z: number): number {
  const step = grid.step / unitsPerMetre;
  const gx = (x - grid.originXz[0] / unitsPerMetre) / step;
  const gz = (z - grid.originXz[1] / unitsPerMetre) / step;
  const column = Math.min(grid.columns - 2, Math.max(0, Math.floor(gx)));
  const row = Math.min(grid.rows - 2, Math.max(0, Math.floor(gz)));
  const fx = Math.min(1, Math.max(0, gx - column));
  const fz = Math.min(1, Math.max(0, gz - row));
  const h = (c: number, r: number) => heightMetres(grid, unitsPerMetre, column + c, row + r);
  // Triangles (00, 01, 10) and (10, 01, 11), split along the 10–01 diagonal.
  if (fx + fz <= 1) {
    return h(0, 0) + (h(1, 0) - h(0, 0)) * fx + (h(0, 1) - h(0, 0)) * fz;
  }
  return h(1, 1) + (h(0, 1) - h(1, 1)) * (1 - fx) + (h(1, 0) - h(1, 1)) * (1 - fz);
}

/** Biomes drawn as smooth surfaces over the terrain instead of as grid cells. */
const OVERLAY_BIOMES: ReadonlySet<string> = new Set(["road", "lake bed", "shore", "plaza", "farmland"]);

/**
 * Biome IDs per sample with overlay biomes (roads, water, the plaza and the
 * field) replaced by the most common other biome within two samples, or by
 * the meadow where only overlay surrounds them.
 */
export function groundBiomes(grid: Grid, biomeNames: ReadonlyMap<number, string>): number[] {
  const overlay = new Set([...biomeNames.entries()].filter(([, name]) => OVERLAY_BIOMES.has(name)).map(([id]) => id));
  const meadow = [...biomeNames.entries()].find(([, name]) => name === "meadow")?.[0];
  return grid.biomes.map((biome, index) => {
    if (!overlay.has(biome)) {
      return biome;
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
        const neighbour = grid.biomes[sampleIndex(grid, c, r)]!;
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
    return best;
  });
}

/** Axis-aligned bounds (metres) of the samples of a named biome, if any. */
export function biomeBounds(grid: Grid, biomes: readonly { id: number; name: string }[], name: string, unitsPerMetre: number):
  { min: [number, number]; max: [number, number] } | null {
  const id = biomes.find((biome) => biome.name === name)?.id;
  let min: [number, number] | null = null;
  let max: [number, number] = [0, 0];
  grid.biomes.forEach((biome, index) => {
    if (biome !== id) {
      return;
    }
    const x = (grid.originXz[0] + (index % grid.columns) * grid.step) / unitsPerMetre;
    const z = (grid.originXz[1] + Math.floor(index / grid.columns) * grid.step) / unitsPerMetre;
    if (!min) {
      min = [x, z];
      max = [x, z];
    } else {
      min = [Math.min(min[0], x), Math.min(min[1], z)];
      max = [Math.max(max[0], x), Math.max(max[1], z)];
    }
  });
  return min ? { min, max } : null;
}

type TerrainOptions = {
  grid: Grid;
  unitsPerMetre: number;
  biomes: readonly number[];
  /** Colour at one biome/tone sample. */
  colorOf: (tone: number) => Color;
  /** Whether cell (column, row) is drawn. */
  includeCell: (column: number, row: number) => boolean;
  vertexY: (x: number, z: number, height: number) => number;
  mesh: MeshBuilder;
};

function appendGrid(options: TerrainOptions): void {
  const { grid, unitsPerMetre } = options;
  const metres = (units: number) => units / unitsPerMetre;
  const mesh = options.mesh;
  const map = new Map<number, number>();
  const vertex = (sample: number): number => {
    const existing = map.get(sample);
    if (existing !== undefined) {
      return existing;
    }
    const column = sample % grid.columns;
    const row = Math.floor(sample / grid.columns);
    const x = metres(grid.originXz[0] + column * grid.step);
    const z = metres(grid.originXz[1] + row * grid.step);
    const height = metres(grid.heights[sample] ?? 0);
    const index = mesh.positions.length;
    mesh.positions.push([x, options.vertexY(x, z, height), z]);
    mesh.normals.push(gridNormal(grid, unitsPerMetre, column, row));
    const tone = options.biomes[sample] ?? 0;
    const color = options.colorOf(tone);
    const rgb = Number.parseInt(color.slice(1), 16);
    mesh.colors.push([((rgb >> 16) & 255) / 255, ((rgb >> 8) & 255) / 255, (rgb & 255) / 255]);
    map.set(sample, index);
    return index;
  };
  for (let row = 0; row + 1 < grid.rows; row += 1) {
    for (let column = 0; column + 1 < grid.columns; column += 1) {
      if (!options.includeCell(column, row)) {
        continue;
      }
      const s00 = sampleIndex(grid, column, row);
      const s10 = s00 + 1;
      const s01 = s00 + grid.columns;
      const s11 = s01 + 1;
      // Counter-clockwise seen from above, so faces point up.
      mesh.indices.push(vertex(s00), vertex(s01), vertex(s10), vertex(s10), vertex(s01), vertex(s11));
    }
  }

}

/** Biomes whose ground varies in tone: grass variants, forest floor and the hollow's dirt. */
const TONED_BIOMES: ReadonlySet<string> = new Set(["meadow", "hub", "woods", "hollow", "foothills", "highland", "rock"]);
/** Tone keys pack a biome ID and a tone variant: `biome * TONES + variant`. */
const TONES = 4;
const PLAIN_TONE = 1;
/** Smooth seeded value noise in [0, 1) over `cell`-metre squares; presentation only. */
function valueNoise(x: number, z: number, cell: number, seed: number): number {
  const gx = x / cell;
  const gz = z / cell;
  const x0 = Math.floor(gx);
  const z0 = Math.floor(gz);
  const smooth = (t: number) => t * t * (3 - 2 * t);
  const fx = smooth(gx - x0);
  const fz = smooth(gz - z0);
  const at = (dx: number, dz: number) => hash01(seed, x0 + dx, z0 + dz);
  const near = at(0, 0) + (at(1, 0) - at(0, 0)) * fx;
  const far = at(0, 1) + (at(1, 1) - at(0, 1)) * fx;
  return near + (far - near) * fz;
}

/** The tone variant of ground at `(x, z)` metres: 0 shaded, 1 plain, 2 sunlit. */
export function toneVariant(x: number, z: number): number {
  const value = valueNoise(x, z, 22, 17) * 0.7 + valueNoise(x, z, 7, 29) * 0.3;
  return value < 0.4 ? 0 : value < 0.62 ? PLAIN_TONE : 2;
}

function toneColor(base: Color, variant: number): Color {
  switch (variant) {
    case 0: return shadeColor(base, 0.9);
    case 2: return mixColor(base, "#c2cf6e", 0.2);
    default: return base;
  }
}

/** Vertex colours interpolate biome boundaries without splitting the relief mesh. */
function biomeColor(scenery: Scenery, style: EnvironmentStyle): (tone: number) => Color {
  const byId = new Map(scenery.biomes.map((biome) => [biome.id, style.biomeColors[biome.name] ?? biome.color]));
  return (key) => toneColor(byId.get(Math.floor(key / TONES)) ?? "#808080", key % TONES);
}

/** Tone keys per sample: ground biome and, for toned biomes, a tone variant. */
function toneKeys(grid: Grid, biomes: readonly number[], names: ReadonlyMap<number, string>, unitsPerMetre: number): number[] {
  return biomes.map((biome, index) => {
    if (!TONED_BIOMES.has(names.get(biome) ?? "")) {
      return biome * TONES + PLAIN_TONE;
    }
    const x = (grid.originXz[0] + (index % grid.columns) * grid.step) / unitsPerMetre;
    const z = (grid.originXz[1] + Math.floor(index / grid.columns) * grid.step) / unitsPerMetre;
    return biome * TONES + toneVariant(x, z);
  });
}

/** Adds the terrain grid and far ring as two vertex-coloured scene-wide meshes. */
export function appendTerrain(batcher: SceneBatcher, scenery: Scenery, style: EnvironmentStyle): void {
  const names = new Map(scenery.biomes.map((biome) => [biome.id, biome.name]));
  const colorOf = biomeColor(scenery, style);
  const { terrain, farTerrain, unitsPerMetre } = scenery;
  appendGrid({
    grid: terrain,
    unitsPerMetre,
    biomes: toneKeys(terrain, groundBiomes(terrain, names), names, unitsPerMetre),
    colorOf,
    includeCell: () => true,
    vertexY: (_x, _z, height) => height,
    mesh: batcher.global("terrain", "#ffffff"),
  });
  // The far ring: every far cell that reaches the terrain grid's edge or beyond.
  const innerMetres = Math.min(
    -terrain.originXz[0], -terrain.originXz[1],
    terrain.originXz[0] + (terrain.columns - 1) * terrain.step,
    terrain.originXz[1] + (terrain.rows - 1) * terrain.step,
  ) / unitsPerMetre;
  const farMetres = (units: number) => units / unitsPerMetre;
  const reach = (column: number, row: number) => Math.max(
    Math.abs(farMetres(farTerrain.originXz[0] + column * farTerrain.step)),
    Math.abs(farMetres(farTerrain.originXz[1] + row * farTerrain.step)),
  );
  appendGrid({
    grid: farTerrain,
    unitsPerMetre,
    biomes: farTerrain.biomes.map((biome) => biome * TONES + PLAIN_TONE),
    colorOf,
    includeCell: (column, row) => Math.max(reach(column, row), reach(column + 1, row), reach(column, row + 1), reach(column + 1, row + 1)) >= innerMetres,
    vertexY: (x, z, height) => (Math.max(Math.abs(x), Math.abs(z)) < innerMetres ? height - FAR_OVERLAP_SINK_METRES : height),
    mesh: batcher.global("far", "#ffffff"),
  });
}

/** A flat ribbon along a polyline, hugging the rendered terrain, with round joints. */
function appendRibbon(mesh: MeshBuilder, points: readonly (readonly [number, number])[], halfWidth: number, surface: (x: number, z: number) => number): void {
  const up: Vec3 = [0, 1, 0];
  for (let index = 0; index + 1 < points.length; index += 1) {
    const [ax, az] = points[index]!;
    const [bx, bz] = points[index + 1]!;
    const length = Math.hypot(bx - ax, bz - az);
    if (length === 0) {
      continue;
    }
    // Left of the direction of travel, seen from above.
    const nx = -(bz - az) / length * halfWidth;
    const nz = (bx - ax) / length * halfWidth;
    const pieces = Math.max(1, Math.ceil(length / 2));
    const base = mesh.positions.length;
    for (let piece = 0; piece <= pieces; piece += 1) {
      const t = piece / pieces;
      const x = ax + (bx - ax) * t;
      const z = az + (bz - az) * t;
      mesh.positions.push([x + nx, surface(x + nx, z + nz), z + nz], [x - nx, surface(x - nx, z - nz), z - nz]);
      mesh.normals.push(up, up);
    }
    for (let piece = 0; piece < pieces; piece += 1) {
      const l0 = base + piece * 2;
      const r0 = l0 + 1;
      const l1 = l0 + 2;
      const r1 = l0 + 3;
      mesh.indices.push(l0, l1, r0, r0, l1, r1);
    }
  }
  for (const [x, z] of points) {
    const centre = mesh.positions.length;
    const segments = 12;
    mesh.positions.push([x, surface(x, z), z]);
    mesh.normals.push(up);
    for (let segment = 0; segment < segments; segment += 1) {
      const angle = (segment / segments) * 2 * Math.PI;
      const px = x + Math.sin(angle) * halfWidth;
      const pz = z + Math.cos(angle) * halfWidth;
      mesh.positions.push([px, surface(px, pz), pz]);
      mesh.normals.push(up);
    }
    for (let segment = 0; segment < segments; segment += 1) {
      mesh.indices.push(centre, centre + 1 + segment, centre + 1 + ((segment + 1) % segments));
    }
  }
}

/** Road surfaces: a packed-dirt core on a slightly wider, grassier verge. */
export function appendRoads(batcher: SceneBatcher, scenery: Scenery, style: EnvironmentStyle): void {
  const { unitsPerMetre, terrain } = scenery;
  const road = style.biomeColors.road ?? "#a4865c";
  const verge = mixColor(road, style.biomeColors.meadow ?? "#6a9a42", 0.45);
  const core = batcher.global("roads", road);
  const edge = batcher.global("roads", verge);
  for (const { points, halfWidth } of scenery.roads) {
    const metres = points.map(([x, z]) => [x / unitsPerMetre, z / unitsPerMetre] as const);
    const width = halfWidth / unitsPerMetre;
    appendRibbon(edge, metres, width + VERGE_EXTRA_METRES, (x, z) => terrainSurfaceY(terrain, unitsPerMetre, x, z) + VERGE_LIFT_METRES);
    appendRibbon(core, metres, width, (x, z) => terrainSurfaceY(terrain, unitsPerMetre, x, z) + ROAD_LIFT_METRES);
  }
}

function ellipse(mesh: MeshBuilder, center: readonly [number, number], radii: readonly [number, number], y: number, segments: number): void {
  const base = mesh.positions.length;
  mesh.positions.push([center[0], y, center[1]]);
  mesh.normals.push([0, 1, 0]);
  for (let segment = 0; segment < segments; segment += 1) {
    const angle = (segment / segments) * 2 * Math.PI;
    mesh.positions.push([center[0] + Math.sin(angle) * radii[0], y, center[1] + Math.cos(angle) * radii[1]]);
    mesh.normals.push([0, 1, 0]);
  }
  for (let segment = 0; segment < segments; segment += 1) {
    mesh.indices.push(base, base + 1 + segment, base + 1 + ((segment + 1) % segments));
  }
}

/** The lake bed: a sandy shore and rim darkening toward the middle, drawn just above the terrain. */
export function appendLakeBeds(batcher: SceneBatcher, scenery: Scenery, style: EnvironmentStyle): void {
  const metres = (units: number) => units / scenery.unitsPerMetre;
  const { bedColor, shoreColor } = style.water;
  for (const water of scenery.water) {
    // The shore biome reaches 3 m beyond the water; relief is flat there.
    const mesh = batcher.global("lake-bed", shoreColor);
    ellipse(mesh, [metres(water.centerXz[0]), metres(water.centerXz[1])], [metres(water.radiiXz[0]) + 3, metres(water.radiiXz[1]) + 3], 0.006, 64);
  }
  const rings: [number, Color, number][] = [
    [1.02, mixColor(shoreColor, bedColor, 0.35), 0.012],
    [0.9, mixColor(shoreColor, bedColor, 0.7), 0.02],
    [0.72, bedColor, 0.028],
    [0.45, shadeColor(bedColor, 0.85), 0.036],
  ];
  for (const water of scenery.water) {
    const center = [metres(water.centerXz[0]), metres(water.centerXz[1])] as const;
    for (const [factor, color, lift] of rings) {
      const mesh = batcher.global("lake-bed", color);
      ellipse(mesh, center, [metres(water.radiiXz[0]) * factor, metres(water.radiiXz[1]) * factor], lift, 48);
    }
  }
}

/** One translucent surface node per water body, slightly above its bed. */
export function waterNodes(scenery: Scenery, style: EnvironmentStyle, resourcePrefix: string): RendererSceneNode[] {
  const metres = (units: number) => units / scenery.unitsPerMetre;
  return scenery.water.map((water, index) => {
    const mesh = new MeshBuilder();
    ellipse(mesh, [metres(water.centerXz[0]), metres(water.centerXz[1])], [metres(water.radiiXz[0]), metres(water.radiiXz[1])], metres(water.surfaceY), 64);
    return {
      id: `water-${index}`,
      geometry: { kind: "mesh", resourceKey: `${resourcePrefix}:water:${index}`, ...mesh.data() },
      color: style.water.color,
      opacity: style.water.opacity,
      transform: { translation: [0, 0, 0] },
    };
  });
}

/** A flat-topped patch over a rectangle (metres) that follows the rendered terrain. */
function patch(mesh: MeshBuilder, min: readonly [number, number], max: readonly [number, number], lift: number, surface: (x: number, z: number) => number): void {
  const columns = Math.max(1, Math.ceil((max[0] - min[0]) / 2));
  const rows = Math.max(1, Math.ceil((max[1] - min[1]) / 2));
  const base = mesh.positions.length;
  for (let row = 0; row <= rows; row += 1) {
    for (let column = 0; column <= columns; column += 1) {
      const x = min[0] + ((max[0] - min[0]) * column) / columns;
      const z = min[1] + ((max[1] - min[1]) * row) / rows;
      mesh.positions.push([x, surface(x, z) + lift, z]);
      mesh.normals.push([0, 1, 0]);
    }
  }
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const i00 = base + row * (columns + 1) + column;
      const i10 = i00 + 1;
      const i01 = i00 + columns + 1;
      const i11 = i01 + 1;
      mesh.indices.push(i00, i01, i10, i10, i01, i11);
    }
  }
}

/** The hub plaza and the ploughed field as smooth patches with a darker border. */
export function appendPatches(batcher: SceneBatcher, scenery: Scenery, style: EnvironmentStyle): void {
  const { terrain, unitsPerMetre } = scenery;
  const surface = (x: number, z: number) => terrainSurfaceY(terrain, unitsPerMetre, x, z);
  for (const [name, border] of [["plaza", 0.6], ["farmland", 0.8]] as const) {
    const bounds = biomeBounds(terrain, scenery.biomes, name, unitsPerMetre);
    const color = style.biomeColors[name] ?? scenery.biomes.find((biome) => biome.name === name)?.color;
    if (!bounds || !color) {
      continue;
    }
    const edge = shadeColor(color, 0.84);
    patch(batcher.global("patches", edge), [bounds.min[0] - border, bounds.min[1] - border], [bounds.max[0] + border, bounds.max[1] + border], 0.025, surface);
    patch(batcher.global("patches", color), bounds.min, bounds.max, 0.04, surface);
  }
}
