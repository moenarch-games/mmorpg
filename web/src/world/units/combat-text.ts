import { sameEntity, type EntityRef } from "../../entity-ref";
import type { ZoneEvent, ZoneSnapshot } from "../../replication";

/**
 * Floating combat text for the viewer's projection events. Events are cosmetic and lossy, so the
 * text only decorates what the self and target sections already show.
 */
export type CombatTextKind = "damage" | "critical" | "periodic" | "taken" | "heal" | "absorb" | "interrupt" | "miss";

export type CombatText = { kind: CombatTextKind; text: string };

function isViewer(entity: EntityRef | null, snapshot: ZoneSnapshot): boolean {
  return entity !== null && entity.kind === "player" && entity.id === snapshot.viewerId;
}

/**
 * Whether the viewer's damage in this projection can only have come from a damage-over-time tick
 * or a channel pulse. Damage events carry no provenance, so this is claimed only when no other
 * source of the viewer's damage can be active this tick: no ability resolved, no auto-attack
 * runs, and a channel is running or the target carries a damage-over-time aura. Anything
 * ambiguous (a Serpent Sting tick beside auto-shots, a swing during Blizzard) shows as ordinary
 * damage.
 */
export function damageIsPeriodic(snapshot: ZoneSnapshot): boolean {
  const resolved = snapshot.events.some((event) => event.kind === "ability-used" && isViewer(event.source, snapshot));
  if (resolved || snapshot.viewer.autoAttacking) {
    return false;
  }
  return Boolean(snapshot.viewer.cast?.channel)
    || snapshot.targetDetail.auras.some((aura) => aura.kind === "damage-over-time");
}

function textFor(event: ZoneEvent, snapshot: ZoneSnapshot, periodic: boolean): CombatText | null {
  switch (event.kind) {
    case "damage-dealt":
      return {
        kind: event.critical ? "critical" : periodic ? "periodic" : "damage",
        text: event.critical ? `${event.amount}!` : `${event.amount}`,
      };
    case "damage-taken":
      return { kind: "taken", text: `-${event.amount}${event.critical ? "!" : ""}` };
    case "miss":
      return isViewer(event.source, snapshot) ? { kind: "miss", text: "Miss" } : null;
    case "evade":
      return { kind: "miss", text: "Evade" };
    case "healed":
      // The viewer's own heals, heals on it, and heals on its target (a creature bandaging itself).
      return isViewer(event.target, snapshot) || isViewer(event.source, snapshot)
        || (snapshot.viewer.target !== null && sameEntity(event.target, snapshot.viewer.target))
        ? { kind: "heal", text: `+${event.amount}` }
        : null;
    case "absorbed":
      return { kind: "absorb", text: "Absorb" };
    case "interrupted":
      return { kind: "interrupt", text: isViewer(event.target, snapshot) ? "Interrupted" : "Interrupt" };
    default:
      return null;
  }
}

/** The floating texts one projection's events produce, in event order. */
export function combatTexts(snapshot: ZoneSnapshot): readonly CombatText[] {
  const periodic = damageIsPeriodic(snapshot);
  return snapshot.events.flatMap((event) => {
    const text = textFor(event, snapshot, periodic);
    return text ? [text] : [];
  });
}

/** How long a floating text lives, in milliseconds. */
export const COMBAT_TEXT_MS = 1_400;

/** The interrupt flash on the player cast bar shows for this long. */
export const INTERRUPT_FLASH_MS = 900;

/** Whether this projection interrupted the viewer's own cast. */
export function viewerInterrupted(snapshot: ZoneSnapshot): boolean {
  return snapshot.events.some((event) => event.kind === "interrupted" && isViewer(event.target, snapshot));
}
