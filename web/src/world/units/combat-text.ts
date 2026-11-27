import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";

export type { CombatText, CombatTextKind } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";

/**
 * Floating combat text for the viewer's projection events: the shared client rules
 * (`mmorpg-client-model::combat_text`, #1234), the same the native client floats over its units.
 * Events are cosmetic and lossy, so the text only decorates what the self and target sections
 * already show.
 */
export function combatTexts(snapshot: ZoneSnapshot): ReturnType<typeof wasm.combatTexts> {
  return wasm.combatTexts(snapshot);
}

/**
 * The texts a stream of projections raises, each projection once: its events' texts, then a
 * cleave or pierce through several units and a mastery level gained (#909, #932, #915).
 */
export const CombatTextFeed = wasm.CombatTextFeed;
export type CombatTextFeed = InstanceType<typeof wasm.CombatTextFeed>;

/** Whether this projection interrupted the viewer's own cast. */
export function viewerInterrupted(snapshot: ZoneSnapshot): boolean {
  return wasm.viewerInterrupted(snapshot);
}

/** How long a floating text lives, in milliseconds. */
export function combatTextMs(): number {
  return wasm.combatTextMs();
}

/** The interrupt flash on the player cast bar shows for this long. */
export const INTERRUPT_FLASH_MS = 900;
