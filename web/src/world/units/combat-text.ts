import { sameEntity, type EntityRef } from "../../entity-ref";
import { ticksFromMs, type ZoneEvent, type ZoneSnapshot } from "../../replication";

/**
 * Floating combat text for the viewer's projection events. Events are cosmetic and lossy, so the
 * text only decorates what the self and target sections already show.
 */
export type CombatTextKind = "damage" | "critical" | "periodic" | "taken" | "heal" | "absorb" | "interrupt" | "miss" | "block" | "guard-break" | "cleave" | "flank";

export type CombatText = { kind: CombatTextKind; text: string };

function isViewer(entity: EntityRef | null, snapshot: ZoneSnapshot): boolean {
  return entity !== null && entity.kind === "player" && entity.id === snapshot.viewerId;
}

/**
 * Whether the viewer's damage in this projection can only have come from a damage-over-time tick
 * or a channel pulse. Damage events carry no provenance, so this is claimed only when no other
 * source of the viewer's damage can be active this tick: no ability resolved, no swing (#517)
 * landed, and a channel is running or the target carries a damage-over-time aura. Anything
 * ambiguous shows as ordinary damage.
 */
export function damageIsPeriodic(snapshot: ZoneSnapshot): boolean {
  const resolved = snapshot.events.some((event) => (event.kind === "ability-used" || event.kind === "strike-swept" || event.kind === "attack-performed") && isViewer(event.source, snapshot));
  if (resolved) {
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
    case "trap-hit":
      return { kind: "taken", text: `-${event.amount}` };
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
    case "blocked":
      // The viewer's own shield (#156); others' blocks read in the log.
      return isViewer(event.target, snapshot) ? { kind: "block", text: `Block ${event.lane}` } : null;
    case "guard-broken":
      return isViewer(event.target, snapshot) ? { kind: "guard-break", text: "Guard broken!" } : null;
    case "strike-swept":
      // A hit shows its damage; a whiff of the viewer's own sweep says so.
      return event.target === null && isViewer(event.source, snapshot) ? { kind: "miss", text: "Whiff" } : null;
    case "flanked":
      // The viewer's Sinister Strike from its victim's flank (#949).
      return { kind: "flank", text: "Flank!" };
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

/**
 * How many units the viewer's current swing has struck after this projection, from `count` before
 * it (#909): the viewer's wind-up starts a new swing, and each of its sweeps that struck a unit
 * counts one. A cleaving timed swing strikes over several ticks, so the count spans projections.
 */
export function cleaveCount(snapshot: ZoneSnapshot, count: number): number {
  return snapshot.events.reduce((count, event) => {
    if (event.kind === "swing-began" && isViewer(event.source, snapshot)) {
      return 0;
    }
    return event.kind === "strike-swept" && event.target !== null && isViewer(event.source, snapshot) ? count + 1 : count;
  }, count);
}

/** The Ranger's Piercing Shot (#291). */
export const PIERCING_SHOT_ABILITY = 19;
/** A piercing arrow strikes everything it reaches within this many ticks of its release. */
const PIERCE_WINDOW_TICKS = BigInt(ticksFromMs(1_000));

/** The viewer's latest Piercing Shot: the tick it was loosed and how many units it struck. */
export type PierceShot = { loosed: bigint; struck: number };

/**
 * The viewer's latest Piercing Shot after this projection, from `shot` before it (#932): loosing
 * one starts a count, the viewer's next ability or attack ends it, and each of the viewer's hits
 * within a second of the release counts one. Damage events carry no provenance, so like
 * `damageIsPeriodic` this reads the viewer's own events around the hits.
 */
export function pierceShot(snapshot: ZoneSnapshot, shot: PierceShot | null): PierceShot | null {
  return snapshot.events.reduce<PierceShot | null>((shot, event) => {
    switch (event.kind) {
      case "ability-used":
        if (!isViewer(event.source, snapshot)) {
          return shot;
        }
        return event.ability === PIERCING_SHOT_ABILITY ? { loosed: snapshot.tick, struck: 0 } : null;
      case "attack-performed":
        return isViewer(event.source, snapshot) ? null : shot;
      case "damage-dealt":
        return shot !== null && isViewer(event.source, snapshot) && snapshot.tick - shot.loosed <= PIERCE_WINDOW_TICKS
          ? { ...shot, struck: shot.struck + 1 }
          : shot;
      default:
        return shot;
    }
  }, shot);
}

/** How long a floating text lives, in milliseconds. */
export const COMBAT_TEXT_MS = 1_400;

/** The interrupt flash on the player cast bar shows for this long. */
export const INTERRUPT_FLASH_MS = 900;

/** Whether this projection interrupted the viewer's own cast. */
export function viewerInterrupted(snapshot: ZoneSnapshot): boolean {
  return snapshot.events.some((event) => event.kind === "interrupted" && isViewer(event.target, snapshot));
}
