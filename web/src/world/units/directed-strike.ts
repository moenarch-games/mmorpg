import type { WorldCommand } from "../../command-wire";
import type { StrikeAim, ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { abilitySlots } from "./abilities";

/**
 * The Warden's directed Heroic Strike input (#154, Atlas 5.2.20/5.2.55): holding the separately
 * bound action starts preparation, mouse motion since the press picks the side (left/right) and
 * height (up is high), and only a deliberate release sends one `DirectedStrike`. The shared
 * mouse-owner rules (`mmorpg-client-model`, #636) read the motion; the zone alone decides what the
 * sweep covers; this module only names the intent.
 */
export const HEROIC_STRIKE_ABILITY = 1;

/** Whether the viewer has the ability with a directed mode in its kit (a living Warden). */
export function canStrikeDirected(snapshot: ZoneSnapshot, catalog: ContentCatalog): boolean {
  return !snapshot.viewer.dead && abilitySlots(snapshot, catalog).some((ability) => ability.id === HEROIC_STRIKE_ABILITY);
}

/** The command for a released directed strike, or null when the viewer has no such mode. */
export function directedStrikeCommand(snapshot: ZoneSnapshot, catalog: ContentCatalog, facing: number, aim: StrikeAim): WorldCommand | null {
  return canStrikeDirected(snapshot, catalog) ? { kind: "directed-strike", ability: HEROIC_STRIKE_ABILITY, facing, aim } : null;
}
