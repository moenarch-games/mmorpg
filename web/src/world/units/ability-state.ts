import { TICK_HZ, UNITS_PER_METRE, type ResourceKind, type ZoneSnapshot } from "../../replication";
import type { SlotState } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "../../wasm";
import type { AbilityRecord, ContentCatalog } from "../catalog";
import { presentationOf, type AbilityRange } from "./ability-presentation";

export type { SlotCooldown, SlotState } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";

/**
 * What each action bar slot shows for a projection, in slot order: the shared client model's
 * slot rules (`mmorpg-client-model`, #543), the same the native client draws. None of it decides
 * whether a use succeeds; the zone answers every `UseAbility` itself.
 */
export function actionBar(snapshot: ZoneSnapshot): SlotState[] {
  return wasm.actionBar(snapshot);
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

const RESOURCE_NAMES: Record<ResourceKind, string> = { rage: "Rage", focus: "Focus", mana: "Mana", faith: "Faith" };

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
