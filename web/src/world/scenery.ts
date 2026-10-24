import type { StaticSceneContent } from "./scenery-nodes";
import type { UnitPresenterHandle } from "./unit-presenter";
/**
 * Presentation scenery from the WASM `scenery()` export (format
 * `mmorpg.scenery`), which maps the Rust `mmorpg-scenery` value the native
 * client draws: a terrain grid and a coarse far ring of the same relief,
 * props as compact records (kind, feet anchor, yaw, scale, body box) with the
 * exact collider box of every structure, roads, water and named areas. The
 * render loop consumes only the decoded `Scenery`. Relief and area lookups
 * call back into Rust so both clients share one owner for them.
 */
export type Color = `#${string}`;
export type XZ = readonly [number, number];
export type XYZ = readonly [number, number, number];

export type TerrainGrid = {
  /** Units at sample (0, 0); sample (column, row) lies at origin + step × (column, row). */
  originXz: XZ;
  step: number;
  columns: number;
  rows: number;
  /** Row-major (Z outer) presentation heights in units. */
  heights: readonly number[];
  /** Row-major biome IDs. */
  biomes: readonly number[];
};

export type Biome = { id: number; name: string; color: Color };

/** Every prop kind the browser draws, by its `mmorpg-scenery` name. */
export const PROP_KINDS = [
  "keep", "inn", "house", "smithy", "barn", "farmhouse", "windmill", "well", "palisade", "gate-post",
  "waystone", "gravestone", "tree-oak", "tree-pine", "tree-birch", "bush", "rock-small", "rock-medium",
  "rock-large", "cliff", "grass-tuft", "flowers", "reeds", "fence", "tent", "campfire", "crate", "barrel",
  "cart", "signpost", "lamp", "mine-entrance", "crop-row", "dock", "cave-roof", "ramp", "loading-dock",
  "shrine-kit",
] as const;
export type PropKind = (typeof PROP_KINDS)[number];

/** The exact core collider box a structure visualises, in units. */
export type ColliderBox = { id: number; center: XYZ; halfExtents: XYZ };

export type Prop = {
  kind: PropKind;
  /** Feet anchor in units, standing on the presentation relief. */
  position: XYZ;
  /** u16 yaw: 0 faces +Z, increasing toward +X. */
  yaw: number;
  /** Size relative to the kind's base size (1 is the base size). */
  scale: number;
  /** Ground-level body box half extents in units, before yaw. */
  halfExtents: XYZ;
  collider: ColliderBox | null;
};

export type Road = { name: string; halfWidth: number; points: readonly XZ[] };
export type Water = { centerXz: XZ; radiiXz: XZ; surfaceY: number };
export type Area = { id: number; name: string; minXz: XZ; maxXz: XZ };

export type Scenery = {
  source: string;
  contentRevision: bigint;
  /** Core-independent presentation identity supplied by shared scenery. */
  presentationFingerprint: string;
  unitsPerMetre: number;
  playerHalfExtents: XYZ;
  terrain: TerrainGrid;
  /** The same relief sampled coarsely over a wider square, for distant mountains. */
  farTerrain: TerrainGrid;
  biomes: readonly Biome[];
  props: readonly Prop[];
  roads: readonly Road[];
  water: readonly Water[];
  areas: readonly Area[];
};

/** What the WASM module exposes about static zone content. */
export type SceneryContent = StaticSceneContent & {
  scenery(): string;
  /** Review-only presentation; the host never calls this. */
  reviewStaticSceneWithoutOak?(index: number): string;
  /** The shared unit presenter (#184) of one joined player. */
  UnitPresenter: new (player: number) => UnitPresenterHandle;
  /** The shared walk-in inn presenter (#158): door swing and interior look. */
  InnPresenter: (new () => InnPresenterHandle) & {
    entries(): string[];
    reachableDoor(x: number, z: number): number;
  };
  /** The Sunken Shrine's kit (#431): its entries, and per placed module its entry and 16 model floats. */
  shrineKitEntries(): string[];
  shrineKitKeys(): string[];
  shrineKitModels(): Float32Array;
  /** The Outpost kit buildings (#420): their entries (`<key>` or `<key>/<variant>`), and per placed module its entry and 16 model floats. */
  outpostKitEntries(): string[];
  outpostKitKeys(): string[];
  outpostKitModels(): Float32Array;
  /** The market square's settlement props (#421): their entries, and per placed prop its entry and 16 model floats. */
  settlementPropEntries(): string[];
  settlementPropKeys(): string[];
  settlementPropModels(): Float32Array;
  /** Greyhaven's ambient animals (#419): entries, clip names, first-waypoint anchors (x, z units) and per animal 16 model floats, clip index and clip time. */
  ambientAnimalEntries(): string[];
  ambientAnimalClips(): string[];
  ambientAnimalAnchors(): Int32Array;
  ambientAnimalFrame(seconds: number, animate: boolean): Float32Array;
  /** The rest clip an imported unit model is measured in. */
  importedRestClip(kind: string): string | undefined;
  areaAt(x: number, z: number): number | undefined;
  /** Destructible pieces (#305) feet at `(x, z)` units can repair, in ID order. */
  reachablePieces(x: number, z: number): Uint16Array;
  reliefAt(x: number, z: number): number;
  /** Per area, the imported unit models its creatures use (#240), as JSON. */
  regionModels(): string;
};

