import type { EntityState } from "../replication";
import { mixColor, shadeColor, type EnvironmentStyle } from "./environment";
import {
  minimapFrame, paintMinimapLayers, rasterizeMinimapTerrain,
  type MinimapFrame, type MinimapLayers, type MinimapTerrainSource, type MinimapVectors,
} from "./minimap-raster";
import type { Color, Prop, Scenery } from "./scenery";

/**
 * The circular minimap. Scenery becomes a top-down map (terrain colours with
 * hill shading as pixels, plus roads, water, buildings and trees as vectors);
 * `Minimap` paints that image every frame rotated so the camera heading
 * points up (or north up), with the player's arrow in the middle and other
 * units as dots. `MinimapPresenter` decides which image it shows.
 */
export type { MinimapLayers } from "./minimap-raster";

/** The playable rectangle and 4 m beyond its walls: Greyhaven Vale, Farmland and Hamlets south of it (#424), the Bandit Highlands north of it (#425) and the Marsh-Lake Wetlands east of it (#426). */
export const MINIMAP_BOUNDS_METRES = { min: [-212, -636], max: [636, 644] } as const;
const PIXELS_PER_METRE = 2;
/** The coarse local map: 1 px/m in a square reaching this far past the widest zoom plus `COARSE_MARGIN_METRES`. */
const COARSE_PIXELS_PER_METRE = 1;
const COARSE_HALF_SIZE_METRES = 140;
/** How far the player walks before the coarse map is re-centred. */
export const COARSE_MARGIN_METRES = 40;
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

/** The whole mapped rectangle at full resolution. */
export function fullMinimapFrame(): MinimapFrame {
  return minimapFrame(MINIMAP_BOUNDS_METRES.min, MINIMAP_BOUNDS_METRES.max, PIXELS_PER_METRE);
}

/** The coarse map's square around `center` (metres), on whole metres. */
export function coarseMinimapFrame(center: { x: number; z: number }): MinimapFrame {
  const halfSize = COARSE_HALF_SIZE_METRES;
  return minimapFrame(
    [Math.floor(center.x - halfSize), Math.floor(center.z - halfSize)],
    [Math.ceil(center.x + halfSize), Math.ceil(center.z + halfSize)],
    COARSE_PIXELS_PER_METRE,
  );
}

/** Scenery's terrain grid for the rasterizer, with the biome colours resolved. */
export function minimapTerrainSource(scenery: Scenery, style: EnvironmentStyle): MinimapTerrainSource {
  const { terrain, unitsPerMetre } = scenery;
  return {
    originXz: terrain.originXz,
    step: terrain.step,
    columns: terrain.columns,
    rows: terrain.rows,
    heights: terrain.heights,
    biomes: terrain.biomes,
    unitsPerMetre,
    biomeNames: new Map(scenery.biomes.map((biome) => [biome.id, biome.name])),
    biomeColors: new Map(scenery.biomes.map((biome) => [biome.id, channels(style.biomeColors[biome.name] ?? biome.color)])),
  };
}

/** The vectors a coarse local map needs: trees and buildings outside `frame` are dropped, since painting them is wasted. */
export function vectorsWithin(vectors: MinimapVectors, frame: MinimapFrame): MinimapVectors {
  const inside = ([x, z]: readonly [number, number]) => x >= frame.min[0] - 10 && x <= frame.max[0] + 10 && z >= frame.min[1] - 10 && z <= frame.max[1] + 10;
  return {
    ...vectors,
    trees: vectors.trees.filter((tree) => inside(tree.center)),
    buildings: vectors.buildings.filter((building) => building.corners.some(inside)),
  };
}

/** Deterministic map layers for a scenery value; pure, no DOM. */
export function buildMinimapLayers(scenery: Scenery, style: EnvironmentStyle): MinimapLayers {
  const frame = fullMinimapFrame();
  return { ...frame, terrain: rasterizeMinimapTerrain(minimapTerrainSource(scenery, style), frame), ...buildMinimapVectors(scenery, style) };
}

