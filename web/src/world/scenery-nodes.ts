import type {
  IndexedMeshGeometry,
  Matrix4Values,
  RendererInstance,
  RendererInstanceBatch,
  RendererSceneNode,
} from "@moritzbrantner/three-d-renderer";
import type { CutawaySurface, TileDraw } from "./ground";
import type { Color, PropKind, Scenery } from "./scenery";

/**
 * The shared static world (#184): `mmorpg-presentation` builds terrain, roads,
 * water and every prop model once in Rust, and both clients draw the same
 * draws. The native client lowers them to wgpu meshes; this module decodes the
 * WASM export (`mmorpg.static-scene` v1) into renderer nodes once per content
 * revision and per frame only hides far groups and places animated parts.
 */
export type CullClass = "always" | "far" | "mid" | "near" | "detail";

/** Beyond this camera distance (metres, to the group's bounding circle) a class is hidden. */
export const CULL_DISTANCE_METRES: Readonly<Record<CullClass, number>> = {
  always: Number.POSITIVE_INFINITY,
  far: 420,
  mid: 150,
  near: 75,
  detail: 30,
};

/** What the WASM module exposes about the shared static world. */
export type StaticSceneContent = {
  staticScene(): string;
  staticSceneVertices(): Float32Array;
  staticSceneIndices(): Uint32Array;
  staticSceneModels(): Float32Array;
  staticSceneMotion(seconds: number, animate: boolean): Float32Array;
  /** The scenery's point lights at a cosmetic clock, eight floats each (#600). */
  staticSceneInterior(viewer: Float64Array): number;
  staticScenePointLights(seconds: number, animate: boolean): Float32Array;
  staticSceneShown(viewer: Float64Array, outpostKit: boolean, settlementProps: boolean, fortressKit: boolean): Uint8Array;
  surfaceY(x: number, z: number): number;
  /** A fresh stream of the terrain tiles (#526). */
  terrainStream(): TerrainStreamSource;
  /** A fresh stream of the static groups (#526), indexed as the layout's `groups`. */
  groupStream(): GroupStreamSource;
};

/**
 * Which static groups a renderer keeps resident (#526, `mmorpg_presentation::tiles::Streamer` over
 * the layout's `groups`): the groups to build now and those to drop.
 */
export type GroupStreamSource = {
  /** `[built count, built…, evicted count, evicted…]` for the camera eye and drawn kits; `dressed`: the graybox props a drawn composition kit covers, sorted. */
  update(x: number, y: number, z: number, outpost: boolean, settlementProps: boolean, nature: boolean, climbing: boolean, crag: boolean, dressed: Uint32Array): Uint32Array;
};

/**
 * The terrain's visual tiles (#526, `mmorpg_presentation::tiles`) streamed for one renderer, from the WASM owner
 * (`TerrainStream`): every tile's far level, the near levels it says to build or drop, and each tile's level per frame.
 * Tile draws are packed as `static_scene::pack` documents.
 */
export type TerrainStreamSource = {
  count(): number;
  far(index: number): Float32Array;
  near(index: number): Float32Array;
  /** `[built count, built…, evicted count, evicted…, one level per tile (1 near, 0 far)]` for the camera eye. */
  update(x: number, y: number, z: number): Uint32Array;
};

/**
 * Which asset-tooling kits the client draws: the Outpost kit buildings (#420), the market square's settlement props
 * (#421), the `greyhaven-nature` trees, rocks and bushes (#49), the climbing kit's ladders and ropes (#621) and the Battle
 * Royale fortress kit (#591).
 */
export type DrawnKits = { outpost: boolean; settlementProps: boolean; nature: boolean; climbing: boolean; crag: boolean; fortress: boolean };
const NO_KITS: DrawnKits = { outpost: false, settlementProps: false, nature: false, climbing: false, crag: false, fortress: false };
type FallbackKit = keyof DrawnKits;

/** `fallback`: the kit whose procedural stand-in the group is, hidden while the client draws that kit. */
type Bounds = { cull: CullClass; center: readonly [number, number]; radius: number; fallback?: FallbackKit };
/** One merged static group's geometry as typed views of the shared WASM buffers, for the batches. */
export type GroupBuffers = { positions: Float32Array; normals: Float32Array; colors: Float32Array | null; indices: Uint32Array };
/**
 * A merged static group (#526): `group` is its index in the layout, as `GroupStreamSource` names it. `node.geometry` is
 * decoded the first time something reads it; [`buffers`] reads the same data without allocating.
 */