/** The WASM inn presenter (`mmorpg_presentation::buildings::Inn`). */
export type InnPresenterHandle = {
  /** Per entry 16 column-major model floats then 1 shown / 0 hidden; `viewer` is feet in metres or empty. */
  frame(open: boolean, viewer: Float64Array, dt: number, reducedMotion: boolean): Float32Array;
};

/** The shared presenter's ambient animal loops (`mmorpg_presentation::ambient`, #419). */
export type AmbientAnimalsSource = {
  entries: readonly string[];
  clips: readonly string[];
  /** Each animal's first waypoint, `[x, z]` in units. */
  anchors: readonly (readonly [number, number])[];
  /** Per animal 16 column-major model floats (metres), the clip index and the clip time. */
  frame(seconds: number, animate: boolean): Float32Array;
};

export type SceneryProvider = {
  readonly scenery: Scenery;
  /** The shared static world both clients draw (#184), straight from the WASM owner. */
  readonly staticScene: StaticSceneContent;
  readonly reviewWithoutOak?: (index: number) => StaticSceneContent;
  /** A fresh shared unit presenter for `player`. */
  unitPresenter(player: number): UnitPresenterHandle;
  /** The walk-in inn's bundle entries, its presenter, and the door feet at `(x, z)` units reach (0 none). */
  readonly inn: { entries(): readonly string[]; presenter(): InnPresenterHandle; reachableDoor(x: number, z: number): number };
  /** The Sunken Shrine's kit entries and placed modules (#431), column-major matrices in metres. */
  readonly shrine: { entries: readonly string[]; placements: readonly { key: string; matrix: Float32Array }[] };
  /** The Outpost kit buildings' entries and placed modules (#420), column-major matrices in metres. */
  readonly outpost: { entries: readonly string[]; placements: readonly { key: string; matrix: Float32Array }[] };
  /** The market square's settlement-prop entries and placed props (#421), column-major matrices in metres. */
  readonly settlement: { entries: readonly string[]; placements: readonly { key: string; matrix: Float32Array }[] };
  /** Greyhaven's cosmetic ambient animals (#419). */
  readonly ambient: AmbientAnimalsSource;
  importedRestClip(kind: string): string;
  /** The named core area at `(x, z)` in units, if any. */
  areaAt(x: number, z: number): Area | null;
  /** Destructible pieces (#305) feet at `(x, z)` units can repair, in ID order. */
  reachablePieces(x: number, z: number): readonly number[];
  /** Presentation relief at `(x, z)` in units; units are drawn raised by it. */
  reliefAt(x: number, z: number): number;
  /** The imported unit models the creatures of area `id` use (#240). */
  regionModels(id: number): readonly string[];
};

const FORMAT = "mmorpg.scenery";
const MAX_TERRAIN_SAMPLES = 1 << 20;
const MAX_PROPS = 1 << 16;
const MAX_AREAS = 64;
const MAX_ROADS = 64;
const MAX_ROAD_POINTS = 256;
/** `[kind, x, feetY, z, yaw, scalePermille, halfX, halfY, halfZ]`. */
const PROP_RECORD_FIELDS = 9;
const KNOWN_KINDS: ReadonlySet<string> = new Set(PROP_KINDS);

type Json = Record<string, unknown>;

function fail(message: string): never {
  throw new Error(`Invalid scenery: ${message}.`);
}

