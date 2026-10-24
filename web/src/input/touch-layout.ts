import type { Axis } from "../command-wire";
import { sameEntity, type EntityRef } from "../entity-ref";
import type { ZoneSnapshot } from "../replication";
import type { HeldIntent } from "../world/orbit-camera";
import type { GameAction } from "./game-controls";

/**
 * Touch controls (#434) are a presentation of the existing semantic actions: every button fires a
 * `GameAction` that the keyboard and gamepad already resolve to, so the zone receives the same
 * validated intents. Nothing here decides a gameplay outcome.
 */

/** Face buttons in hotbar order: face 1 (bottom), 2 (right), 3 (left), 4 (top), like A/B/X/Y. */
export const FACE_BUTTONS = ["south", "east", "west", "north"] as const;
export type FaceButton = (typeof FACE_BUTTONS)[number];

/**
 * The 2-tap chord pages: a single face tap fires page 0; the modifier, then a face tap, fires
 * page 1. Together they reach all eight hotbar slots (modifier, then face 2, fires ability 6).
 */
export const TOUCH_CHORD_PAGES: readonly (readonly [GameAction, GameAction, GameAction, GameAction])[] = [
  ["ability.slot1", "ability.slot2", "ability.slot3", "ability.slot4"],
  ["ability.slot5", "ability.slot6", "ability.slot7", "ability.slot8"],
];

/** An armed modifier that sees no face tap lapses after this long. */
export const CHORD_TIMEOUT_MS = 2500;

/** The modifier-then-face chord: which page the next face tap fires. */
export class ChordState {
  #armedAt: number | null = null;

  /** The page the next face tap fires at `now`. */
  page(now: number): number {
    if (this.#armedAt !== null && now - this.#armedAt > CHORD_TIMEOUT_MS) {
      this.#armedAt = null;
    }
    return this.#armedAt === null ? 0 : 1;
  }

  /** A modifier tap arms the next page; tapping it again disarms. */
  toggleModifier(now: number): void {
    this.#armedAt = this.page(now) === 0 ? now : null;
  }

  /** A face tap fires the current page's action and returns to the first page. */
  press(face: number, now: number): GameAction | null {
    const action = TOUCH_CHORD_PAGES[this.page(now)]?.[face] ?? null;
    this.#armedAt = null;
    return action;
  }

  reset(): void {
    this.#armedAt = null;
  }
}

/** A thumbstick direction counts once its component passes about sin 22.5°: eight sectors. */
const SECTOR = 0.38;
/** Inside this fraction of the stick radius it is centred. */
export const STICK_DEADZONE = 0.25;

/**
 * Held movement from the virtual thumbstick (x right, y down, in stick radii) and a held jump
 * button, which ascends while swimming like held Space. Axes stay the wire's −1/0/1.
 */
export function stickIntent(x: number, y: number, jumpHeld: boolean): HeldIntent {
  const length = Math.hypot(x, y);
  const centred = !(length > STICK_DEADZONE);
  const nx = centred ? 0 : x / length;
  const ny = centred ? 0 : y / length;
  const forward: Axis = ny < -SECTOR ? 1 : ny > SECTOR ? -1 : 0;
  const strafe: Axis = nx > SECTOR ? 1 : nx < -SECTOR ? -1 : 0;
  return { forward, strafe, steering: forward !== 0 || strafe !== 0, rise: jumpHeld ? 1 : 0 };
}

/** Keyboard/gamepad and touch held movement together; either device may steer. */
export function mergeHeld(keys: HeldIntent, touch: HeldIntent): HeldIntent {
  const pick = (left: Axis, right: Axis): Axis => (left !== 0 ? left : right);
  return {
    forward: pick(keys.forward, touch.forward),
    strafe: pick(keys.strafe, touch.strafe),
    steering: keys.steering || touch.steering,
    rise: pick(keys.rise ?? 0, touch.rise ?? 0),
  };
}

/** What the context interact button does here; `null` hides it. */
export type InteractKind = "loot" | "vendor" | "talk" | "open" | "repair";

export type TouchContext = {
  /** A living attackable opponent is selected or near: show target and attack. */
  combat: boolean;
  dead: boolean;
  interact: InteractKind | null;
  /** The NPC an interact talks to or trades with. */
  npc: EntityRef | null;
};

/** Reach in zone units, mirroring `mmorpg-core` (the zone still checks every intent). */
export const LOOT_REACH_UNITS = 300;
export const NPC_REACH_UNITS = 500;
/** Opponents this close (30 m) bring up the combat buttons. */
export const COMBAT_NEAR_UNITS = 3_000;

export type TouchContextInputs = {
  /** NPC ids that are vendors. */
  isVendor: (npc: number) => boolean;
  doorInReach: boolean;
  repairInReach: boolean;
};

/**
 * Which context buttons to show from the received projection: loot before an NPC before a door
 * before a repair, and combat whenever an opponent is selected, near, or the viewer is fighting.
 */
export function touchContext(snapshot: ZoneSnapshot | null, inputs: TouchContextInputs): TouchContext {
  const viewer = snapshot?.entities[0];
  if (!snapshot || !viewer) {
    return { combat: false, dead: false, interact: null, npc: null };
  }
  const distance = (position: readonly number[]) =>
    Math.hypot((position[0] ?? 0) - viewer.position[0], (position[2] ?? 0) - viewer.position[2]);
  const others = snapshot.entities.slice(1);
  const opponents = others.filter((entity) => entity.flags.attackable && !entity.flags.dead);
  const combat = snapshot.viewer.inCombat
    || opponents.some((entity) => sameEntity(snapshot.viewer.target, { kind: entity.kind, id: entity.entityId }))
    || opponents.some((entity) => (entity.flags.hostile || entity.flags.targetsViewer) && distance(entity.position) <= COMBAT_NEAR_UNITS);
  if (snapshot.viewer.dead) {
    return { combat: false, dead: true, interact: null, npc: null };
  }
  const corpse = others.some((entity) => entity.kind === "creature" && entity.flags.lootable && !entity.flags.tappedByOther
    && distance(entity.position) <= LOOT_REACH_UNITS);
  const npc = others
    .filter((entity) => entity.kind === "npc" && distance(entity.position) <= NPC_REACH_UNITS)
    .sort((left, right) => distance(left.position) - distance(right.position) || left.entityId - right.entityId)[0];
  const npcRef: EntityRef | null = npc ? { kind: "npc", id: npc.entityId } : null;
  const interact: InteractKind | null = corpse ? "loot"
    : npc ? (inputs.isVendor(npc.entityId) ? "vendor" : "talk")
      : inputs.doorInReach ? "open" : inputs.repairInReach ? "repair" : null;
  return { combat, dead: false, interact, npc: npcRef };
}

export const INTERACT_LABELS: Readonly<Record<InteractKind, string>> = {
  loot: "Loot",
  vendor: "Trade",
  talk: "Talk",
  open: "Open",
  repair: "Repair",
};