export type StaticBatch = Bounds & { group: number; node: RendererSceneNode & { geometry: IndexedMeshGeometry }; buffers(): GroupBuffers };
export type InstanceBatch = Bounds & { group: number; batch: RendererInstanceBatch };

// Animated props use the default lit material; retain that branch of the renderer contract.
type AnimatedNode = Omit<Extract<RendererSceneNode, { unlit?: false }>, "transform" | "modelMatrix">;

export type SceneryScene = {
  batches: readonly StaticBatch[];
  /** Static rock surfaces share the terrain bundle and its material batches. */
  surfaces: readonly TileDraw[];
  /** Fixed rock roofs share the terrain material and the owning animated part's cutaway. */
  cutaways: readonly CutawaySurface[];
  instanceBatches: readonly InstanceBatch[];
  water: readonly RendererSceneNode[];
  /** Nodes of each animated part; `motion` places them. */
  animated: readonly (readonly AnimatedNode[])[];
  /** One column-major matrix (16 floats) per animated part at a cosmetic clock. */
  motion(seconds: number, animate: boolean): Float32Array;
  /** Campfire and lamp lights at a cosmetic clock: position, sRGB colour, lumens and range, eight floats each. */
  interior(viewer: readonly [number, number, number]): number;
  pointLights(seconds: number, animate: boolean): Float32Array;
  /**
   * Per animated part 1 (drawn) or 0: a cave roof is cut away while `viewer` (feet, metres) stands under it, and an
   * asset-tooling kit's procedural fallback is hidden while `kits` says the kit is drawn.
   */
  shown(viewer: readonly [number, number, number] | null, kits: DrawnKits): Uint8Array;
  stats: SceneryStats;
  /** The rendered terrain height in metres at `(x, z)` metres. */
  surfaceY(x: number, z: number): number;
};

export type SceneryStats = {
  /** Ordinary and instanced static draw groups, including currently hidden ones. */
  staticNodes: number;
  /** Vertices stored across unique immutable geometries (instance copies are transforms). */
  staticVertices: number;
  /** Triangles stored across the same unique geometries, before instance expansion. */
  staticTriangles: number;
  props: Partial<Record<PropKind, number>>;
};

const FORMAT = "mmorpg.static-scene";
const CULLS: readonly CullClass[] = ["always", "far", "mid", "near", "detail"];
/**
 * Instanced detail (ground cover, handholds, repeated prop pieces) casts only into the two near sun cascades: in the
 * far one it covers less than a texel and cost about a fifth of its draws (three-lighting `shadowCascades`).
 */
const NEAR_SHADOW = { shadowCascades: 2 };

type GeometryEntry = {
  resourceKey: string; vertexOffset: number; vertexCount: number; colored: boolean; textured: boolean; weighted: boolean; indexOffset: number; indexCount: number;
};
type DrawEntry = { id: string; geometry: number; model: number; color: Color; emissive: Color | null; opacity: number; ground?: { layer: string; overlay: boolean } };

type GroupEntry = { cull: CullClass; instanced: boolean; fallback?: FallbackKit | null; min: [number, number]; max: [number, number]; draws: DrawEntry[] };
type Layout = { format: string; prefix: string; geometries: GeometryEntry[]; groups: GroupEntry[]; water: DrawEntry[]; animated: DrawEntry[][] };

function fail(reason: string): never {
  throw new Error(`Invalid static scene: ${reason}`);
}

function triples(values: Float32Array, start: number, count: number): [number, number, number][] {
  const out: [number, number, number][] = new Array(count);
  for (let index = 0; index < count; index += 1) {
    const at = start + index * 3;
    out[index] = [values[at]!, values[at + 1]!, values[at + 2]!];
  }
  return out;
}

/**
 * An instance batch's mesh name, `scenery.<kind>.<cull>[.fallback-<kit>]`, without its chunk or geometry index so the
 * draw profile groups one kind of instance: batch IDs are `instances:<chunk>:<cull>[:fallback-<kit>]:<kind>…` (the
 * presentation `Batcher`), where the kind is `handholds`, a ground-cover kind or the static scene's prefixed `scenery`
 * (repeated prop pieces).
 */
