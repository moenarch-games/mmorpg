import { TICK_HZ, type ResourceKind, type ZoneSnapshot } from "../../replication";
import type { SlotState } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "../../wasm";
import type { AbilityRecord, ContentCatalog } from "../catalog";
import { presentationOf } from "./ability-presentation";

export type { SlotCooldown, SlotState } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";

/**
 * What each action bar slot shows for a projection, in slot order: the shared client model's
 * slot rules (`mmorpg-client-model`, #543), the same the native client draws. None of it decides
 * whether a use succeeds; the zone answers every `UseAbility` itself.
 */
export function actionBar(snapshot: ZoneSnapshot): SlotState[] {
  return wasm.actionBar(snapshot);
}

/**
 * The action bar decoded once per projection (#1091): `actionBar` hands the whole projection to the module, so a frame
 * that shows the same projection again (a frame rate above the tick rate, a paused zone) reuses what the bar shows.
 */
export class ActionBarCache {
  #snapshot: ZoneSnapshot | null = null;
  #decodes = 0;

  /** The slots of `snapshot` when it is not the projection last asked about; null when the bar shows it already. */
  changed(snapshot: ZoneSnapshot): SlotState[] | null {
    if (snapshot === this.#snapshot) {
      return null;
    }
    this.#snapshot = snapshot;
    this.#decodes += 1;
    return actionBar(snapshot);
  }

  /** Forgets the projection, so the next one is decoded and painted whatever it is. */
  reset(): void {
    this.#snapshot = null;
  }

  /** Projections decoded so far (the `world-frame` budget's `action_bar_decodes`). */
  get decodes(): number {
    return this.#decodes;
  }
}

/** Degrees of the cooldown sweep still covering the icon: a full turn at the start, none when ready. */
export function sweepDegrees(remaining: number, total: number): number {
  if (!(total > 0) || !(remaining > 0)) {
    return 0;
  }
  return Math.min(360, (remaining / total) * 360);
}


function secondsText(ticks: number): string {
  const seconds = ticks / TICK_HZ;
  return `${Number.isInteger(seconds) ? seconds : seconds.toFixed(1)} sec`;
}

const RESOURCE_NAMES: Record<ResourceKind, string> = { rage: "Rage", focus: "Focus", mana: "Mana", faith: "Faith" };

export function resourceName(kind: ResourceKind): string {
  return RESOURCE_NAMES[kind];
}

export type Tooltip = { title: string; lines: readonly string[]; description: string; note: string | null };

/** The tooltip for an ability: name, cost, range, cast time, cooldown and description. */
export function tooltipText(ability: AbilityRecord, catalog: ContentCatalog, viewerLevel: number): Tooltip {
  const presentation = presentationOf(ability.id);
  const resource = [...catalog.classes.values()].find((record) => record.name === ability.user)?.resource;
  const lines: string[] = [];
  lines.push(ability.cost > 0 && resource ? `${ability.cost} ${RESOURCE_NAMES[resource]}` : "No cost");
  lines.push(ability.rangeLabel);
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
