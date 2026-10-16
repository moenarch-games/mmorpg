import type { Matrix4Values, RendererSceneNode, RendererInstanceBatch } from "@moritzbrantner/three-d-renderer";
import { ENVIRONMENT, type EnvironmentStyle } from "./environment";
import { SceneBatcher, batchVisible, type InstanceBatch, type StaticBatch } from "./mesh-batching";
import type { Vec3 } from "./mesh-builder";
import { appendPropModels, type AnimatedMesh, type AnimatedParts, type ModelContext } from "./prop-models";
import type { Color, Prop, PropKind, Scenery } from "./scenery";
import { appendLakeBeds, appendPatches, appendRoads, appendTerrain, terrainSurfaceY, waterNodes } from "./terrain-mesh";

/**
 * The static world of one scenery value, built once per content revision:
 * vertex-coloured terrain and far-ring meshes, road ribbons, the lake, and
 * procedural props. Vegetation and rocks share instance geometry; other static
 * parts merge by spatial chunk, colour and cull class. A few animated parts (windmill sails, campfire flames, reeds) stay
 * separate nodes whose transforms change every frame.
 */
export type SceneryScene = {
  batches: readonly StaticBatch[];
  instanceBatches: readonly InstanceBatch[];
  water: readonly RendererSceneNode[];
  animated: AnimatedParts;
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

const HUB_KINDS: ReadonlySet<PropKind> = new Set(["keep", "inn", "smithy", "house", "well", "waystone"]);

function resourcePrefix(scenery: Scenery): string {
  return `scenery:${scenery.source}:${scenery.contentRevision}:${scenery.presentationFingerprint}`;
}

/** Centroid of the samples of a named biome, in metres. */
function biomeCentroid(scenery: Scenery, name: string): readonly [number, number] | null {
  const id = scenery.biomes.find((biome) => biome.name === name)?.id;
  const { terrain, unitsPerMetre } = scenery;
  let sumX = 0;
  let sumZ = 0;
  let count = 0;
  terrain.biomes.forEach((biome, index) => {
    if (biome === id) {
      sumX += terrain.originXz[0] + (index % terrain.columns) * terrain.step;
      sumZ += terrain.originXz[1] + Math.floor(index / terrain.columns) * terrain.step;
      count += 1;
    }
  });
  return count === 0 ? null : [sumX / count / unitsPerMetre, sumZ / count / unitsPerMetre];
}

function nearestRoadPoint(scenery: Scenery, x: number, z: number): readonly [number, number] | null {
  let best: [number, number] | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  const metres = (units: number) => units / scenery.unitsPerMetre;
  for (const road of scenery.roads) {
    for (let index = 0; index + 1 < road.points.length; index += 1) {
      const [ax, az] = road.points[index]!.map(metres) as [number, number];
      const [bx, bz] = road.points[index + 1]!.map(metres) as [number, number];
      const dx = bx - ax;
      const dz = bz - az;
      const lengthSquared = dx * dx + dz * dz;
      const t = lengthSquared === 0 ? 0 : Math.min(1, Math.max(0, ((x - ax) * dx + (z - az) * dz) / lengthSquared));
      const px = ax + dx * t;
      const pz = az + dz * t;
      const distance = Math.hypot(px - x, pz - z);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = [px, pz];
      }
    }
  }
  return best;
}

export function buildSceneryScene(scenery: Scenery, style: EnvironmentStyle = ENVIRONMENT): SceneryScene {
  const prefix = resourcePrefix(scenery);
  const batcher = new SceneBatcher(prefix);
  const { unitsPerMetre, terrain } = scenery;
  const surfaceY = (x: number, z: number) => terrainSurfaceY(terrain, unitsPerMetre, x, z);
  appendTerrain(batcher, scenery, style);
  appendPatches(batcher, scenery, style);
  appendRoads(batcher, scenery, style);
  appendLakeBeds(batcher, scenery, style);
  const plaza = biomeCentroid(scenery, "plaza") ?? [0, 0];
  const metres = (prop: Prop): readonly [number, number] => [prop.position[0] / unitsPerMetre, prop.position[2] / unitsPerMetre];
  const gatePosts = scenery.props.filter((prop) => prop.kind === "gate-post").map(metres);
  const animated: AnimatedParts = { sails: [], flames: [], reeds: [] };
  const context: ModelContext = {
    batcher,
    style,
    unitsPerMetre,
    surface: surfaceY,
    doorTarget(prop) {
      const [x, z] = metres(prop);
      return HUB_KINDS.has(prop.kind) ? plaza : nearestRoadPoint(scenery, x, z) ?? plaza;
    },
    gatePartner(prop) {
      const [x, z] = metres(prop);
      let best: readonly [number, number] | null = null;
      let bestDistance = 7;
      for (const other of gatePosts) {
        const distance = Math.hypot(other[0] - x, other[1] - z);
        if (distance > 0.01 && distance < bestDistance) {
          best = other;
          bestDistance = distance;
        }
      }
      return best;
    },
    animated,
  };
  const { byKind } = appendPropModels(context, scenery.props);
  const emissive = new Set<Color>([style.palette.rune, style.palette.lampGlow, style.palette.windowLit, style.palette.fire, style.palette.fireCore]);
  const batches = batcher.batches().map((batch): StaticBatch => typeof batch.node.color === "string" && emissive.has(batch.node.color)
    ? { ...batch, node: { ...batch.node, unlit: false, emissive: batch.node.color } } : batch);
  const instanceBatches = batcher.instanceBatches();
  const geometries = new Set([...batches.map((batch) => batch.node.geometry), ...instanceBatches.map((batch) => batch.batch.geometry)]);
  return {
    batches,
    instanceBatches,
    water: waterNodes(scenery, style, prefix),
    animated,
    stats: {
      staticNodes: batches.length + instanceBatches.length,
      staticVertices: [...geometries].reduce((sum, geometry) => sum + (geometry.kind === "mesh" ? geometry.positions.length : 0), 0),
      staticTriangles: [...geometries].reduce((sum, geometry) => sum + (geometry.kind === "mesh" ? geometry.indices.length / 3 : 0), 0),
      props: byKind,
    },
    surfaceY,
  };
}

