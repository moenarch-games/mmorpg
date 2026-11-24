import type { AbilityFacts, ZoneConstants } from "./generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "./wasm";

export type { AbilityFacts, ZoneConstants } from "./generated/mmorpg-wasm/mmorpg_wasm.js";

let constants: ZoneConstants | null = null;
let facts: ReadonlyMap<number, AbilityFacts> | null = null;

/**
 * The core's reaches (zone units), and the Pages day length, from `mmorpg-wasm` (#1159). The
 * browser uses them only as presentation hints; the zone checks every intent. Load the module
 * (`loadWasm`) first.
 */
export function zoneConstants(): ZoneConstants {
  constants ??= wasm.zoneConstants();
  return constants;
}

/** What each catalog ability's definition states, by ID, for holding its description to (#1159). */
export function abilityFacts(): ReadonlyMap<number, AbilityFacts> {
  facts ??= new Map(wasm.abilityFacts().map((entry) => [entry.id, entry]));
  return facts;
}
