import { sameEntity } from "../../entity-ref";
import { GLOBAL_COOLDOWN_TICKS, TICK_HZ, UNITS_PER_METRE, findEntity, type ResourceKind, type ZoneSnapshot } from "../../replication";
import type { AbilityRecord, ContentCatalog } from "../catalog";
import { presentationOf, reachUnits, type AbilityRange } from "./ability-presentation";

/**
 * What an action bar slot shows, derived only from the decoded projection, the exported catalog
 * and the distance to the target. None of it decides whether a use succeeds; the zone answers
 * every `UseAbility` itself.
 */
export type SlotCooldown = { kind: "cooldown" | "gcd"; remaining: number; total: number };

export type SlotState = {
  ability: AbilityRecord;
  learned: boolean;
  /** Level at which the class learns the ability. */
  unlockLevel: number;
  /** The class resource is below the cost. */
  resourceShort: boolean;
  /** The target is known and farther than the ability reaches. */
  outOfRange: boolean;
  /** The viewer is casting or channelling this ability. */
  casting: boolean;
  cooldown: SlotCooldown | null;
};

/**
 * XZ centre distance in core units from the viewer to its target, or null without a target (or
 * when targeting itself). A selected target the projection omits has left the interest radius,
 * beyond every targeted ability's reach, so it counts as infinitely far.
 */
export function targetDistanceUnits(snapshot: ZoneSnapshot): number | null {
  const selected = snapshot.viewer.target;
  const viewer = findEntity(snapshot, { kind: "player", id: snapshot.viewerId });
  if (selected === null || !viewer || sameEntity(selected, { kind: "player", id: snapshot.viewerId })) {
    return null;
  }
  const target = findEntity(snapshot, selected);
  if (!target) {
    return Number.POSITIVE_INFINITY;
  }
  return Math.hypot(target.position[0] - viewer.position[0], target.position[2] - viewer.position[2]);
}

export function slotState(ability: AbilityRecord, snapshot: ZoneSnapshot, distanceUnits: number | null): SlotState {
  const viewer = snapshot.viewer;
  const range: AbilityRange = presentationOf(ability.id)?.range ?? "self";
  const reach = reachUnits(range);
  const running = snapshot.cooldowns.find((cooldown) => cooldown.ability === ability.id)?.remaining ?? 0;
  const global = viewer.globalCooldown;
  // The longer wait is the one the player is waiting on.
  const cooldown: SlotCooldown | null = running > 0 && running >= global
    ? { kind: "cooldown", remaining: running, total: ability.cooldown }
    : global > 0
      ? { kind: "gcd", remaining: global, total: GLOBAL_COOLDOWN_TICKS }
      : null;
  return {
    ability,
    learned: viewer.level >= ability.level,
    unlockLevel: ability.level,
    resourceShort: viewer.resource !== null && viewer.resource.value < ability.cost,
    outOfRange: reach !== null && distanceUnits !== null && distanceUnits > reach,
    casting: viewer.cast?.ability === ability.id,
    cooldown,
  };
}

/** Degrees of the cooldown sweep still covering the icon: a full turn at the start, none when ready. */
export function sweepDegrees(remaining: number, total: number): number {
  if (!(total > 0) || !(remaining > 0)) {
    return 0;
  }
  return Math.min(360, (remaining / total) * 360);
}

/** Whole seconds shown on a slot while at least 2 s remain; null below that. */
export function cooldownSeconds(remainingTicks: number): number | null {
  return remainingTicks >= 2 * TICK_HZ ? Math.ceil(remainingTicks / TICK_HZ) : null;
}

/** Time left as a short label: whole seconds, then minutes from one minute. */
export function formatRemaining(ticks: number): string {
  const seconds = Math.max(1, Math.ceil(ticks / TICK_HZ));
  return seconds >= 60 ? `${Math.ceil(seconds / 60)}m` : `${seconds}s`;
}

function secondsText(ticks: number): string {
  const seconds = ticks / TICK_HZ;
  return `${Number.isInteger(seconds) ? seconds : seconds.toFixed(1)} sec`;
}

const RESOURCE_NAMES: Record<ResourceKind, string> = { rage: "Rage", focus: "Focus", mana: "Mana" };

export function resourceName(kind: ResourceKind): string {
  return RESOURCE_NAMES[kind];
}

export function rangeText(range: AbilityRange): string {
  if (range === "self") {
    return "Self";
  }
  return range === "melee" ? "Melee range" : `${range.units / UNITS_PER_METRE} m range`;
}

export type Tooltip = { title: string; lines: readonly string[]; description: string; note: string | null };

/** The tooltip for an ability: name, cost, range, cast time, cooldown and description. */
export function tooltipText(ability: AbilityRecord, catalog: ContentCatalog, viewerLevel: number): Tooltip {
  const presentation = presentationOf(ability.id);
  const resource = [...catalog.classes.values()].find((record) => record.name === ability.user)?.resource;
  const lines: string[] = [];
  lines.push(ability.cost > 0 && resource ? `${ability.cost} ${RESOURCE_NAMES[resource]}` : "No cost");
  lines.push(rangeText(presentation?.range ?? "self"));
  lines.push(
    ability.castTicks === 0
      ? "Instant"
      : `${secondsText(ability.castTicks)} ${ability.channel ? "channel" : "cast"}`,
  );
  if (ability.cooldown > 0) {
    lines.push(`${secondsText(ability.cooldown)} cooldown`);
  }
  return {
    title: ability.name,
    lines,
    description: presentation?.description ?? "",
    note: viewerLevel < ability.level ? `Unlocks at level ${ability.level}` : null,
  };
}