type Quaternion = [number, number, number, number];

function yawQuaternion(yaw: number): Quaternion {
  return [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)];
}

/** Rotation about Y by `yaw`, then about the (turned) Z axis by `roll`. */
export function yawRollQuaternion(yaw: number, roll: number): Quaternion {
  const sy = Math.sin(yaw / 2);
  const wy = Math.cos(yaw / 2);
  const sz = Math.sin(roll / 2);
  const wz = Math.cos(roll / 2);
  return [sy * sz, sy * wz, wy * sz, wy * wz];
}

// Animated props use the default lit material; retain that branch of the renderer contract.
type AnimatedNode = Omit<Extract<RendererSceneNode, { unlit?: false }>, "transform" | "modelMatrix">;

function meshNode(mesh: AnimatedMesh, prefix: string): AnimatedNode {
  return {
    id: mesh.id,
    geometry: { kind: "mesh", resourceKey: `${prefix}:${mesh.id}`, positions: mesh.positions, normals: mesh.normals, indices: mesh.indices },
    color: mesh.color,
  };
}

/** Cosmetic animation input; without motion every part holds its rest pose. */
export type SceneryMotion = { seconds: number; animate: boolean };

/** Per-frame renderer nodes: static batches (far detail hidden), water and animated parts. */
export class SceneryFrame {
  readonly #scene: SceneryScene;
  readonly #visible: RendererSceneNode[];
  readonly #hidden: RendererSceneNode[];
  readonly #animated: AnimatedNode[][];

  constructor(scene: SceneryScene, prefix: string) {
    this.#scene = scene;
    this.#visible = scene.batches.map((batch) => batch.node);
    this.#hidden = scene.batches.map((batch) => ({ ...batch.node, visible: false }));
    const { sails, flames, reeds } = scene.animated;
    this.#animated = [...sails, ...flames, ...reeds].map((part) => part.mesh.map((mesh) => meshNode(mesh, prefix)));
  }

  nodes(eye: Vec3, motion: SceneryMotion): { nodes: RendererSceneNode[]; instanceBatches: RendererInstanceBatch[]; visibleBatches: number } {
    const nodes: RendererSceneNode[] = [];
    let visibleBatches = 0;
    this.#scene.batches.forEach((batch, index) => {
      const visible = batchVisible(batch, eye[0], eye[2]);
      visibleBatches += visible ? 1 : 0;
      nodes.push(visible ? this.#visible[index]! : this.#hidden[index]!);
    });
    nodes.push(...this.#scene.water);
    const t = motion.animate ? motion.seconds : 0;
    const { sails, flames, reeds } = this.#scene.animated;
    let part = 0;
    const partNodes = () => this.#animated[part++] ?? [];
    for (const sail of sails) {
      const rotationQuaternion = yawRollQuaternion(sail.axisYaw, t * 0.45);
      for (const node of partNodes()) {
        nodes.push({ ...node, transform: { translation: [...sail.hub], rotationQuaternion } });
      }
    }
    for (const flame of flames) {
      const flicker: [number, number, number] = motion.animate
        ? [1 + 0.1 * Math.sin(t * 9.1 + flame.seed), 1 + 0.18 * Math.sin(t * 7.3 + flame.seed * 2) * Math.sin(t * 3.1), 1 + 0.1 * Math.cos(t * 8.3)]
        : [1, 1, 1];
      for (const node of partNodes()) {
        nodes.push({ ...node, transform: { translation: [...flame.base], scale: flicker, rotationQuaternion: yawQuaternion(t * 0.7) } });
      }
    }
    for (const reed of reeds) {
      const kx = motion.animate ? 0.045 * Math.sin(t * 1.1 + reed.phase) : 0;
      const kz = motion.animate ? 0.035 * Math.sin(t * 0.83 + reed.phase * 1.7) : 0;
      // Column-major shear about the ground: x += kx (y - ground), z += kz (y - ground).
      const modelMatrix: Matrix4Values = [1, 0, 0, 0, kx, 1, kz, 0, 0, 0, 1, 0, -kx * reed.groundY, 0, -kz * reed.groundY, 1];
      for (const node of partNodes()) {
        nodes.push({ ...node, modelMatrix });
      }
    }
    const instanceBatches = this.#scene.instanceBatches.map((batch) => {
      const visible = batchVisible(batch, eye[0], eye[2]);
      visibleBatches += visible ? 1 : 0;
      return { ...batch.batch, visible };
    });
    return { nodes, instanceBatches, visibleBatches };
  }
}

export { resourcePrefix as sceneryResourcePrefix };