function object(value: unknown, keys: readonly string[], name: string): Json {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  const record = value as Json;
  const actual = Object.keys(record);
  if (actual.length !== keys.length || keys.some((key) => !Object.hasOwn(record, key))) {
    fail(`${name} must have exactly ${keys.join(", ")}`);
  }
  return record;
}

function int(value: unknown, name: string, min = -(2 ** 31), max = 2 ** 31 - 1): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
    fail(`${name} must be an integer in [${min}, ${max}]`);
  }
  return value;
}

function list(value: unknown, name: string, maximum: number): readonly unknown[] {
  if (!Array.isArray(value) || value.length > maximum) {
    fail(`${name} must be an array of at most ${maximum} entries`);
  }
  return value;
}

function integers(value: unknown, length: number, name: string, min?: number): number[] {
  const entries = list(value, name, length);
  if (entries.length !== length) {
    fail(`${name} must have ${length} entries`);
  }
  return entries.map((entry, index) => int(entry, `${name}[${index}]`, min));
}

function xz(value: unknown, name: string, min?: number): XZ {
  const [x = 0, z = 0] = integers(value, 2, name, min);
  return [x, z];
}

function xyz(value: unknown, name: string, min?: number): XYZ {
  const [x = 0, y = 0, z = 0] = integers(value, 3, name, min);
  return [x, y, z];
}

function text(value: unknown, name: string): string {
  if (typeof value !== "string" || value.trim().length === 0 || value.length > 64) {
    fail(`${name} must be non-empty text`);
  }
  return value;
}

function color(value: unknown, name: string): Color {
  if (typeof value !== "string" || !/^#[0-9a-f]{6}$/i.test(value)) {
    fail(`${name} must be a #rrggbb colour`);
  }
  return value as Color;
}

function decodeTerrain(value: unknown, biomeIds: ReadonlySet<number>): TerrainGrid {
  const terrain = object(value, ["originXz", "step", "columns", "rows", "heights", "biomes"], "terrain");
  const columns = int(terrain.columns, "terrain columns", 2, 4096);
  const rows = int(terrain.rows, "terrain rows", 2, 4096);
  const samples = columns * rows;
  if (samples > MAX_TERRAIN_SAMPLES) {
    fail("terrain has too many samples");
  }
  const heights = list(terrain.heights, "terrain heights", samples).map((height, index) => int(height, `height ${index}`));
  const biomes = list(terrain.biomes, "terrain biomes", samples).map((biome, index) => {
    const id = int(biome, `biome ${index}`, 0, 255);
    if (!biomeIds.has(id)) {
      fail(`terrain sample ${index} uses unknown biome ${id}`);
    }
    return id;
  });
  if (heights.length !== samples || biomes.length !== samples) {
    fail("terrain arrays must hold columns × rows samples");
  }
  return {
    originXz: xz(terrain.originXz, "terrain origin"),
    step: int(terrain.step, "terrain step", 1),
    columns,
    rows,
    heights,
    biomes,
  };
}

function decodePropKinds(value: unknown): PropKind[] {
  const kinds = list(value, "prop kinds", 256).map((entry, index) => {
    const name = text(entry, `prop kind ${index}`);
    if (!KNOWN_KINDS.has(name)) {
      fail(`prop kind ${index} is unknown: ${name}`);
    }
    return name as PropKind;
  });
  if (new Set(kinds).size !== kinds.length) {
    fail("prop kinds must be unique");
  }
  return kinds;
}

function decodeProp(value: unknown, index: number, kinds: readonly PropKind[]): Prop {
  const name = `prop ${index}`;
  const [kind = 0, x = 0, y = 0, z = 0, yaw = 0, scale = 0, halfX = 0, halfY = 0, halfZ = 0] =
    integers(value, PROP_RECORD_FIELDS, name);
  const propKind = kinds[kind];
  if (propKind === undefined) {
    fail(`${name} has no kind ${kind}`);
  }
  return {
    kind: propKind,
    position: [x, y, z],
    yaw: int(yaw, `${name} yaw`, 0, 65_535),
    scale: int(scale, `${name} scale`, 1, 65_535) / 1_000,
    halfExtents: [int(halfX, `${name} half x`, 1), int(halfY, `${name} half y`, 1), int(halfZ, `${name} half z`, 1)],
    collider: null,
  };
}

