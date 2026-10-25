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
  residentsReady(): void;
  setBlend(lowerSeconds: number, upperSeconds: number, lowerWeight: number, upperWeight: number): void;
  setBodyOverlay(shown: boolean): void;
  push(projection: Uint8Array, seconds: number): void;
  reset(): void;
  prepare(ability: number, seconds: number): void;
  preparedSent(seconds: number): void;
  abandonPreparation(): void;
  /** Sound cues and fades of the projections pushed since the last call (#447), as JSON. */
  takeAudio(): string;
  frame(units: Float64Array, seconds: number, dt: number, reducedMotion: boolean, barYaw: number,
    choice: number, headwear: string, previewYaw: number): string;
  models(): Float32Array;
  vertices(): Float32Array;
  indices(): Uint32Array;
};

export type ImportedModelKind = "Ranger" | "Wolf" | "Rat" | "Pig";
/** The shared presenter's `Loadout::name`s. */
export type Loadout = "sword-and-shield" | "bow-and-quiver" | "sword" | "dagger" | "staff.bone" | "staff.crystal-and-tome"
  | "mace.flanged" | "staff.gnarled";
/**
 * A class starter outfit (asset-tooling #379, the shared `Outfit`): the `class.<id>` appearance
 * preset and the armour sockets whose outfit pieces the viewer's equipped drops replace.
 */
export type ClassOutfit = { preset: string; replaced: readonly string[] };

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
  /** The upper body's target clip and time: what the unit is seen doing. */
  clip: string;
  time: number;
  /** The legs' target clip and time. */
  lower: readonly [string, number];
  /** The layered clips to pose (Atlas 6.6.45). */
  layers: readonly ClipLayer[];
  /** Linear RGB colour factor: corpse darkening and the companion's green (#296); null as authored. */
  tint: readonly [number, number, number] | null;
  /** What the unit carries: a player its class weapons (asset-tooling #375), a guard its sword. */
  loadout: Loadout | null;
  /** A named resident's `humanoid-appearance` preset id (#418). */
  resident: string | null;
  /** The Ranger figure a resident is drawn on, by its character key (`imported::Figure`, #493). */
  figure: string | null;
  /** A varied resident's look seed (its NPC id; asset-tooling #380): each guard is his own man. */
  variety: number | null;
  /** A player's class starter outfit (asset-tooling #379). */
  outfit: ClassOutfit | null;
  /** The living viewer: its imported model wears the viewer's chosen headwear (#374). */
  headwear: boolean;
  /**
   * How far its loose gear swings (Atlas 6.6.39): a model-frame quaternion `[x, y, z, w]` for the
   * items on hanging sockets, or null where they hang as authored.
   */
  swing: readonly [number, number, number, number] | null;
};

/** Which joints a layer drives: every joint, the `spine_01` subtree, or the rest (pelvis, legs). */
export type LayerMask = "all" | "upper" | "lower";
/** One weighted clip sample; a joint averages the layers that drive it, the rest of a total below 1 stays at rest. */
export type ClipLayer = { clip: string; time: number; weight: number; mask: LayerMask };

/** How the body regions blend (the Action Lab, #384): crossfade seconds and overlay weights, each 0–1. */
export type AnimationBlend = { lowerSeconds: number; upperSeconds: number; lowerWeight: number; upperWeight: number };
export const DEFAULT_BLEND: AnimationBlend = { lowerSeconds: 0.15, upperSeconds: 0.1, lowerWeight: 1, upperWeight: 1 };
/** The viewer's layered animation (#384): each region's target clip and time, and the posed layers. */
export type ViewerAnimation = { upper: readonly [string, number]; lower: readonly [string, number]; layers: readonly ClipLayer[] };

export type UnitFrameNodes = { nodes: RendererSceneNode[]; units: PresentedUnit[]; imported: ImportedRequest[] };