function instanceBatchName(id: string, group: GroupEntry): string {
  const segments = id.split(":");
  const kind = segments[group.fallback ? 4 : 3] ?? "unknown";
  return `scenery.${kind === "scenery" ? "prop-pieces" : kind}.${group.cull}${group.fallback ? `.fallback-${group.fallback}` : ""}`;
}

function bounds(group: GroupEntry): Bounds {
  const [minX, minZ] = group.min;
  const [maxX, maxZ] = group.max;
  return {
    cull: group.cull, center: [(minX + maxX) / 2, (minZ + maxZ) / 2], radius: Math.hypot(maxX - minX, maxZ - minZ) / 2,
    ...(group.fallback ? { fallback: group.fallback } : {}),
  };
}

/** Decodes the shared static world once; unknown shapes fail closed. */
export function buildSceneryScene(scenery: Scenery, content: StaticSceneContent): SceneryScene {
  let layout: Layout;
  try {
    layout = JSON.parse(content.staticScene()) as Layout;
  } catch {
    fail("not JSON");
  }
  if (layout.format !== FORMAT) {
    fail(`unsupported format ${String(layout.format)}`);
  }
  if (!layout.prefix.startsWith(`scenery:${scenery.contentRevision}:`)) {
    fail(`prefix ${layout.prefix} does not match content revision ${scenery.contentRevision}`);
  }
  const vertices = content.staticSceneVertices();
  const indices = content.staticSceneIndices();
  const models = content.staticSceneModels();
  const floats = (entry: GeometryEntry) => entry.vertexCount * ((entry.colored ? 9 : 6) + (entry.textured ? 6 : 0) + (entry.weighted ? 1 : 0));
  for (const entry of layout.geometries) {
    if (entry.vertexOffset + floats(entry) > vertices.length || entry.indexOffset + entry.indexCount > indices.length) {
      fail(`geometry ${entry.resourceKey} exceeds the shared buffers`);
    }
  }
  // Decoded on first read (#526): most of the world's geometries are never drawn from most places.
  const decoded = new Map<number, IndexedMeshGeometry>();
  const geometryEntry = (draw: DrawEntry) => layout.geometries[draw.geometry] ?? fail(`draw ${draw.id} names a missing geometry`);
  const geometry = (draw: DrawEntry): IndexedMeshGeometry => {
    let geometry = decoded.get(draw.geometry);
    if (!geometry) {
      const entry = geometryEntry(draw);
      const { vertexOffset: start, vertexCount: count, indexOffset, indexCount } = entry;
      const mesh: IndexedMeshGeometry = {
        kind: "mesh",
        resourceKey: entry.resourceKey,
        positions: triples(vertices, start, count),
        normals: triples(vertices, start + count * 3, count),
        indices: Array.from(indices.subarray(indexOffset, indexOffset + indexCount)),
      };
      geometry = entry.colored ? { ...mesh, colors: triples(vertices, start + count * 6, count) } : mesh;
      decoded.set(draw.geometry, geometry);
    }
    return geometry;
  };
  const buffers = (draw: DrawEntry): GroupBuffers => {
    const entry = geometryEntry(draw);
    const { vertexOffset: start, vertexCount: count, indexOffset, indexCount } = entry;
    return {
      positions: vertices.subarray(start, start + count * 3), normals: vertices.subarray(start + count * 3, start + count * 6),
      colors: entry.colored ? vertices.subarray(start + count * 6, start + count * 9) : null,
      indices: indices.subarray(indexOffset, indexOffset + indexCount),
    };
  };
  /** `node` with a `geometry` decoded when first read. */
  const lazyGeometry = <T extends object>(node: T, draw: DrawEntry): T & { geometry: IndexedMeshGeometry } => {
    geometryEntry(draw);
    return Object.defineProperty(node, "geometry", { enumerable: true, get: () => geometry(draw) }) as T & { geometry: IndexedMeshGeometry };
  };
  const model = (draw: DrawEntry): Matrix4Values => {
    if ((draw.model + 1) * 16 > models.length) {
      fail(`draw ${draw.id} names a missing model matrix`);
    }
    return Array.from(models.subarray(draw.model * 16, draw.model * 16 + 16)) as Matrix4Values;
  };
  const surface = (draw: DrawEntry): TileDraw => {
    const entry = layout.geometries[draw.geometry] ?? fail("surface names a missing geometry");
    if (!draw.ground || !entry.colored || !entry.textured) {
      fail("textured surface needs a material, colors, UVs and tangents");
    }
    const count = entry.vertexCount;
    let at = entry.vertexOffset;
    const take = (length: number) => {
      const out = vertices.subarray(at, at + length);
      at += length;
      return out;
    };
    return {
      layer: draw.ground.layer, overlay: draw.ground.overlay, far: false,
      positions: take(count * 3), normals: take(count * 3), colors: take(count * 3),
      uvs: take(count * 2), tangents: take(count * 4),
      alpha: entry.weighted ? take(count) : new Float32Array(count).fill(1),
      indices: indices.slice(entry.indexOffset, entry.indexOffset + entry.indexCount),
    };
  };
  const surfaces: TileDraw[] = [];
  const batches: StaticBatch[] = [];
  const instanceBatches: InstanceBatch[] = [];
  for (const [groupIndex, group] of layout.groups.entries()) {
    if (!CULLS.includes(group.cull) || group.draws.length === 0) {
      fail("group has an unknown cull class or no draws");
    }
    if (group.fallback && !(group.fallback in NO_KITS)) {
      fail(`group is the fallback of an unknown kit ${group.fallback}`);
    }
    if (!group.instanced) {
      const [draw] = group.draws;
      if (draw!.ground) {
        // Shared static rock geometry is already in world space, as terrain tiles are.
        surfaces.push(surface(draw!));
        continue;
      }
      batches.push({
        ...bounds(group),
        group: groupIndex,
        buffers: () => buffers(draw!),
        node: lazyGeometry({
          id: draw!.id,
          color: draw!.color,
          modelMatrix: model(draw!),
          ...(draw!.emissive ? { unlit: false as const, emissive: draw!.emissive } : {}),
        }, draw!) as StaticBatch["node"],
      });
      continue;
    }
    const first = group.draws[0]!;
    // Instance draw IDs are `<batch id>:<ordinal>`; the batch shares one geometry.
    const id = first.id.slice(0, first.id.lastIndexOf(":"));
    const batch = lazyGeometry({
      id, name: instanceBatchName(id, group), userData: NEAR_SHADOW, color: "#ffffff", revision: layout.prefix,
    }, first) as unknown as RendererInstanceBatch;
    // The instance transforms stay undecoded with the geometry until the batch is first drawn.
    let instances: RendererInstance[] | null = null;
    Object.defineProperty(batch, "instances", {
      enumerable: true,
      get: () => (instances ??= group.draws.map((draw) => ({ modelMatrix: model(draw), color: draw.color }))),
    });
    instanceBatches.push({ ...bounds(group), group: groupIndex, batch });
  }
  const water = layout.water.map((draw): RendererSceneNode => lazyGeometry({
    id: draw.id, color: draw.color, opacity: draw.opacity, modelMatrix: model(draw),
  }, draw) as unknown as RendererSceneNode);
  // Fixed textured roofs use their rest transform and shared cutaway visibility; moving
  // props (flames, doors, sails) retain the renderer's ordinary lit-material path.
  const cutaways: CutawaySurface[] = [];
  const rest = content.staticSceneMotion(0, false);
  const animated = layout.animated.map((part, index) => part.flatMap((draw): AnimatedNode[] => {
    if (draw.ground) {
      if (rest.length < (index + 1) * 16) { fail("cutaway roof needs its rest transform"); }
      cutaways.push({ part: index, draw: surface(draw), model: rest.slice(index * 16, index * 16 + 16) });
      return [];
    }
    return [{ id: draw.id, geometry: geometry(draw), color: draw.color, opacity: draw.opacity,
      ...(draw.emissive ? { unlit: false as const, emissive: draw.emissive } : {}) }];
  }));
  const byKind: Partial<Record<PropKind, number>> = {};
  for (const prop of scenery.props) {
    byKind[prop.kind] = (byKind[prop.kind] ?? 0) + 1;
  }
  return {
    batches,
    surfaces,
    cutaways,
    instanceBatches,
    water,
    animated,
    motion: (seconds, animate) => content.staticSceneMotion(seconds, animate),
    interior: (viewer) => content.staticSceneInterior(Float64Array.from(viewer)),
    pointLights: (seconds, animate) => content.staticScenePointLights(seconds, animate),
    shown: (viewer, kits) => content.staticSceneShown(Float64Array.from(viewer ?? []), kits.outpost, kits.settlementProps, kits.fortress),
    stats: {
      staticNodes: batches.length + surfaces.length + cutaways.length + instanceBatches.length,
      staticVertices: layout.geometries.reduce((sum, entry) => sum + entry.vertexCount, 0),
      staticTriangles: layout.geometries.reduce((sum, entry) => sum + entry.indexCount / 3, 0),
      props: byKind,
    },
    surfaceY: (x, z) => content.surfaceY(x, z),
  };
}