/** Attaches every structure's exact collider box to its prop; each prop and collider appears once. */
function attachStructures(value: unknown, props: Prop[]): void {
  const colliders = new Set<number>();
  list(value, "structures", MAX_PROPS).forEach((entry, index) => {
    const structure = object(entry, ["prop", "colliderId", "center", "halfExtents"], `structure ${index}`);
    const propIndex = int(structure.prop, `structure ${index} prop`, 0);
    const prop = props[propIndex];
    if (!prop) {
      fail(`structure ${index} names missing prop ${propIndex}`);
    }
    if (prop.collider) {
      fail(`prop ${propIndex} has two structures`);
    }
    const id = int(structure.colliderId, `structure ${index} collider`, 0);
    if (colliders.has(id)) {
      fail(`collider ${id} has two structures`);
    }
    colliders.add(id);
    props[propIndex] = {
      ...prop,
      collider: {
        id,
        center: xyz(structure.center, `structure ${index} centre`),
        halfExtents: xyz(structure.halfExtents, `structure ${index} half extents`, 1),
      },
    };
  });
}

function decodeRoad(value: unknown, index: number): Road {
  const road = object(value, ["name", "halfWidth", "points"], `road ${index}`);
  const points = list(road.points, `road ${index} points`, MAX_ROAD_POINTS)
    .map((point, pointIndex) => xz(point, `road ${index} point ${pointIndex}`));
  if (points.length < 2) {
    fail(`road ${index} needs at least two points`);
  }
  return { name: text(road.name, `road ${index} name`), halfWidth: int(road.halfWidth, `road ${index} half width`, 1), points };
}

/** Strictly decodes the export at the WASM boundary; unknown shapes fail closed. */
export function decodeScenery(json: string): Scenery {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    fail("not JSON");
  }
  const root = object(parsed, [
    "format", "source", "contentRevision", "presentationFingerprint", "unitsPerMetre", "playerHalfExtents",
    "terrain", "farTerrain", "biomes", "propKinds", "props", "structures", "roads", "water", "areas",
  ], "export");
  if (root.format !== FORMAT) {
    fail(`unsupported format ${String(root.format)}`);
  }
  if (typeof root.contentRevision !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(root.contentRevision)) {
    fail("content revision must be a canonical decimal u64");
  }
  const contentRevision = BigInt(root.contentRevision);
  if (contentRevision > 0xffff_ffff_ffff_ffffn) {
    fail("content revision exceeds u64");
  }
  if (typeof root.presentationFingerprint !== "string" || !/^[0-9a-f]{16}$/.test(root.presentationFingerprint)) {
    fail("presentation fingerprint must be a canonical hexadecimal u64");
  }
  const biomes = list(root.biomes, "biomes", 256).map((value, index) => {
    const biome = object(value, ["id", "name", "color"], `biome ${index}`);
    return { id: int(biome.id, `biome ${index} id`, 0, 255), name: text(biome.name, `biome ${index} name`), color: color(biome.color, `biome ${index} colour`) };
  });
  const biomeIds = new Set(biomes.map((biome) => biome.id));
  if (biomeIds.size !== biomes.length) {
    fail("biome IDs must be unique");
  }
  const areas = list(root.areas, "areas", MAX_AREAS).map((value, index) => {
    const area = object(value, ["id", "name", "minXz", "maxXz"], `area ${index}`);
    const minXz = xz(area.minXz, `area ${index} min`);
    const maxXz = xz(area.maxXz, `area ${index} max`);
    if (minXz[0] > maxXz[0] || minXz[1] > maxXz[1]) {
      fail(`area ${index} bounds are inverted`);
    }
    return { id: int(area.id, `area ${index} id`, 0, 65_535), name: text(area.name, `area ${index} name`), minXz, maxXz };
  });
  if (new Set(areas.map((area) => area.id)).size !== areas.length) {
    fail("area IDs must be unique");
  }
  const kinds = decodePropKinds(root.propKinds);
  const props = list(root.props, "props", MAX_PROPS).map((value, index) => decodeProp(value, index, kinds));
  attachStructures(root.structures, props);
  return {
    source: text(root.source, "source"),
    contentRevision,
    presentationFingerprint: root.presentationFingerprint,
    unitsPerMetre: int(root.unitsPerMetre, "units per metre", 1),
    playerHalfExtents: xyz(root.playerHalfExtents, "player half extents", 1),
    terrain: decodeTerrain(root.terrain, biomeIds),
    farTerrain: decodeTerrain(root.farTerrain, biomeIds),
    biomes,
    props,
    roads: list(root.roads, "roads", MAX_ROADS).map(decodeRoad),
    water: list(root.water, "water", 64).map((value, index) => {
      const water = object(value, ["centerXz", "radiiXz", "surfaceY"], `water ${index}`);
      return {
        centerXz: xz(water.centerXz, `water ${index} centre`),
        radiiXz: xz(water.radiiXz, `water ${index} radii`, 1),
        surfaceY: int(water.surfaceY, `water ${index} surface`),
      };
    }),
    areas,
  };
}

