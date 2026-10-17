import type { EntityState } from "../replication";
import { mixColor, shadeColor, type EnvironmentStyle } from "./environment";
import type { Color, Prop, Scenery, TerrainGrid } from "./scenery";
import { disposition as unitDisposition } from "./units/creature-bodies";

const OVERLAY_BIOMES: ReadonlySet<string> = new Set(["road", "lake bed", "shore", "plaza", "farmland"]);

/**
 * Biome IDs per sample with overlay biomes (roads, water, the plaza and the
 * field) replaced by the most common other biome within two samples, or by
 * the meadow where only overlay surrounds them.
 */
export function groundBiomes(grid: TerrainGrid, biomeNames: ReadonlyMap<number, string>): number[] {
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
    return best;
  });
}

/**
 * The circular minimap. `buildMinimapLayers` turns scenery into a top-down
 * map once at load (terrain colours with hill shading as pixels, plus
 * roads, water, buildings and trees as vectors); `Minimap` paints that image
 * every frame rotated so the camera heading points up (or north up), with
 * the player's arrow in the middle and other units as dots.
 */
export type MinimapLayers = {
  /** Half the side of the mapped square in metres, centred on the origin. */
  extent: number;
  pixelsPerMetre: number;
  width: number;
  height: number;
  /** RGBA rows from −Z (north, top) to +Z, each from −X to +X. */
  terrain: Uint8ClampedArray;
  roads: { points: (readonly [number, number])[]; width: number; color: Color }[];
  water: { center: readonly [number, number]; radii: readonly [number, number]; color: Color }[];
  buildings: { corners: (readonly [number, number])[]; color: Color }[];
  trees: { center: readonly [number, number]; radius: number; color: Color }[];
};

export const MINIMAP_EXTENT_METRES = 132;
const PIXELS_PER_METRE = 2;
/** Zoom levels: metres from the centre to the minimap's rim. */
export const MINIMAP_RADII_METRES = [30, 45, 70, 100] as const;