/** Whether a group is drawn: not the fallback of a kit the client draws, and within its class's distance of the eye. */
function batchShown(batch: Bounds, eyeX: number, eyeZ: number, kits: DrawnKits): boolean {
  return !(batch.fallback && kits[batch.fallback]) && batchVisible(batch, eyeX, eyeZ);
}

/** Whether a group is within its class's distance of the camera eye (XZ, metres). */
export function batchVisible(batch: Bounds, eyeX: number, eyeZ: number): boolean {
  const distance = Math.hypot(batch.center[0] - eyeX, batch.center[1] - eyeZ) - batch.radius;
  return distance <= CULL_DISTANCE_METRES[batch.cull];
}

/**
 * Cosmetic animation input; without motion every part holds its rest pose. Cave roofs are cut
 * away while `viewer` (feet, metres) stands under them (#159); a kit's procedural fallback stays drawn until
 * `kits` says the kit is.
 */
export type SceneryMotion = {
  seconds: number; animate: boolean; viewer?: readonly [number, number, number] | null; kits?: DrawnKits;
  /** Which layout groups are resident (#526): the rest are not built yet and their instance batches are left out. */
  resident?: (group: number) => boolean;
};

/** Per-frame renderer nodes: static batches (far detail hidden), water and animated parts. */
export class SceneryFrame {
  readonly #scene: SceneryScene;
  readonly #visible: RendererSceneNode[];
  readonly #hidden: RendererSceneNode[];

