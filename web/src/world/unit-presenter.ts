import type { IndexedMeshGeometry, Matrix4Values, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { EntityKind, EntityRef, EntityState } from "../replication";
import type { Color } from "./scenery";

/**
 * The shared unit presentation (#184): `mmorpg-presentation` draws procedural players,
 * creatures and NPCs, their locomotion and action poses, spell visuals, health bars and
 * target rings, and picks each imported model's clip, exactly as the native client does.
 * This adapter feeds it the received projections and the sampled units, turns its draws into
 * renderer nodes and keeps the geometry of resources still in use.
 */
export type UnitPresenterHandle = {
  modelReady(kind: string, clips: string): void;
  push(projection: Uint8Array, seconds: number): void;
  reset(): void;
  frame(units: Float64Array, seconds: number, dt: number, reducedMotion: boolean, barYaw: number,
    choice: number, headwear: string, previewYaw: number): string;
  models(): Float32Array;
  vertices(): Float32Array;
  indices(): Uint32Array;
};

export type ImportedModelKind = "Ranger" | "Wolf" | "Rat" | "Pig";

/** Where a presented unit stands, in metres and radians. */
export type PresentedUnit = {
  entity: EntityRef;
  /** `unit-<kind>-<id>`, the prefix of its node IDs. */
  identity: string;
  feet: readonly [number, number, number];
  centreY: number;
  yaw: number;
  /** Nameplate anchor above its health bar. */
  label: readonly [number, number, number];
  /** Its procedural body nodes (empty for an imported model). */
  body: readonly RendererSceneNode[];
};

/** An imported model to skin this frame, with the clip and time the shared presenter chose. */
export type ImportedRequest = {
  id: string;
  kind: ImportedModelKind;
  entity: EntityRef;
  feet: readonly [number, number, number];
  yaw: number;
  heightMetres: number;
  clip: string;
  time: number;
  /** Corpse darkening. */
  tint: number | null;
  /** A Warden carries the bundle's sword and shield (#156). */
  swordAndShield: boolean;
  /** A Ranger carries the bundle's bow and quiver on its back (#244). */
  bowAndQuiver: boolean;
  /** A Rogue carries the bundle's sword alone as its blade (#302). */
  blade: boolean;
  /** The living viewer: its imported model wears the viewer's chosen headwear (#374). */
  headwear: boolean;
};

export type UnitFrameNodes = { nodes: RendererSceneNode[]; units: PresentedUnit[]; imported: ImportedRequest[] };

/** The viewer's own look: class and sex as an appearance code, and headwear. */
export type ViewerLook = {
  classId: "warden" | "ranger" | "arcanist" | "necromancer" | "rogue" | null; sex: "female" | "male"; headwear: string | null;
  /** The viewer's hairstyle and colour (#376); hidden under the hood and helm. */
  hair?: "buzzed" | "parted" | "long" | "bald"; hairColor?: "black" | "brown" | "blond";
};

type DrawEntry = { id: string; key: string; model: number; color: Color; emissive: Color | null; opacity: number; unlit: boolean; doubleSided: boolean };
type GeometryEntry = { key: string; vertexOffset: number; vertexCount: number; colored: boolean; indexOffset: number; indexCount: number };
type UnitEntry = { kind: EntityKind; entityId: number; node: string; feet: [number, number, number]; centreY: number; yaw: number; label: [number, number, number]; body: [number, number] };
type ImportedEntry = { id: string; kind: ImportedModelKind; entityKind: EntityKind; entityId: number; feet: [number, number, number]; yaw: number;
  height: number; clip: string; time: number; tint: number | null; swordAndShield: boolean; bowAndQuiver: boolean; blade: boolean; headwear: boolean };
type Layout = { draws: DrawEntry[]; geometries: GeometryEntry[]; units: UnitEntry[]; imported: ImportedEntry[] };

const KIND_CODE: Readonly<Record<EntityKind, number>> = { player: 0, creature: 1, npc: 2 };
const CLASS_CODE = { warden: 0, ranger: 1, arcanist: 2, necromancer: 3, rogue: 4 } as const;

function triples(values: Float32Array, start: number, count: number): [number, number, number][] {
  const out: [number, number, number][] = new Array(count);
  for (let index = 0; index < count; index += 1) {
    const at = start + index * 3;
    out[index] = [values[at]!, values[at + 1]!, values[at + 2]!];
  }
  return out;
}

/** The appearance code `1 + class × 2 + sex` the shared presenter reads, 0 for no class. */
export function appearanceCode(look: Pick<ViewerLook, "classId" | "sex">): number {
  return look.classId === null ? 0 : 1 + CLASS_CODE[look.classId] * 2 + (look.sex === "male" ? 1 : 0);
}

export class SharedUnits {
  readonly #handle: UnitPresenterHandle;
  /** Geometry of the resources drawn in the previous call, which the presenter will not resend. */
  #geometries = new Map<string, IndexedMeshGeometry>();

  constructor(handle: UnitPresenterHandle) {
    this.#handle = handle;
  }

  /** An imported model finished loading, with its clip lengths in seconds. */
  modelReady(kind: ImportedModelKind, clips: readonly (readonly [string, number])[]): void {
    this.#handle.modelReady(kind, JSON.stringify(clips));
  }

  push(projection: Uint8Array, seconds: number): void {
    this.#handle.push(projection, seconds);
  }

  reset(): void {
    this.#handle.reset();
    this.#geometries = new Map();
  }

  frame(units: readonly EntityState[], options: { seconds: number; dt: number; reducedMotion: boolean; barYaw: number;
    look: ViewerLook; previewYaw?: number }): UnitFrameNodes {
    const input = new Float64Array(units.length * 6);
    units.forEach((entity, index) => {
      input.set([KIND_CODE[entity.kind], entity.entityId, entity.position[0], entity.position[1], entity.position[2], entity.facing], index * 6);
    });
    const layout = JSON.parse(this.#handle.frame(input, options.seconds, options.dt, options.reducedMotion, options.barYaw,
      appearanceCode(options.look), options.look.headwear ?? "", options.previewYaw ?? Number.NaN)) as Layout;
    const nodes = this.#nodes(layout, new Map());
    return {
      nodes,
      units: layout.units.map((unit) => ({
        entity: { kind: unit.kind, id: unit.entityId },
        identity: unit.node,
        feet: unit.feet,
        centreY: unit.centreY,
        yaw: unit.yaw,
        label: unit.label,
        body: nodes.slice(unit.body[0], unit.body[1]),
      })),
      imported: layout.imported.map((unit) => ({
        id: unit.id, kind: unit.kind, entity: { kind: unit.entityKind, id: unit.entityId }, feet: unit.feet, yaw: unit.yaw,
        heightMetres: unit.height, clip: unit.clip, time: unit.time, tint: unit.tint, swordAndShield: unit.swordAndShield, bowAndQuiver: unit.bowAndQuiver, blade: unit.blade, headwear: unit.headwear,
      })),
    };
  }



  /** Renderer nodes of one call; `kept` collects the geometry still in use. */
  #nodes(layout: Layout, kept: Map<string, IndexedMeshGeometry>): RendererSceneNode[] {
    const models = this.#handle.models();
    const vertices = layout.geometries.length > 0 ? this.#handle.vertices() : new Float32Array();
    const indices = layout.geometries.length > 0 ? this.#handle.indices() : new Uint32Array();
    const fresh = new Map<string, IndexedMeshGeometry>();
    for (const entry of layout.geometries) {
      const { vertexOffset: start, vertexCount: count } = entry;
      const geometry: IndexedMeshGeometry = {
        kind: "mesh", resourceKey: entry.key,
        positions: triples(vertices, start, count),
        normals: triples(vertices, start + count * 3, count),
        indices: Array.from(indices.subarray(entry.indexOffset, entry.indexOffset + entry.indexCount)),
      };
      fresh.set(entry.key, entry.colored ? { ...geometry, colors: triples(vertices, start + count * 6, count) } : geometry);
    }
    const nodes = layout.draws.map((draw): RendererSceneNode => {
      const geometry = fresh.get(draw.key) ?? this.#geometries.get(draw.key) ?? kept.get(draw.key);
      if (!geometry) {
        throw new Error(`The shared unit presenter drew ${draw.key} without its geometry.`);
      }
      kept.set(draw.key, geometry);
      const modelMatrix = Array.from(models.subarray(draw.model * 16, draw.model * 16 + 16)) as Matrix4Values;
      const shading = draw.emissive
        ? { unlit: false as const, emissive: draw.emissive }
        : draw.unlit ? { unlit: true as const } : {};
      return {
        id: draw.id, geometry, color: draw.color, modelMatrix, ...shading,
        ...(draw.opacity < 1 ? { opacity: draw.opacity } : {}),
        ...(draw.doubleSided ? { doubleSided: true } : {}),
      };
    });
    if (kept !== this.#geometries) {
      this.#geometries = kept;
    }
    return nodes;
  }
}
