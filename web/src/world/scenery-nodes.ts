import type {
  IndexedMeshGeometry,
  Matrix4Values,
  RendererInstance,
  RendererInstanceBatch,
  RendererSceneNode,
} from "@moritzbrantner/three-d-renderer";
import type { Color, PropKind, Scenery } from "./scenery";

/**
 * The shared static world (#184): `mmorpg-presentation` builds terrain, roads,
 * water and every prop model once in Rust, and both clients draw the same
 * draws. The native client lowers them to wgpu meshes; this module decodes the
 * WASM export (`mmorpg.static-scene` v1) into renderer nodes once per content
 * revision and per frame only hides far groups and places animated parts.
 */
export type CullClass = "always" | "far" | "mid" | "near";

/** Beyond this camera distance (metres, to the group's bounding circle) a class is hidden. */
export const CULL_DISTANCE_METRES: Readonly<Record<CullClass, number>> = {
  always: Number.POSITIVE_INFINITY,
  far: 420,
  mid: 150,
  near: 75,
};

/** What the WASM module exposes about the shared static world. */
export type StaticSceneContent = {
  staticScene(): string;
  staticSceneVertices(): Float32Array;
  staticSceneIndices(): Uint32Array;
  staticSceneModels(): Float32Array;
  staticSceneMotion(seconds: number, animate: boolean): Float32Array;
  staticSceneShown(viewer: Float64Array, outpostKit: boolean, settlementProps: boolean): Uint8Array;
  surfaceY(x: number, z: number): number;
};

/** Which asset-tooling kits the client draws: the Outpost kit buildings (#420) and the market square's settlement props (#421). */
export type DrawnKits = { outpost: boolean; settlementProps: boolean };
const NO_KITS: DrawnKits = { outpost: false, settlementProps: false };

type Bounds ={ cull: CullClass; center: readonly [number, number]; radius: number };
export type StaticBatch = Bounds & { node: RendererSceneNode & { geometry: IndexedMeshGeometry } };
export type InstanceBatch = Bounds & { batch: RendererInstanceBatch };

// Animated props use the default lit material; retain that branch of the renderer contract.
type AnimatedNode = Omit<Extract<RendererSceneNode, { unlit?: false }>, "transform" | "modelMatrix">;

