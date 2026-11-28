import type { ZoneSnapshot } from "../../replication";
import type { MinimapRules, PanelViews } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { wasm } from "../../wasm";

export type {
  AuraRow, AuraRows, DepthsLootRow, DepthsLootRows, DisturbanceArea, LootButton, MinimapDot, MinimapDotClass, MinimapRules,
  PanelViews, VendorSale, WaystoneOffer,
} from "../../generated/mmorpg-wasm/mmorpg_wasm.js";

/**
 * The minimap's dot classes and raid disturbance, the unit frames' auras, the waystone panel and the depths loot rows
 * come from `mmorpg-client-model` (#1280) through `mmorpg-wasm`, the same rules the native client calls. They are
 * derived once per projection, however many frames and panels show it (Atlas 7.1.i1256): every panel asking about
 * the same projection object shares one call.
 */
let latest: { snapshot: ZoneSnapshot; views: PanelViews } | null = null;
let reads = 0;

/** The panel views of `snapshot`. */
export function panelViews(snapshot: ZoneSnapshot): PanelViews {
  if (latest?.snapshot !== snapshot) {
    reads += 1;
    latest = { snapshot, views: wasm.panelViews(snapshot) };
  }
  return latest.views;
}

/** Projections whose panel views crossed into WASM since the page loaded (the `panel-views` work counter). */
export function panelViewReads(): number {
  return reads;
}

let rules: MinimapRules | null = null;

/** The minimap's zoom radii, landmark range, building kinds and quest marker glyphs; read once per page. */
export function minimapRules(): MinimapRules {
  rules ??= wasm.minimapRules();
  return rules;
}