/** Decodes the export once and routes relief/area queries to the Rust owners. */
export function createSceneryProvider(content: SceneryContent): SceneryProvider {
  const scenery = decodeScenery(content.scenery());
  const areasById = new Map(scenery.areas.map((area) => [area.id, area]));
  const regionModels = new Map((JSON.parse(content.regionModels()) as { area: number; models: string[] }[])
    .map((region) => [region.area, region.models]));
  return {
    regionModels: (id) => regionModels.get(id) ?? [],
    scenery,
    staticScene: content,
    ...(content.reviewStaticSceneWithoutOak ? {
      reviewWithoutOak: (index: number): StaticSceneContent => {
        const review = JSON.parse(content.reviewStaticSceneWithoutOak!(index)) as {
          layout: string; vertices: number[]; indices: number[]; models: number[];
        };
        return {
          staticScene: () => review.layout,
          staticSceneVertices: () => Float32Array.from(review.vertices),
          staticSceneIndices: () => Uint32Array.from(review.indices),
          staticSceneModels: () => Float32Array.from(review.models),
          staticSceneMotion: (seconds, animate) => content.staticSceneMotion(seconds, animate),
          staticSceneShown: (viewer, outpostKit, settlementProps) => content.staticSceneShown(viewer, outpostKit, settlementProps),
          surfaceY: (x, z) => content.surfaceY(x, z),
        };
      },
    } : {}),
    unitPresenter: (player) => new content.UnitPresenter(player),
    inn: {
      entries: () => content.InnPresenter.entries(),
      presenter: () => new content.InnPresenter(),
      reachableDoor: (x, z) => content.InnPresenter.reachableDoor(Math.round(x), Math.round(z)),
    },
    shrine: (() => {
      const models = content.shrineKitModels();
      return {
        entries: content.shrineKitEntries(),
        placements: content.shrineKitKeys().map((key, index) => ({ key, matrix: models.slice(index * 16, index * 16 + 16) })),
      };
    })(),
    outpost: (() => {
      const models = content.outpostKitModels();
      return {
        entries: content.outpostKitEntries(),
        placements: content.outpostKitKeys().map((key, index) => ({ key, matrix: models.slice(index * 16, index * 16 + 16) })),
      };
    })(),
    settlement: (() => {
      const models = content.settlementPropModels();
      return {
        entries: content.settlementPropEntries(),
        placements: content.settlementPropKeys().map((key, index) => ({ key, matrix: models.slice(index * 16, index * 16 + 16) })),
      };
    })(),
    ambient: (() => {
      const anchors = content.ambientAnimalAnchors();
      return {
        entries: content.ambientAnimalEntries(),
        clips: content.ambientAnimalClips(),
        anchors: Array.from({ length: anchors.length / 2 }, (_, index) => [anchors[index * 2]!, anchors[index * 2 + 1]!] as const),
        frame: (seconds: number, animate: boolean) => content.ambientAnimalFrame(seconds, animate),
      };
    })(),
    importedRestClip(kind) {
      const clip = content.importedRestClip(kind);
      if (clip === undefined) {
        throw new Error(`No imported unit model ${kind}.`);
      }
      return clip;
    },
    reachablePieces(x, z) {
      return [...content.reachablePieces(Math.round(x), Math.round(z))];
    },
    areaAt(x, z) {
      const id = content.areaAt(Math.round(x), Math.round(z));
      if (id === undefined) {
        return null;
      }
      const area = areasById.get(id);
      if (!area) {
        throw new Error(`Area ${id} is missing from the scenery export.`);
      }
      return area;
    },
    reliefAt(x, z) {
      return content.reliefAt(Math.round(x), Math.round(z));
    },
  };
}