export type SceneryScene = {
  batches: readonly StaticBatch[];
  instanceBatches: readonly InstanceBatch[];
  water: readonly RendererSceneNode[];
  /** Nodes of each animated part; `motion` places them. */
  animated: readonly (readonly AnimatedNode[])[];
  /** One column-major matrix (16 floats) per animated part at a cosmetic clock. */
  motion(seconds: number, animate: boolean): Float32Array;
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
const CULLS: readonly CullClass[] = ["always", "far", "mid", "near"];

type GeometryEntry = { resourceKey: string; vertexOffset: number; vertexCount: number; colored: boolean; indexOffset: number; indexCount: number };
type DrawEntry = { id: string; geometry: number; model: number; color: Color; emissive: Color | null; opacity: number };
type GroupEntry = { cull: CullClass; instanced: boolean; min: [number, number]; max: [number, number]; draws: DrawEntry[] };
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

function bounds(group: GroupEntry): Bounds {
  const [minX, minZ] = group.min;
  const [maxX, maxZ] = group.max;
  return { cull: group.cull, center: [(minX + maxX) / 2, (minZ + maxZ) / 2], radius: Math.hypot(maxX - minX, maxZ - minZ) / 2 };
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
  const geometries = layout.geometries.map((entry): IndexedMeshGeometry => {
    const { vertexOffset: start, vertexCount: count, indexOffset, indexCount } = entry;
    const end = start + count * (entry.colored ? 9 : 6);
    if (end > vertices.length || indexOffset + indexCount > indices.length) {
      fail(`geometry ${entry.resourceKey} exceeds the shared buffers`);
    }
    const geometry: IndexedMeshGeometry = {
      kind: "mesh",
      resourceKey: entry.resourceKey,
      positions: triples(vertices, start, count),
      normals: triples(vertices, start + count * 3, count),
      indices: Array.from(indices.subarray(indexOffset, indexOffset + indexCount)),
    };
    return entry.colored ? { ...geometry, colors: triples(vertices, start + count * 6, count) } : geometry;
  });
  const geometry = (draw: DrawEntry) => geometries[draw.geometry] ?? fail(`draw ${draw.id} names a missing geometry`);
  const model = (draw: DrawEntry): Matrix4Values => {
    if ((draw.model + 1) * 16 > models.length) {
      fail(`draw ${draw.id} names a missing model matrix`);
    }
    return Array.from(models.subarray(draw.model * 16, draw.model * 16 + 16)) as Matrix4Values;
  };
  const batches: StaticBatch[] = [];
  const instanceBatches: InstanceBatch[] = [];
  for (const group of layout.groups) {
    if (!CULLS.includes(group.cull) || group.draws.length === 0) {
      fail("group has an unknown cull class or no draws");
    }
    if (!group.instanced) {
      const [draw] = group.draws;
      batches.push({
        ...bounds(group),
        node: {
          id: draw!.id,
          geometry: geometry(draw!),
          color: draw!.color,
          modelMatrix: model(draw!),
          ...(draw!.emissive ? { unlit: false, emissive: draw!.emissive } : {}),
        },
      });
      continue;
    }
    const first = group.draws[0]!;
    const instances: RendererInstance[] = group.draws.map((draw) => ({ modelMatrix: model(draw), color: draw.color }));
    instanceBatches.push({
      ...bounds(group),
      // Instance draw IDs are `<batch id>:<ordinal>`; the batch shares one geometry.
      batch: { id: first.id.slice(0, first.id.lastIndexOf(":")), geometry: geometry(first), color: "#ffffff", revision: layout.prefix, instances },
    });
  }
  const water = layout.water.map((draw): RendererSceneNode => ({
    id: draw.id, geometry: geometry(draw), color: draw.color, opacity: draw.opacity, modelMatrix: model(draw),
  }));
  const animated = layout.animated.map((part) => part.map((draw): AnimatedNode => ({ id: draw.id, geometry: geometry(draw), color: draw.color })));
  const byKind: Partial<Record<PropKind, number>> = {};
  for (const prop of scenery.props) {
    byKind[prop.kind] = (byKind[prop.kind] ?? 0) + 1;
  }
  return {
    batches,
    instanceBatches,
    water,
    animated,
    motion: (seconds, animate) => content.staticSceneMotion(seconds, animate),
    shown: (viewer, kits) => content.staticSceneShown(Float64Array.from(viewer ?? []), kits.outpost, kits.settlementProps),
    stats: {
      staticNodes: batches.length + instanceBatches.length,
      staticVertices: layout.geometries.reduce((sum, entry) => sum + entry.vertexCount, 0),
      staticTriangles: layout.geometries.reduce((sum, entry) => sum + entry.indexCount / 3, 0),
      props: byKind,
    },
    surfaceY: (x, z) => content.surfaceY(x, z),
  };
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
export type SceneryMotion = { seconds: number; animate: boolean; viewer?: readonly [number, number, number] | null; kits?: DrawnKits };

/** Per-frame renderer nodes: static batches (far detail hidden), water and animated parts. */
export class SceneryFrame {
  readonly #scene: SceneryScene;
  readonly #visible: RendererSceneNode[];
  readonly #hidden: RendererSceneNode[];

  constructor(scene: SceneryScene) {
    this.#scene = scene;
    this.#visible = scene.batches.map((batch) => batch.node);
    this.#hidden = scene.batches.map((batch) => ({ ...batch.node, visible: false }));
  }

  nodes(eye: readonly [number, number, number], motion: SceneryMotion): { nodes: RendererSceneNode[]; instanceBatches: RendererInstanceBatch[]; visibleBatches: number } {
    const nodes: RendererSceneNode[] = [];
    let visibleBatches = 0;
    this.#scene.batches.forEach((batch, index) => {
      const visible = batchVisible(batch, eye[0], eye[2]);
      visibleBatches += visible ? 1 : 0;
      nodes.push(visible ? this.#visible[index]! : this.#hidden[index]!);
    });
    nodes.push(...this.#scene.water);
    const matrices = this.#scene.motion(motion.seconds, motion.animate);
    const shown = this.#scene.shown(motion.viewer ?? null, motion.kits ?? NO_KITS);
    this.#scene.animated.forEach((part, index) => {
      const modelMatrix = Array.from(matrices.subarray(index * 16, index * 16 + 16)) as Matrix4Values;
      const visible = shown[index] !== 0;
      for (const node of part) {
        nodes.push(visible ? { ...node, modelMatrix } : { ...node, modelMatrix, visible: false });
      }
    });
    const instanceBatches = this.#scene.instanceBatches.map((batch) => {
      const visible = batchVisible(batch, eye[0], eye[2]);
      visibleBatches += visible ? 1 : 0;
      return { ...batch.batch, visible };
    });
    return { nodes, instanceBatches, visibleBatches };
  }
}