/** The viewer's own look: class and sex as an appearance code, and headwear. */
export type ViewerLook = {
  classId: "warden" | "ranger" | "arcanist" | "necromancer" | "rogue" | "cleric" | "druid" | null; sex: "female" | "male"; headwear: string | null;
  /** The viewer's hairstyle and colour (#376); hidden under the hood and helm. */
  hair?: "buzzed" | "parted" | "long" | "bald"; hairColor?: "black" | "brown" | "blond";
  /** The viewer's face (#411): bundle face feature id to option id. */
  face?: Readonly<Record<string, string>>;
};

type DrawEntry = { id: string; key: string; model: number; color: Color; emissive: Color | null; opacity: number; unlit: boolean; doubleSided: boolean };
type GeometryEntry = { key: string; vertexOffset: number; vertexCount: number; colored: boolean; indexOffset: number; indexCount: number };
type UnitEntry = { kind: EntityKind; entityId: number; node: string; feet: [number, number, number]; centreY: number; yaw: number; label: [number, number, number]; body: [number, number] };
type ImportedEntry = { id: string; kind: ImportedModelKind; entityKind: EntityKind; entityId: number; feet: [number, number, number]; yaw: number;
  height: number; clip: string; time: number; lower: [string, number]; layers: ClipLayer[]; tint: readonly [number, number, number] | null; loadout: Loadout | null; resident: string | null; figure: string | null; variety: number | null; outfit: ClassOutfit | null; headwear: boolean;
  swing: readonly [number, number, number, number] | null };
type Layout = { draws: DrawEntry[]; geometries: GeometryEntry[]; units: UnitEntry[]; imported: ImportedEntry[] };

const KIND_CODE: Readonly<Record<EntityKind, number>> = { player: 0, creature: 1, npc: 2, minion: 3 };
const CLASS_CODE = { warden: 0, ranger: 1, arcanist: 2, necromancer: 3, rogue: 4, cleric: 5, druid: 6 } as const;

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

  /** The residents' appearance presets loaded (#418): named residents become Rangers. */
  residentsReady(): void {
    this.#handle.residentsReady();
  }

  /** Tunes the layered blend; the presenter clamps it. Presentation only. */
  setBlend(blend: AnimationBlend): void {
    this.#handle.setBlend(blend.lowerSeconds, blend.upperSeconds, blend.lowerWeight, blend.upperWeight);
  }

  /** Shows or hides the development body overlay (#39): each unit's authoritative body over its model. */
  setBodyOverlay(shown: boolean): void {
    this.#handle.setBodyOverlay(shown);
  }

  push(projection: Uint8Array, seconds: number): void {
    this.#handle.push(projection, seconds);
  }

  reset(): void {
    this.#handle.reset();
    this.#geometries = new Map();
  }

  /**
   * The local player pressed or began aiming an attack (Atlas 6.6.10): the viewer winds up at
   * once and holds short of the swing until the zone accepts (the swing continues) or refuses
   * (the wind-up fades). Presentation only; the zone still decides every outcome.
   */
  prepare(ability: number, seconds: number): void {
    this.#handle.prepare(ability, seconds);
  }

  /** The prepared input went to the zone; an unanswered wind-up lets go shortly after. */
  preparedSent(seconds: number): void {
    this.#handle.preparedSent(seconds);
  }

  /** Blur, menus, a retired input or a superseding gesture: no swing. */
  abandonPreparation(): void {
    this.#handle.abandonPreparation();
  }

  /** The shared presenter's sound operations since the last call, for `WorldSound.apply`. */
  takeAudio(): string {
    return this.#handle.takeAudio();
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
        heightMetres: unit.height, clip: unit.clip, time: unit.time, lower: unit.lower, layers: unit.layers, tint: unit.tint, loadout: unit.loadout, resident: unit.resident, figure: unit.figure, variety: unit.variety, outfit: unit.outfit, headwear: unit.headwear,
        swing: unit.swing,
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
