import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import type { EntityKind, EntityRef, EntityState } from "../replication";
import type { ContentCatalog } from "./catalog";
import { UnitAnimator, poseFor, type LocomotionState } from "./character-animation";
import { archerNodes } from "./archer-asset";
import { OTHER_PLAYER_LOOK, humanoidHeadwearNodes, humanoidNodes, humanoidStance, type HumanoidLook, type UnitPlacement } from "./humanoid";
import { applyActionPose } from "./action-pose";
import type { ViewerAction } from "./units/spell-effects";
import { CREATURE_MODEL } from "./units/creature-models";

/**
 * Unit rendering by projected entity. A registry maps each entity kind to a
 * model, and the model reads the whole projected entity, so it can pick a
 * body by appearance (template ID) and stand on its own half height. Players
 * are animated humanoids (Rangers use the packaged archer mesh); creatures and NPCs get a
 * procedural model by family or role (`units/creature-models.ts`), falling back to the
 * placeholder box of `units/creature-bodies.ts` for unknown templates.
 */
export type { UnitPlacement } from "./humanoid";

/** The local character's look. Projections carry no player appearance yet, so other players share one look. */
export type UnitLook = HumanoidLook;

/** What every unit model may read besides the entity itself. */
export type UnitContext = {
  unitsPerMetre: number;
  /** Half the height of the shared player body box, in units. */
  playerHalfHeightUnits: number;
  /** The player this projection is addressed to. */
  viewerId: number;
  viewerLook: UnitLook;
  /** Creature template and NPC facts for the IDs projections carry. */
  catalog: ContentCatalog;
  /** The viewer's current target, which its model marks. */
  viewerTarget: EntityRef | null;
  /** The viewer's cast, draw or swing, which its humanoid plays over its locomotion. */
  viewerAction?: ViewerAction | null;
};

/** Everything a unit model draws from in one frame. */
export type UnitFrame = {
  id: string;
  entity: EntityState;
  placement: UnitPlacement;
  locomotion: LocomotionState;
  context: UnitContext;
};

export type UnitModel = {
  /** Half the body height in units: a projection places the body centre this far above the feet. */
  halfHeightUnits(entity: EntityState, context: UnitContext): number;
  nodes(frame: UnitFrame): RendererSceneNode[];
};

/** Models by entity kind; adding a kind to `EntityKind` requires an entry here. */
export type UnitModels = { readonly [kind in EntityKind]: UnitModel };

const YAW_STEPS = 65_536;
/** Presentation velocity arrives in units per tick. */
const TICKS_PER_SECOND = 30;

export function yawRadians(facing: number): number {
  return (facing / YAW_STEPS) * 2 * Math.PI;
}

/**
 * Converts a projected entity (body centre in units) into a placement. The
 * physics body centre sits `halfHeight` above the feet; relief is added on
 * top because walkable ground is physically flat.
 */
export function placeUnit(
  entity: EntityState,
  halfHeightUnits: number,
  reliefUnits: number,
  unitsPerMetre: number,
): UnitPlacement {
  return {
    x: entity.position[0] / unitsPerMetre,
    feetY: (entity.position[1] - halfHeightUnits + reliefUnits) / unitsPerMetre,
    z: entity.position[2] / unitsPerMetre,
    yawRadians: yawRadians(entity.facing),
  };
}

/** Players: the viewer's own unit wears the local character's look, others the shared one. */
export const PLAYER_MODEL: UnitModel = {
  halfHeightUnits: (_entity, context) => context.playerHalfHeightUnits,
  nodes: ({ id, entity, placement, locomotion, context }) => {
    const look = entity.entityId === context.viewerId ? context.viewerLook : OTHER_PLAYER_LOOK;
    const action = entity.entityId === context.viewerId ? context.viewerAction ?? null : null;
    if (look.visuals.weapon === "bow") {
      const base = poseFor(locomotion, "bow");
      const pose = applyActionPose(base, action);
      // The rigid archer mesh cannot bend its arms; the draw pose turns it side-on instead.
      return [...archerNodes(id, placement, look, pose.bob, pose.twist - base.twist), ...humanoidHeadwearNodes(id, placement, look, pose)];
    }
    return humanoidNodes(id, placement, look, applyActionPose(poseFor(locomotion, humanoidStance(look)), action));
  },
};

export const UNIT_MODELS: UnitModels = {
  player: PLAYER_MODEL,
  creature: CREATURE_MODEL,
  npc: CREATURE_MODEL,
};

/** The model that draws a projected entity. */
export function unitModel(entity: Pick<EntityState, "kind">, models: UnitModels = UNIT_MODELS): UnitModel {
  return models[entity.kind];
}

/** Where a unit stands: its model's half height below the projected body centre, raised by relief. */
export function placeWithModel(model: UnitModel, entity: EntityState, context: UnitContext, reliefUnits: number): UnitPlacement {
  return placeUnit(entity, model.halfHeightUnits(entity, context), reliefUnits, context.unitsPerMetre);
}

export function unitIdentity(entity: Pick<EntityState, "kind" | "entityId">): string {
  return `unit-${entity.kind}-${entity.entityId}`;
}

/** The IDs of the nodes one unit drew, given its `unitIdentity` (IDs are `<identity>-<part>`). */
export function unitNodeIds(nodes: readonly { id: string }[], identity: string): string[] {
  const prefix = `${identity}-`;
  return nodes.filter((node) => node.id.startsWith(prefix)).map((node) => node.id);
}

/** Animation state per visible unit, dropped when the unit leaves the projection. */
export class UnitAnimators {
  readonly #animators = new Map<string, UnitAnimator>();

  /** Locomotion for this frame; `instant` snaps blends (reduced motion). */
  locomotion(entity: EntityState, placement: UnitPlacement, unitsPerMetre: number, deltaSeconds: number, instant: boolean): LocomotionState {
    const identity = unitIdentity(entity);
    let animator = this.#animators.get(identity);
    if (!animator) {
      animator = new UnitAnimator();
      this.#animators.set(identity, animator);
    }
    const scale = TICKS_PER_SECOND / unitsPerMetre;
    return animator.update({
      x: placement.x,
      z: placement.z,
      velocity: [entity.velocity[0] * scale, entity.velocity[1] * scale, entity.velocity[2] * scale],
      facing: placement.yawRadians,
    }, deltaSeconds, instant);
  }

  /** Forgets units that are no longer visible. */
  retain(identities: ReadonlySet<string>): void {
    for (const identity of this.#animators.keys()) {
      if (!identities.has(identity)) {
        this.#animators.delete(identity);
      }
    }
  }

  clear(): void {
    this.#animators.clear();
  }
}
