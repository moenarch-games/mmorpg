import type { WorldCommand } from "../../command-wire";
import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";
import type { AbilityRecord, ContentCatalog } from "../catalog";

/**
 * Ability input: slots 1–8 hold the viewer's class kit, the shared client model's
 * (`mmorpg-client-model`, #543). The zone validates class, level, cooldowns, resource, target and
 * range; this module only names the ability.
 */
export function abilitySlots(snapshot: ZoneSnapshot, catalog: ContentCatalog): readonly AbilityRecord[] {
  const classId = snapshot.viewer.classChoice?.classId;
  if (classId === undefined) {
    return [];
  }
  return [...wasm.classKit(classId)].flatMap((id) => catalog.abilities.get(id) ?? []);
}

/** The command for ability slot `slot` (1–8) at the current selection, or null without one. */
export function useAbilitySlot(slot: number, snapshot: ZoneSnapshot, catalog: ContentCatalog): WorldCommand | null {
  const ability = abilitySlots(snapshot, catalog)[slot - 1];
  return ability ? { kind: "use-ability", ability: ability.id, target: null } : null;
}

/** Cancels the viewer's own cast or channel; nothing to send without one. */
export function cancelCast(snapshot: ZoneSnapshot): WorldCommand | null {
  return snapshot.viewer.cast ? { kind: "cancel-cast" } : null;
}

/** The four minion group orders (#295, Atlas 5.2.84), as the `minions.*` actions name them. */
export type MinionAction = "attack" | "defend" | "follow" | "retreat";

/**
 * A Necromancer's group order for all its minions: attack the selected unit, defend where the
 * viewer stands, follow or retreat. Nothing to send for an attack without a selection; the zone
 * checks summoner, target and range.
 */
export function minionCommand(action: MinionAction, snapshot: ZoneSnapshot): WorldCommand | null {
  switch (action) {
    case "attack": {
      const target = snapshot.viewer.target;
      return target ? { kind: "command-minions", order: { order: "attack", target } } : null;
    }
    case "defend": {
      const me = snapshot.entities.find((entity) => entity.kind === "player" && entity.entityId === snapshot.viewerId);
      return me ? { kind: "command-minions", order: { order: "defend", point: [me.position[0], me.position[2]] } } : null;
    }
    case "follow":
    case "retreat":
      return { kind: "command-minions", order: { order: action } };
  }
}