function channels(color: Color): [number, number, number] {
  const value = Number.parseInt(color.slice(1), 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

const BUILDING_KINDS = new Set<Prop["kind"]>([
  "keep", "inn", "house", "smithy", "barn", "farmhouse", "windmill", "well", "palisade", "gate-post", "tent",
  "mine-entrance", "waystone", "fence", "cliff",
]);

/** Deterministic map layers for a scenery value; pure, no DOM. */
export function buildMinimapLayers(scenery: Scenery, style: EnvironmentStyle): MinimapLayers {
  const { terrain, unitsPerMetre } = scenery;
  const extent = MINIMAP_EXTENT_METRES;
  const size = Math.round(extent * 2 * PIXELS_PER_METRE);
  const names = new Map(scenery.biomes.map((biome) => [biome.id, biome.name]));
  const ground = groundBiomes(terrain, names);
  const colorOf = new Map(scenery.biomes.map((biome) => [biome.id, channels(style.biomeColors[biome.name] ?? biome.color)]));
  const step = terrain.step / unitsPerMetre;
  const originX = terrain.originXz[0] / unitsPerMetre;
  const originZ = terrain.originXz[1] / unitsPerMetre;
  const sample = (column: number, row: number) => {
    const c = Math.min(terrain.columns - 1, Math.max(0, column));
    const r = Math.min(terrain.rows - 1, Math.max(0, row));
    const index = r * terrain.columns + c;
    return { color: colorOf.get(ground[index] ?? 0) ?? [128, 128, 128], height: (terrain.heights[index] ?? 0) / unitsPerMetre };
  };
  const pixels = new Uint8ClampedArray(size * size * 4);
  for (let py = 0; py < size; py += 1) {
    const z = -extent + (py + 0.5) / PIXELS_PER_METRE;
    const gz = (z - originZ) / step;
    const row = Math.floor(gz);
    const fz = gz - row;
    for (let px = 0; px < size; px += 1) {
      const x = -extent + (px + 0.5) / PIXELS_PER_METRE;
      const gx = (x - originX) / step;
      const column = Math.floor(gx);
      const fx = gx - column;
      const s00 = sample(column, row);
      const s10 = sample(column + 1, row);
      const s01 = sample(column, row + 1);
      const s11 = sample(column + 1, row + 1);
      // Hill shading from the height gradient, lit from the north-west.
      const dx = ((s10.height - s00.height) + (s11.height - s01.height)) / (2 * step);
      const dz = ((s01.height - s00.height) + (s11.height - s10.height)) / (2 * step);
      const shade = Math.min(1.25, Math.max(0.6, 1 + (dx + dz) * 0.9));
      const offset = (py * size + px) * 4;
      for (let channel = 0; channel < 3; channel += 1) {
        const top = s00.color[channel]! * (1 - fx) + s10.color[channel]! * fx;
        const bottom = s01.color[channel]! * (1 - fx) + s11.color[channel]! * fx;
        pixels[offset + channel] = (top * (1 - fz) + bottom * fz) * shade;
      }
      pixels[offset + 3] = 255;
    }
  }
  const metres = (units: number) => units / unitsPerMetre;
  const { palette } = style;
  const buildings: MinimapLayers["buildings"] = [];
  const trees: MinimapLayers["trees"] = [];
  for (const prop of scenery.props) {
    const x = metres(prop.position[0]);
    const z = metres(prop.position[2]);
    if (Math.abs(x) > extent + 10 || Math.abs(z) > extent + 10) {
      continue;
    }
    if (prop.kind === "tree-oak" || prop.kind === "tree-pine" || prop.kind === "tree-birch") {
      const color = prop.kind === "tree-pine" ? palette.pineNeedles : prop.kind === "tree-birch" ? palette.birchLeaves : palette.oakLeaves;
      trees.push({ center: [x, z], radius: 1.6 * (prop.collider ? 1 : prop.scale), color: shadeColor(color, 0.85) });
      continue;
    }
    if (!BUILDING_KINDS.has(prop.kind)) {
      continue;
    }
    const hx = metres(prop.halfExtents[0]);
    const hz = metres(prop.halfExtents[2]);
    const yaw = (prop.yaw / 65_536) * 2 * Math.PI;
    const cos = Math.cos(yaw);
    const sin = Math.sin(yaw);
    const corners = ([[-hx, -hz], [hx, -hz], [hx, hz], [-hx, hz]] as const)
      .map(([lx, lz]) => [x + lx * cos + lz * sin, z - lx * sin + lz * cos] as const);
    const color: Color = prop.kind === "cliff"
      ? palette.rockDark
      : prop.kind === "palisade" || prop.kind === "gate-post" || prop.kind === "fence" ? palette.woodDark
        : prop.kind === "barn" ? palette.barnRed : prop.kind === "keep" ? palette.stone : palette.roofRed;
    buildings.push({ corners, color });
  }
  return {
    extent,
    pixelsPerMetre: PIXELS_PER_METRE,
    width: size,
    height: size,
    terrain: pixels,
    roads: scenery.roads.map((road) => ({
      points: road.points.map(([x, z]) => [metres(x), metres(z)] as const),
      width: metres(road.halfWidth) * 2,
      color: mixColor(style.biomeColors.road ?? "#a4865c", "#e8d8b0", 0.25),
    })),
    water: scenery.water.map((water) => ({
      center: [metres(water.centerXz[0]), metres(water.centerXz[1])],
      radii: [metres(water.radiiXz[0]), metres(water.radiiXz[1])],
      color: mixColor(style.water.color, "#9fd0e0", 0.2),
    })),
    buildings,
    trees,
  };
}

/**
 * Screen offset in minimap pixels (x right, y down) of a world offset
 * `(dx, dz)` in metres. North (−Z) is up when `headingUp` is false;
 * otherwise the camera heading `heading` (radians, 0 toward +Z, increasing
 * toward +X) points up.
 */
export function minimapOffset(dx: number, dz: number, heading: number, headingUp: boolean, pixelsPerMetre: number): [number, number] {
  // North-up maps world X to screen right and world Z to screen down.
  if (!headingUp) {
    return [dx * pixelsPerMetre, dz * pixelsPerMetre];
  }
  const angle = heading - Math.PI;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return [(dx * cos - dz * sin) * pixelsPerMetre, (dx * sin + dz * cos) * pixelsPerMetre];
}

/** The screen rotation (radians, clockwise positive in canvas space) that `minimapOffset` applies. */
export function minimapRotation(heading: number, headingUp: boolean): number {
  return headingUp ? heading - Math.PI : 0;
}

/** Keeps a dot inside the rim: offsets beyond `radius` pixels move onto it. */
export function clampToRim(offset: readonly [number, number], radius: number): [number, number] {
  const length = Math.hypot(offset[0], offset[1]);
  if (length <= radius || length === 0) {
    return [offset[0], offset[1]];
  }
  return [(offset[0] / length) * radius, (offset[1] / length) * radius];
}

export type Disposition = "hostile" | "neutral" | "friendly" | "player";

export const DOT_COLORS: Readonly<Record<Disposition, Color>> = {
  hostile: "#e5483d",
  neutral: "#f0cf4a",
  friendly: "#5ccf6c",
  player: "#4ea8ff",
};

/**
 * The dot colour class of a projected unit: players apart, creatures and
 * NPCs by the disposition their placeholder bodies are coloured by
 * (hostile or neutral creatures by their flags, friendly NPCs).
 */
export function dispositionOf(entity: EntityState): Disposition {
  return entity.kind === "player" ? "player" : unitDisposition(entity);
}

export type MinimapElements = {
  root: HTMLElement;
  canvas: HTMLCanvasElement;
  zoomIn: HTMLButtonElement;
  zoomOut: HTMLButtonElement;
  northUp: HTMLButtonElement;
};

export type MinimapUnit = { x: number; z: number; disposition: Disposition };

/** Paints the pre-rendered map image once; vectors are drawn over the terrain pixels. */
export function paintMinimapImage(layers: MinimapLayers): HTMLCanvasElement {
  const image = document.createElement("canvas");
  image.width = layers.width;
  image.height = layers.height;
  const context = image.getContext("2d");
  if (!context) {
    return image;
  }
  context.putImageData(new ImageData(new Uint8ClampedArray(layers.terrain), layers.width, layers.height), 0, 0);
  const scale = layers.pixelsPerMetre;
  context.setTransform(scale, 0, 0, scale, layers.extent * scale, layers.extent * scale);
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
  return image;
}

/** The live minimap: a map image, a player arrow and unit dots in a circle. */
export class Minimap {
  readonly #elements: MinimapElements;
  readonly #image: HTMLCanvasElement;
  readonly #layers: MinimapLayers;
  #zoom = 1;
  #headingUp = true;

  constructor(elements: MinimapElements, layers: MinimapLayers) {
    this.#elements = elements;
    this.#layers = layers;
    this.#image = paintMinimapImage(layers);
    for (const button of [elements.zoomIn, elements.zoomOut, elements.northUp]) {
      // A mouse press would move keyboard focus onto the button, where the world ignores
      // movement keys and Space presses the button again. Keep focus where it was;
      // keyboard users still reach the buttons with Tab.
      button.addEventListener("mousedown", (event) => event.preventDefault());
    }
    elements.zoomIn.addEventListener("click", () => this.#setZoom(this.#zoom - 1));
    elements.zoomOut.addEventListener("click", () => this.#setZoom(this.#zoom + 1));
    elements.northUp.addEventListener("click", () => {
      this.#headingUp = !this.#headingUp;
      elements.northUp.setAttribute("aria-pressed", String(!this.#headingUp));
    });
    this.#setZoom(this.#zoom);
  }

  get radiusMetres(): number {
    return MINIMAP_RADII_METRES[this.#zoom] ?? MINIMAP_RADII_METRES[1];
  }

  #setZoom(zoom: number): void {
    this.#zoom = Math.min(MINIMAP_RADII_METRES.length - 1, Math.max(0, zoom));
    this.#elements.zoomIn.disabled = this.#zoom === 0;
    this.#elements.zoomOut.disabled = this.#zoom === MINIMAP_RADII_METRES.length - 1;
  }

  /** Draws the map around `self` (metres); `heading` is the camera yaw, `facing` the character's. */
  draw(self: { x: number; z: number }, heading: number, facing: number, units: readonly MinimapUnit[]): void {
    const { canvas } = this.#elements;
    const context = canvas.getContext("2d");
    if (!context) {
      return;
    }
    const ratio = Math.min(2, window.devicePixelRatio || 1);
    const cssSize = canvas.clientWidth || 180;
    const size = Math.round(cssSize * ratio);
    if (canvas.width !== size || canvas.height !== size) {
      canvas.width = size;
      canvas.height = size;
    }
    const radius = size / 2;
    const pixelsPerMetre = radius / this.radiusMetres;
    const rotation = minimapRotation(heading, this.#headingUp);
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, size, size);
    context.save();
    context.beginPath();
    context.arc(radius, radius, radius, 0, 2 * Math.PI);
    context.clip();
    context.fillStyle = "#2c3a2e";
    context.fillRect(0, 0, size, size);
    // Map image: scale metres to pixels, rotate about the player, then offset.
    context.translate(radius, radius);
    context.rotate(rotation);
    const imageScale = pixelsPerMetre / this.#layers.pixelsPerMetre;
    context.scale(imageScale, imageScale);
    context.imageSmoothingEnabled = true;
    context.drawImage(
      this.#image,
      (-this.#layers.extent - self.x) * this.#layers.pixelsPerMetre,
      (-this.#layers.extent - self.z) * this.#layers.pixelsPerMetre,
    );
    context.restore();
    // Units as dots, the player's arrow on top.
    for (const unit of units) {
      const [dx, dy] = clampToRim(minimapOffset(unit.x - self.x, unit.z - self.z, heading, this.#headingUp, pixelsPerMetre), radius - 5 * ratio);
      context.fillStyle = DOT_COLORS[unit.disposition];
      context.strokeStyle = "rgb(0 0 0 / 60%)";
      context.lineWidth = ratio;
      context.beginPath();
      context.arc(radius + dx, radius + dy, 3.2 * ratio, 0, 2 * Math.PI);
      context.fill();
      context.stroke();
    }
    const [ax, ay] = minimapOffset(Math.sin(facing), Math.cos(facing), heading, this.#headingUp, 1);
    const arrowAngle = Math.atan2(ay, ax);
    context.save();
    context.translate(radius, radius);
    context.rotate(arrowAngle);
    context.fillStyle = "#fff6d8";
    context.strokeStyle = "#3a2a14";
    context.lineWidth = 1.2 * ratio;
    context.beginPath();
    context.moveTo(9 * ratio, 0);
    context.lineTo(-6 * ratio, 5.5 * ratio);
    context.lineTo(-3 * ratio, 0);
    context.lineTo(-6 * ratio, -5.5 * ratio);
    context.closePath();
    context.fill();
    context.stroke();
    context.restore();
    // North marker on the rim.
    const [nx, ny] = clampToRim(minimapOffset(0, -1e6, heading, this.#headingUp, 1), radius - 9 * ratio);
    context.fillStyle = "#f3e6b8";
    context.font = `700 ${11 * ratio}px Georgia, serif`;
    context.textAlign = "center";
    context.textBaseline = "middle";
    context.fillText("N", radius + nx, radius + ny);
  }
}
