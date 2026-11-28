import type { ErrorCode, ZoneSnapshot } from "../../replication";
import type { HudTexts, RefusalContext } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "../../wasm";

export type { HudTexts, RefusalContext };

/**
 * The words for the zone's answers come from `mmorpg-client-model`'s `texts` (#1278) through
 * `mmorpg-wasm`, the same rules the native client calls. The HUD's lines are derived once per
 * projection, however many frames show it (Atlas 7.1.i1256): every HUD asking about the same
 * projection object shares one call.
 */
let latest: { snapshot: ZoneSnapshot; texts: HudTexts } | null = null;
let reads = 0;

/** The combat HUD's status and feedback lines and the target's name for `snapshot`. */
export function hudTexts(snapshot: ZoneSnapshot): HudTexts {
  if (latest?.snapshot !== snapshot) {
    reads += 1;
    latest = { snapshot, texts: wasm.hudTexts(snapshot) };
  }
  return latest.texts;
}

/** Projections whose HUD texts crossed into WASM since the page loaded (the `hud-texts` work counter). */
export function hudTextReads(): number {
  return reads;
}

/** What refusal `code` says in the `context` panel, or `null` when that panel ignores it. */
export function panelRefusal(code: ErrorCode, context: RefusalContext): string | null {
  return wasm.panelRefusal(code, context) ?? null;
}
