import type { WorldCommand } from "../../command-wire";
import type { StrikeAim, StrikeHeight, StrikeSide, ZoneSnapshot } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { abilitySlots } from "./abilities";

/**
 * The Warden's directed Heroic Strike input (#154, Atlas 5.2.20/5.2.55): holding the separately
 * bound action starts preparation, pointer motion since the press picks the side (left/right) and
 * height (up is high), and only a deliberate release sends one `DirectedStrike`. The zone alone
 * decides what the sweep covers; this module only names the intent.
 */
export const HEROIC_STRIKE_ABILITY = 1;
/** Pointer travel (CSS pixels) from the press before the aim leaves the centre or the middle. */
export const AIM_THRESHOLD_PX = 24;

export function aimFromDelta(dx: number, dy: number, threshold = AIM_THRESHOLD_PX): StrikeAim {
  const side: StrikeSide = dx <= -threshold ? "left" : dx >= threshold ? "right" : "centre";
  // Screen y grows downward: moving the pointer up aims high.
  const height: StrikeHeight = dy <= -threshold ? "high" : dy >= threshold ? "low" : "middle";
  return { side, height };
}

/** Whether the viewer has the ability with a directed mode in its kit (a living Warden). */
export function canStrikeDirected(snapshot: ZoneSnapshot, catalog: ContentCatalog): boolean {
  return !snapshot.viewer.dead && abilitySlots(snapshot, catalog).some((ability) => ability.id === HEROIC_STRIKE_ABILITY);
}

/** The command for a released directed strike, or null when the viewer has no such mode. */
export function directedStrikeCommand(snapshot: ZoneSnapshot, catalog: ContentCatalog, facing: number, aim: StrikeAim): WorldCommand | null {
  return canStrikeDirected(snapshot, catalog) ? { kind: "directed-strike", ability: HEROIC_STRIKE_ABILITY, facing, aim } : null;
}

/** One held preparation: where the pointer was at the press and where it is now. */
export class DirectedStrikeAim {
  #origin: { x: number; y: number } | null = null;
  #pointer: { x: number; y: number } | null = null;

  get active(): boolean {
    return this.#origin !== null;
  }

  /** Remembers the latest pointer position, held or not. */
  track(x: number, y: number): void {
    if (Number.isFinite(x) && Number.isFinite(y)) {
      this.#pointer = { x, y };
    }
  }

  begin(): void {
    this.#origin = this.#pointer ?? { x: 0, y: 0 };
  }

  aim(): StrikeAim | null {
    const origin = this.#origin;
    if (!origin) {
      return null;
    }
    const pointer = this.#pointer ?? origin;
    return aimFromDelta(pointer.x - origin.x, pointer.y - origin.y);
  }

  /** Ends the preparation; returns its aim for a deliberate release. */
  release(): StrikeAim | null {
    const aim = this.aim();
    this.#origin = null;
    return aim;
  }

  cancel(): void {
    this.#origin = null;
  }
}
