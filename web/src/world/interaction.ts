import type { InteractPress } from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import type { QuestSheet, ZoneSnapshot } from "../replication";
import { wasm } from "../wasm";

export type { InteractPress };

/**
 * What a press of Interact does now for the viewer of `snapshot` with the active quests of `sheet` (#1252): the shared
 * client model's one ladder, which the native client reads too; the zone checks every intent again.
 */
export function interactPress(snapshot: ZoneSnapshot, sheet: QuestSheet | null): InteractPress {
  return wasm.interactPress(snapshot, sheet);
}

/** What Interact would do with free hands (#1252), for a button that names it; never `sheathe-first`. */
export function nextInteraction(snapshot: ZoneSnapshot, sheet: QuestSheet | null): InteractPress {
  return wasm.nextInteraction(snapshot, sheet);
}