  constructor(scene: SceneryScene) {
    this.#scene = scene;
    this.#visible = scene.batches.map((batch) => batch.node);
    // A hidden node inherits from its visible one, so hiding a group does not decode its geometry (#526).
    this.#hidden = scene.batches.map((batch) => Object.create(batch.node, { visible: { value: false, enumerable: true } }) as RendererSceneNode);
  }

  nodes(eye: readonly [number, number, number], motion: SceneryMotion): { nodes: RendererSceneNode[]; instanceBatches: RendererInstanceBatch[]; visibleBatches: number; shown: Uint8Array } {
    const nodes: RendererSceneNode[] = [];
    let visibleBatches = 0;
    const kits = motion.kits ?? NO_KITS;
    this.#scene.batches.forEach((batch, index) => {
      const visible = batchShown(batch, eye[0], eye[2], kits);
      visibleBatches += visible ? 1 : 0;
      nodes.push(visible ? this.#visible[index]! : this.#hidden[index]!);
    });
    nodes.push(...this.#scene.water);
    const matrices = this.#scene.motion(motion.seconds, motion.animate);
    const shown = this.#scene.shown(motion.viewer ?? null, kits);
    this.#scene.animated.forEach((part, index) => {
      const modelMatrix = Array.from(matrices.subarray(index * 16, index * 16 + 16)) as Matrix4Values;
      const visible = shown[index] !== 0;
      for (const node of part) {
        nodes.push(visible ? { ...node, modelMatrix } : { ...node, modelMatrix, visible: false });
      }
    });
    const resident = motion.resident ?? (() => true);
    const instanceBatches: RendererInstanceBatch[] = [];
    for (const batch of this.#scene.instanceBatches) {
      const visible = batchShown(batch, eye[0], eye[2], kits);
      visibleBatches += visible ? 1 : 0;
      if (resident(batch.group)) { instanceBatches.push({ ...batch.batch, visible }); }
    }
    return { nodes, instanceBatches, visibleBatches, shown };
  }
}