/** Deterministic vector layers for a scenery value; pure, no DOM. */
export function buildMinimapVectors(scenery: Scenery, style: EnvironmentStyle): MinimapVectors {
  const { unitsPerMetre } = scenery;
  const { min, max } = MINIMAP_BOUNDS_METRES;
  const metres = (units: number) => units / unitsPerMetre;
  const { palette } = style;
  const buildings: MinimapVectors["buildings"] = [];
  const trees: MinimapVectors["trees"] = [];
  for (const prop of scenery.props) {
    const x = metres(prop.position[0]);
    const z = metres(prop.position[2]);
    if (x < min[0] - 10 || x > max[0] + 10 || z < min[1] - 10 || z > max[1] + 10) {
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
  if (entity.kind !== "creature") {
    return entity.kind === "player" ? "player" : "friendly";
  }
  return entity.flags.hostile ? "hostile" : "neutral";
}

export type MinimapElements = {
  root: HTMLElement;
  canvas: HTMLCanvasElement;
  zoomIn: HTMLButtonElement;
  zoomOut: HTMLButtonElement;
  northUp: HTMLButtonElement;
};

export type MinimapUnit = { x: number; z: number; disposition: Disposition };

/** An approximate disturbance area (#530): a night raid's circle, centre and radius in metres. */
export type MinimapDisturbance = { x: number; z: number; radius: number };

const DISTURBANCE_COLOR = "#ff7a3d";

/** A map image ready to draw and the world rectangle it covers. */
export type MinimapImage = { source: HTMLCanvasElement | ImageBitmap; frame: MinimapFrame };

/** Paints map layers into a new canvas on the page; vectors are drawn over the terrain pixels. */
export function paintMinimapImage(layers: MinimapLayers): MinimapImage {
  const { min, max, pixelsPerMetre, width, height } = layers;
  const source = document.createElement("canvas");
  source.width = width;
  source.height = height;
  const context = source.getContext("2d");
  if (context) {
    paintMinimapLayers(context, layers);
  }
  return { source, frame: { min, max, pixelsPerMetre, width, height } };
}

/** The live minimap: a map image, a player arrow and unit dots in a circle. */
export class Minimap {
  readonly #elements: MinimapElements;
  #image: MinimapImage | null = null;
  #zoom = 1;
  #headingUp = true;

  constructor(elements: MinimapElements) {
    this.#elements = elements;
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

  /** Shows `image` from the next draw on, or none (markers only) while there is no map yet. */
  show(image: MinimapImage | null): void {
    this.#image = image;
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
  draw(
    self: { x: number; z: number },
    heading: number,
    facing: number,
    units: readonly MinimapUnit[],
    disturbance: MinimapDisturbance | null = null,
  ): void {
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
    if (this.#image) {
      const { source, frame } = this.#image;
      context.translate(radius, radius);
      context.rotate(rotation);
      const imageScale = pixelsPerMetre / frame.pixelsPerMetre;
      context.scale(imageScale, imageScale);
      context.imageSmoothingEnabled = true;
      context.drawImage(source, (frame.min[0] - self.x) * frame.pixelsPerMetre, (frame.min[1] - self.z) * frame.pixelsPerMetre);
    }
    context.restore();
    if (disturbance) {
      this.#drawDisturbance(context, disturbance, self, heading, pixelsPerMetre, radius, ratio);
    }
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

  /** A soft circle over the area, or a pip on the rim pointing at it while it is off the map. */
  #drawDisturbance(
    context: CanvasRenderingContext2D,
    area: MinimapDisturbance,
    self: { x: number; z: number },
    heading: number,
    pixelsPerMetre: number,
    radius: number,
    ratio: number,
  ): void {
    const [dx, dy] = minimapOffset(area.x - self.x, area.z - self.z, heading, this.#headingUp, pixelsPerMetre);
    const size = area.radius * pixelsPerMetre;
    context.save();
    context.beginPath();
    context.arc(radius, radius, radius, 0, 2 * Math.PI);
    context.clip();
    if (Math.hypot(dx, dy) - size < radius) {
      context.fillStyle = "rgb(255 122 61 / 22%)";
      context.strokeStyle = DISTURBANCE_COLOR;
      context.lineWidth = 1.5 * ratio;
      context.setLineDash([4 * ratio, 3 * ratio]);
      context.beginPath();
      context.arc(radius + dx, radius + dy, size, 0, 2 * Math.PI);
      context.fill();
      context.stroke();
    } else {
      const [px, py] = clampToRim([dx, dy], radius - 7 * ratio);
      context.fillStyle = DISTURBANCE_COLOR;
      context.strokeStyle = "rgb(0 0 0 / 60%)";
      context.lineWidth = ratio;
      context.beginPath();
      context.arc(radius + px, radius + py, 4.5 * ratio, 0, 2 * Math.PI);
      context.fill();
      context.stroke();
    }
    context.restore();
  }
}
