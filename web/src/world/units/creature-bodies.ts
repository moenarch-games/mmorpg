import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { sameEntity } from "../../entity-ref";
import type { EntityState } from "../../replication";
import type { ContentCatalog, CreatureFamily } from "../catalog";
import type { UnitModel, UnitPlacement } from "../unit-nodes";

/**
 * Placeholder bodies for creatures and NPCs until they get models of their
 * own: a box sized by the creature's collision box (a character-sized post
 * for NPCs), coloured by disposition (hostile red-ish, neutral yellow-ish,
 * friendly green-ish) and shaded by family, with a nose marker for facing.
 * Dead creatures lie flat. The viewer's target stands on a ring.
 */
type Color = `#${string}`;
type Quaternion = [number, number, number, number];

export type Disposition = "hostile" | "neutral" | "friendly";

const HOSTILE: Record<CreatureFamily, Color> = {
  wolf: "#b3533f", boar: "#b8452f", vermin: "#a84a3a", marauder: "#c23b2e", mirefin: "#a55a4c", redbrand: "#992d27",
};
const NEUTRAL: Record<CreatureFamily, Color> = {
  wolf: "#c9ab45", boar: "#c79f2f", vermin: "#bfae5a", marauder: "#d1b04a", mirefin: "#b7a553", redbrand: "#c2a03c",
};
const FRIENDLY: Color = "#4f9d5d";
const GUARD: Color = "#3d8a4c";
const TAPPED: Color = "#8b8b8b";
const CORPSE_SHADE = 0.45;
const TARGET_RING: Color = "#f2d15f";
/** NPC and fallback bodies: the character box, 0.6 m × 1.8 m. */
const CHARACTER_HALF_METRES: readonly [number, number, number] = [0.3, 0.9, 0.3];
const UNITS_PER_METRE = 100;

export function disposition(entity: EntityState): Disposition {
  if (entity.kind !== "creature") {
    return "friendly";
  }
  return entity.flags.hostile ? "hostile" : "neutral";
}

/** Half height in units of a unit's collision box, for placing its feet. */
export function bodyHalfHeightUnits(entity: EntityState, catalog: ContentCatalog, playerHalfHeight: number): number {
  if (entity.kind === "creature") {
    return catalog.creatureTemplates.get(entity.appearance)?.halfExtents[1] ?? playerHalfHeight;
  }
  return playerHalfHeight;
}

export function shade(color: Color, factor: number): Color {
  const value = Number.parseInt(color.slice(1), 16);
  const channel = (shift: number) => Math.round(((value >> shift) & 0xff) * factor);
  return `#${[16, 8, 0].map((shift) => channel(shift).toString(16).padStart(2, "0")).join("")}`;
}

/** The disposition colour of a creature, before the corpse shade: tapped grey, else hostile or neutral by family. */
export function stateColor(entity: EntityState, catalog: ContentCatalog): Color {
  if (entity.kind === "npc") {
    return catalog.npcs.get(entity.entityId)?.role === "guard" ? GUARD : FRIENDLY;
  }
  const family = catalog.creatureTemplates.get(entity.appearance)?.family ?? "wolf";
  return entity.flags.tappedByOther ? TAPPED : disposition(entity) === "hostile" ? HOSTILE[family] : NEUTRAL[family];
}

function bodyColor(entity: EntityState, catalog: ContentCatalog): Color {
  const base = stateColor(entity, catalog);
  return entity.kind === "creature" && entity.flags.dead ? shade(base, CORPSE_SHADE) : base;
}

/**
 * Renderer nodes for one creature or NPC; players are drawn by the player
 * model. IDs are unique per unit, so any number of units share a frame.
 */
export function creatureBodyNodes(
  entity: EntityState,
  placement: UnitPlacement,
  catalog: ContentCatalog,
  targeted: boolean,
): RendererSceneNode[] {
  if (entity.kind === "player") {
    return [];
  }
  const id = `unit-${entity.kind}-${entity.entityId}`;
  const template = entity.kind === "creature" ? catalog.creatureTemplates.get(entity.appearance) : undefined;
  const [halfX, halfY, halfZ] = template === undefined
    ? CHARACTER_HALF_METRES
    : [template.halfExtents[0] / UNITS_PER_METRE, template.halfExtents[1] / UNITS_PER_METRE,
      template.halfExtents[2] / UNITS_PER_METRE];
  const { x, z, feetY, yawRadians: yaw } = placement;
  const rotationQuaternion: Quaternion = [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)];
  const color = bodyColor(entity, catalog);
  const nodes: RendererSceneNode[] = [];
  if (targeted) {
    nodes.push({
      id: `${id}-target-ring`,
      geometry: { kind: "cylinder", radius: Math.max(halfX, halfZ) + 0.35, height: 0.04 },
      color: TARGET_RING,
      unlit: true,
      opacity: 0.85,
      transform: { translation: [x, feetY + 0.02, z] },
    });
  }
  if (entity.flags.dead) {
    // A corpse lies flat: its height becomes its length along the facing.
    nodes.push({
      id: `${id}-body`,
      geometry: { kind: "box", size: [halfX * 2, 0.16, halfY * 2] },
      color,
      transform: { translation: [x, feetY + 0.08, z], rotationQuaternion },
    });
    return nodes;
  }
  const centreY = feetY + halfY;
  nodes.push({
    id: `${id}-body`,
    geometry: { kind: "box", size: [halfX * 2, halfY * 2, halfZ * 2] },
    color,
    transform: { translation: [x, centreY, z], rotationQuaternion },
  });
  // The nose touches the front face at about three quarters of the height.
  const reach = halfZ + 0.08;
  nodes.push({
    id: `${id}-nose`,
    geometry: { kind: "box", size: [0.14, 0.14, 0.16] },
    color: shade(color, 0.55),
    transform: {
      translation: [x + Math.sin(yaw) * reach, feetY + halfY * 1.5, z + Math.cos(yaw) * reach],
      rotationQuaternion,
    },
  });
  return nodes;
}

/** The unit model for creatures and NPCs: feet on the ground under the collision box, the target ringed. */
export const PLACEHOLDER_BODY_MODEL: UnitModel = {
  halfHeightUnits: (entity, context) => bodyHalfHeightUnits(entity, context.catalog, context.playerHalfHeightUnits),
  nodes: ({ entity, placement, context }) => creatureBodyNodes(
    entity,
    placement,
    context.catalog,
    sameEntity({ kind: entity.kind, id: entity.entityId }, context.viewerTarget),
  ),
};
