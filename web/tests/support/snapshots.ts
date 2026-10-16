import type { EntityFlags, EntityState, QuestSheet, ViewerState, Vector3, ZoneSnapshot } from "../../src/replication";

export const NO_FLAGS: EntityFlags = {
  dead: false, inCombat: false, hostile: false, attackable: false, tappedByOther: false, evading: false, targetsViewer: false, lootable: false,
};

/** A healthy level-1 player record. */
export function playerEntity(entityId: number, position: Vector3, velocity: Vector3 = [0, 0, 0], facing = 0): EntityState {
  return { kind: "player", entityId, appearance: 0, position, velocity, facing, level: 1, healthPercent: 100, flags: NO_FLAGS };
}

export const HEALTHY_VIEWER: ViewerState = {
  copper: 0, health: 50, maxHealth: 50, experience: 0, experienceToNextLevel: 100, level: 1, dead: false, inCombat: false, autoAttacking: false, target: null,
  classChoice: null, resource: null, cast: null, globalCooldown: 0, damage: { min: 3, max: 6 },
};

/** Nothing equipped. */
export const NO_EQUIPMENT: readonly null[] = Array<null>(6).fill(null);
export const NO_STATS = { stamina: 0, strength: 0, agility: 0, intellect: 0 } as const;
/** No active, turned-in or marked quests. */
export const NO_QUESTS: QuestSheet = { completed: 0, entries: [], markers: [] };

/** A projection with a healthy viewer, no target and no events. */
export function testSnapshot(
  fields: Pick<ZoneSnapshot, "zoneId" | "tick" | "contentRevision" | "acknowledgedSequence" | "viewerId" | "entities"> &
    Partial<Pick<ZoneSnapshot, "viewer" | "targetOfTarget" | "events" | "inventoryRevision" | "inventory" | "equipment" | "stats" | "loot" | "cooldowns" | "auras" | "targetDetail" | "quests">>,
): ZoneSnapshot {
  return {
    loot: null, inventoryRevision: 1n, inventory: null, equipment: null, stats: null, viewer: HEALTHY_VIEWER, targetOfTarget: null, events: [], chat: [],
    cooldowns: [], auras: [], targetDetail: { cast: null, auras: [] },
    // The quest sheet travels exactly with the bag sheet.
    quests: fields.inventory ? NO_QUESTS : null, ...fields,
  };
}
