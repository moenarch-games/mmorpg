import type { WorldCommand } from "../../command-wire";
import type { ZoneSnapshot } from "../../replication";
import type { AbilityRecord, ContentCatalog } from "../catalog";

/**
 * Ability input for the minimal presentation: slots 1–4 hold the viewer's class abilities in
 * catalog ID order (every class has four). The zone validates class, level, cooldowns, resource,
 * target and range; this module only names the ability. The action bar arrives with step 8b.
 */
export function abilitySlots(snapshot: ZoneSnapshot, catalog: ContentCatalog): readonly AbilityRecord[] {
  const classId = snapshot.viewer.classChoice?.classId;
  if (classId === undefined) {
    return [];
  }
  return [...catalog.abilities.values()]
    .filter((ability) => ability.user === classId)
    .sort((left, right) => left.id - right.id)
    .slice(0, 4);
}

/** The command for ability slot `slot` (1–4) at the current selection, or null without one. */
export function useAbilitySlot(slot: number, snapshot: ZoneSnapshot, catalog: ContentCatalog): WorldCommand | null {
  const ability = abilitySlots(snapshot, catalog)[slot - 1];
  return ability ? { kind: "use-ability", ability: ability.id, target: null } : null;
}

/** Cancels the viewer's own cast or channel; nothing to send without one. */
export function cancelCast(snapshot: ZoneSnapshot): WorldCommand | null {
  return snapshot.viewer.cast ? { kind: "cancel-cast" } : null;
}
